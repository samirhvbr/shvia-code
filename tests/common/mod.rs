// Shared fixtures for the process-level tests: an isolated HOME and a loopback gateway
// that answers each request from a script and records what it received. The real `anna`
// binary runs against it, so these tests cover the loop, the protocol and the retry/stop
// paths end to end, which unit tests of single functions cannot.
//
// HOME isolates the real CLI's config and session store on Unix only (Windows uses known
// folders), so the tests that include this module are `#![cfg(unix)]`.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Home(pub PathBuf);

impl Home {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "anna-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub fn spawn(&self, url: &str, args: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_anna"))
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
            .unwrap()
    }

    /// Runs the CLI with all of `input` on stdin and waits for it to exit successfully.
    pub fn run(&self, url: &str, args: &[&str], input: &str) -> Output {
        let mut child = self.spawn(url, args);
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        wait_success(child)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn wait_success(mut child: Child) -> Output {
    let deadline = Instant::now() + TIMEOUT;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("CLI did not exit: {}", String::from_utf8_lossy(&output.stderr));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    output
}

/// NDJSON events printed by `anna --json`.
pub fn events(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// One scripted answer from the fake gateway.
pub enum Reply {
    /// `200 text/event-stream` with these `{kind, data}` events.
    Sse(Vec<Value>),
    /// A response before any stream: status, extra headers, JSON body.
    Status { code: u16, headers: Vec<(&'static str, String)>, body: Value },
}

/// A final answer with no tool calls.
pub fn text(answer: &str) -> Reply {
    Reply::Sse(vec![
        json!({"kind": "text_delta", "data": {"delta": answer}}),
        json!({"kind": "done", "data": {"text": answer, "tool_calls": []}}),
    ])
}

/// Serves `replies` in order, one per request, and returns every request body it received.
pub fn gateway(replies: Vec<Reply>) -> (String, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let mut stream = accept(&listener);
            requests.push(read_request(&mut stream));
            match reply {
                Reply::Sse(events) => {
                    let body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
                    write!(stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(), body
                    ).unwrap();
                }
                Reply::Status { code, headers, body } => {
                    let body = body.to_string();
                    let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
                    write!(stream,
                        "HTTP/1.1 {code} Scripted\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(), body
                    ).unwrap();
                }
            }
        }
        requests
    });
    (url, server)
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + TIMEOUT;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "gateway request timed out");
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("gateway accept failed: {e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    stream
}

fn read_request(stream: &mut TcpStream) -> Value {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.starts_with("POST /api/v1/code/chat HTTP/1.1"), "{line}");
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
