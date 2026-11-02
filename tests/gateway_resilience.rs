// The loop against gateway failures, end to end: the real `anna --json` against the
// scripted loopback gateway in `common`.
#![cfg(unix)]

mod common;

use common::{events, gateway, text, wait_success, Home, Live, Reply};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id": id, "name": name, "arguments": arguments})
}

/// A round that asks for these tools.
fn tools(calls: Vec<Value>) -> Reply {
    let mut evs: Vec<Value> = calls.iter().map(|c| json!({"kind": "tool_call", "data": c})).collect();
    evs.push(json!({"kind": "done", "data": {"text": "", "tool_calls": calls}}));
    Reply::Sse(evs)
}

fn saved_session(home: &Home) -> Value {
    let dir = home.0.join(".shvia-code/sessions");
    let files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 1, "{files:?}");
    serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap()
}

#[test]
fn a_rate_limited_round_is_retried_after_the_gateways_retry_after() {
    let home = Home::new();
    // The per-key throttle answers 429 with Retry-After (0 here, to keep the test fast);
    // the retry must reach the gateway again and finish the turn.
    let (url, server) = gateway(vec![
        Reply::Status {
            code: 429,
            headers: vec![("Retry-After", "0".into())],
            body: json!({"message": "Too Many Attempts."}),
        },
        text("Done."),
    ]);
    let output = home.run(&url, &["--json"], &format!("{}\n", json!({"type": "user", "text": "hi"})));
    let requests = server.join().unwrap();
    let events = events(&output);

    assert_eq!(requests.len(), 2, "the round was not retried");
    assert_eq!(requests[0]["messages"], requests[1]["messages"], "the retry must resend the same history");
    assert!(
        events.iter().any(|e| e["type"] == "warn" && e["message"].as_str().unwrap().contains("429")),
        "the wait must be visible: {events:?}"
    );
    assert!(!events.iter().any(|e| e["type"] == "error"), "{events:?}");
    assert!(events.iter().any(|e| e["type"] == "text" && e["delta"] == "Done."));
}

#[test]
fn ctrl_c_stops_the_turn_not_anna_and_each_round_is_saved_before_the_turn_ends() {
    let home = Home::new();
    std::fs::write(home.0.join("notes.txt"), "remember the milk").unwrap();
    let (streaming, sent) = mpsc::channel();
    let (url, server) = gateway(vec![
        tools(vec![tool_call("c1", "read_file", json!({"path": "notes.txt"}))]),
        Reply::Hang { events: vec![json!({"kind": "text_delta", "data": {"delta": "Reading the"}})], sent: streaming },
    ]);
    let mut child = home.spawn(&url, &[]);
    child.stdin.take().unwrap().write_all(b"summarize notes.txt\n/exit\n").unwrap();

    // The second round is streaming, so the first round's tools already ran: the session
    // must already hold them, before the turn ends (per-round autosave).
    sent.recv_timeout(common::TIMEOUT).expect("second round never streamed");
    let mid_turn = saved_session(&home);
    let roles: Vec<&str> = mid_turn["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles.last(), Some(&"tool"), "the tool round was not saved mid-turn: {roles:?}");

    let pid = child.id().to_string();
    assert!(std::process::Command::new("kill").args(["-INT", &pid]).status().unwrap().success());
    // anna must survive the Ctrl-C, read `/exit` and leave normally.
    let output = wait_success(child);
    server.join().unwrap();

    let last = saved_session(&home)["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "assistant", "{last}");
    let content = last["content"].as_str().unwrap();
    assert!(content.starts_with("Reading the") && content.contains("[interrompido pelo usuário]"), "{content}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("turno interrompido"));
}

#[test]
fn ndjson_cancel_kills_the_running_command_and_answers_every_pending_call() {
    let home = Home::new();
    std::fs::write(home.0.join("notes.txt"), "remember the milk").unwrap();
    let (url, server) = gateway(vec![
        tools(vec![
            tool_call("c1", "bash", json!({"command": "sleep 30", "why": "wait"})),
            tool_call("c2", "read_file", json!({"path": "notes.txt"})),
        ]),
        text("Ok."),
    ]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "wait, then read"}));
    live.wait_for(|e| e["type"] == "gate_request");
    live.send(json!({"decision": "approve"}));
    std::thread::sleep(Duration::from_millis(300));
    let stopped_at = Instant::now();
    live.send(json!({"type": "cancel"}));
    live.wait_for(|e| e["type"] == "turn_done");
    assert!(stopped_at.elapsed() < Duration::from_secs(10), "the command was not killed: {:?}", stopped_at.elapsed());
    assert!(live.seen.iter().any(|e| e["type"] == "interrupted"), "{:?}", live.seen);

    // The next turn carries a valid history: one answer per call, then the new request.
    live.send(json!({"type": "user", "text": "go on"}));
    live.wait_for(|e| e["type"] == "turn_done");
    live.finish();
    let requests = server.join().unwrap();
    let msgs = requests[1]["messages"].as_array().unwrap();
    let n = msgs.len();
    assert_eq!(msgs[n - 4]["tool_calls"].as_array().unwrap().len(), 2);
    assert_eq!((msgs[n - 3]["tool_call_id"].as_str(), msgs[n - 2]["tool_call_id"].as_str()), (Some("c1"), Some("c2")));
    assert!(msgs[n - 3]["content"].as_str().unwrap().contains("interrompeu o turno"), "{}", msgs[n - 3]);
    assert!(msgs[n - 2]["content"].as_str().unwrap().starts_with("RECUSADO"), "{}", msgs[n - 2]);
    assert_eq!(msgs[n - 1], json!({"role": "user", "content": "go on"}));
}

#[test]
fn a_provider_refusal_is_retried_once_with_a_shorter_history() {
    let home = Home::new();
    // A resumed session long enough to cut, against a gateway that does not announce a
    // window: the provider's refusal is the only signal that the context is full.
    let mut history = vec![json!({"role": "system", "content": "seed"}), json!({"role": "user", "content": "first request"})];
    for i in 0..30 {
        history.push(json!({"role": "assistant", "content": format!("step {i}")}));
        history.push(json!({"role": "user", "content": format!("more {i}")}));
    }
    let sessions = home.0.join(".shvia-code/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("42.json"), json!({"id": "42", "title": "t", "messages": history}).to_string()).unwrap();
    let (url, server) = gateway(vec![
        Reply::Sse(vec![
            json!({"kind": "error", "data": {"message": "prompt too long", "codigo": "provedor_recusou_pedido", "status": 400}}),
            json!({"kind": "done", "data": {"text": "", "tool_calls": []}}),
        ]),
        text("Done."),
    ]);
    home.run(&url, &["sessions", "resume", "42"], "next\n/exit\n");
    let requests = server.join().unwrap();

    let (first, retry) = (requests[0]["messages"].as_array().unwrap(), requests[1]["messages"].as_array().unwrap());
    assert!(retry.len() < first.len() / 2 + 8, "the retry must be cut harder: {} → {}", first.len(), retry.len());
    let has = |msgs: &[Value], content: &str| msgs.iter().any(|m| m["content"] == content);
    assert!(has(retry, "first request") && has(retry, "next"), "the requests that anchor the work stay");
    assert!(retry.iter().any(|m| m["content"].as_str().unwrap().starts_with("[Contexto compactado]")));
}
