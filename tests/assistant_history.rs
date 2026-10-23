// HOME isolates the real CLI's config and session store on Unix. Windows uses
// native known folders instead, so these process-level fixtures run on Unix only.
#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const FIRST: &str = "How should I parse config?";
const ANSWER: &str = "Use parse_config.";
const FOLLOW_UP: &str = "Use your previous answer.";
const TIMEOUT: Duration = Duration::from_secs(10);

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "anna-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, url: &str, args: &[&str], input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_anna"))
            .args(args)
            .current_dir(&self.0)
            .env_clear()
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("SHVIA_URL", url)
            .env("SHVIA_API_KEY", "synthetic-test-key")
            .env("TERM", "dumb")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let deadline = Instant::now() + TIMEOUT;
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "CLI did not exit: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gateway() -> (String, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for answer in [ANSWER, "Done."] {
            let deadline = Instant::now() + TIMEOUT;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "gateway request timed out");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("gateway accept failed: {e}"),
                }
            };
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            requests.push(read_request(&mut stream));
            let body = format!(
                "data: {}\n\ndata: {}\n\n",
                json!({"kind": "text_delta", "data": {"delta": answer}}),
                json!({"kind": "done", "data": {"text": answer, "tool_calls": []}})
            );
            write!(stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            ).unwrap();
        }
        requests
    });
    (url, server)
}

fn read_request(stream: &mut TcpStream) -> Value {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.starts_with("POST /api/v1/code/chat HTTP/1.1"));
    let mut length = None;
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
    }
    let mut body = vec![0; length.expect("JSON request content length")];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn conversation(messages: &Value) -> Vec<Value> {
    messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] != "system")
        .cloned()
        .collect()
}

fn first_turn() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": FIRST}),
        json!({"role": "assistant", "content": ANSWER}),
    ]
}

#[test]
fn ndjson_follow_up_receives_the_previous_final_answer_once() {
    let home = Home::new();
    let (url, server) = gateway();
    let input = format!(
        "{}\n{}\n",
        json!({"type": "user", "text": FIRST}),
        json!({"type": "user", "text": FOLLOW_UP})
    );
    let output = home.run(&url, &["--json"], &input);
    let requests = server.join().unwrap();
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "turn_done")
            .count(),
        2
    );
    assert!(events
        .iter()
        .any(|event| event["type"] == "text" && event["delta"] == ANSWER));
    let mut expected = first_turn();
    expected.push(json!({"role": "user", "content": FOLLOW_UP}));
    assert_eq!(conversation(&requests[1]["messages"]), expected);
}

#[test]
fn orchestrator_json_retry_keeps_the_invalid_answer_once() {
    let home = Home::new();
    let (url, server) = gateway();
    // Neither fixture answer is a plan: both attempts fail before the approval
    // gate or executor. The second request must include the first answer once.
    home.run(&url, &[], &format!("/plano {FIRST}\n/exit\n"));
    let requests = server.join().unwrap();
    let retry = conversation(&requests[1]["messages"]);
    assert_eq!(&retry[..2], first_turn().as_slice());
    assert_eq!(retry.len(), 3);
    assert_eq!(retry[2]["role"], "user");
    assert!(retry[2]["content"]
        .as_str()
        .unwrap()
        .contains("Sua resposta não passou na validação"));
}

#[test]
fn repl_autosave_and_resume_preserve_the_final_answer() {
    let home = Home::new();
    let (url, server) = gateway();
    home.run(&url, &[], &format!("{FIRST}\n/exit\n"));
    let sessions: Vec<_> = std::fs::read_dir(home.0.join(".shvia-code/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(sessions.len(), 1);
    let saved: Value = serde_json::from_slice(&std::fs::read(&sessions[0]).unwrap()).unwrap();
    home.run(
        &url,
        &["sessions", "resume", saved["id"].as_str().unwrap()],
        &format!("{FOLLOW_UP}\n/exit\n"),
    );
    let requests = server.join().unwrap();
    assert_eq!(conversation(&saved["messages"]), first_turn());
    let mut expected = first_turn();
    expected.push(json!({"role": "user", "content": FOLLOW_UP}));
    assert_eq!(conversation(&requests[1]["messages"]), expected);
    expected.push(json!({"role": "assistant", "content": "Done."}));
    let resumed: Value = serde_json::from_slice(&std::fs::read(&sessions[0]).unwrap()).unwrap();
    assert_eq!(conversation(&resumed["messages"]), expected);
}
