// Exercise the real HTTP clients, including hook tasks, with synthetic credentials.
#![cfg(unix)]

mod common;

use common::{events, gateway, text, Home, Live};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PROMPT: &str = "synthetic confidential project content";
const STATUSES: &[u16] = &[307, 308, 301, 302, 303];

#[derive(Clone, Debug)]
struct Request {
    target: String,
    headers: Vec<(String, String)>,
    body: Value,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(redirect: Option<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let (stop, stopped) = mpsc::channel();
        let thread = thread::spawn(move || {
            while stopped.try_recv().is_err() {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(common::TIMEOUT)).unwrap();
                stream.set_write_timeout(Some(common::TIMEOUT)).unwrap();
                let request = read_request(&mut stream);
                let redirected = request.target == "/redirected";
                captured.lock().unwrap().push(request);
                if let Some((status, location)) = &redirect {
                    if !redirected {
                        write!(stream, "HTTP/1.1 {status} Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                        continue;
                    }
                }
                let body = format!(
                    "data: {}\n\n",
                    json!({"kind": "done", "data": {"text": "Done.", "tool_calls": []}})
                );
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn wait_for_prompt(&self) {
        let deadline = Instant::now() + common::TIMEOUT;
        while !self
            .requests()
            .iter()
            .any(|request| request.body["prompt"] == PROMPT)
        {
            assert!(Instant::now() < deadline, "memory prompt was not sent");
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let target = line.split_whitespace().nth(1).unwrap().to_owned();
    let mut headers = Vec::new();
    let mut length = 0;
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.split_once(':').unwrap();
        if key.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().unwrap();
        }
        headers.push((key.to_owned(), value.trim().to_owned()));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Request {
        target,
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    }
}

#[test]
fn gateway_rejects_redirects_without_forwarding_credentials_or_history() {
    for &status in STATUSES {
        for same_origin in [false, true] {
            let home = Home::new();
            let destination = Server::start(None);
            let location = if same_origin {
                "/redirected".into()
            } else {
                format!("{}/redirected", destination.url)
            };
            let source = Server::start(Some((status, location)));
            let output = home.run(
                &source.url,
                &["--json"],
                &format!("{}\n", json!({"type": "user", "text": PROMPT})),
            );
            let requests = source.requests();
            assert_eq!(
                requests.len(),
                1,
                "HTTP {status}, same origin {same_origin}: {requests:?}"
            );
            assert_eq!(requests[0].header("x-api-key"), Some("synthetic-test-key"));
            assert!(requests[0].body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"] == PROMPT));
            assert!(
                destination.requests().is_empty(),
                "HTTP {status} reached the redirect destination: {:?}",
                destination.requests()
            );
            let events = events(&output);
            assert!(
                events.iter().any(|event| event["type"] == "error"
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("redirecionamento bloqueado")
                            && message.contains(&status.to_string()))),
                "{events:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|event| event["type"] == "text" || event["type"] == "warn"),
                "redirect was accepted or retried: {events:?}"
            );
        }
    }
}

#[test]
fn memory_does_not_follow_redirects_and_still_sends_direct_hooks() {
    for status in std::iter::once(None).chain(STATUSES.iter().copied().map(Some)) {
        for same_origin in [false, true] {
            let home = Home::new();
            let destination = Server::start(None);
            let location = if same_origin {
                "/redirected".into()
            } else {
                format!("{}/redirected", destination.url)
            };
            let memory = Server::start(status.map(|status| (status, location)));
            let (url, gateway) = gateway(vec![text("Done.")]);
            let mut anna = Live::start_env(
                &home,
                &url,
                &[
                    ("AI_MEMORY_URL", &memory.url),
                    ("AI_MEMORY_TOKEN", "synthetic-memory-key"),
                ],
            );
            anna.send(json!({"type": "user", "text": PROMPT}));
            anna.wait_for(|event| event["type"] == "turn_done");
            memory.wait_for_prompt();
            assert!(anna
                .seen
                .iter()
                .any(|event| event["type"] == "text" && event["delta"] == "Done."));
            assert!(!anna.seen.iter().any(|event| event["type"] == "error"));
            anna.finish();
            let requests = memory.requests();
            assert!(
                requests
                    .iter()
                    .all(|request| request.target.starts_with("/hook?")
                        && request.header("authorization") == Some("Bearer synthetic-memory-key")),
                "HTTP {status:?}: {requests:?}"
            );
            assert!(requests
                .iter()
                .any(|request| request.target.contains("event=session-end")));
            assert!(
                destination.requests().is_empty(),
                "HTTP {status:?} sent a hook to the redirect destination"
            );
            assert_eq!(
                gateway.join().unwrap().len(),
                1,
                "memory failure must not affect inference"
            );
        }
    }
}
