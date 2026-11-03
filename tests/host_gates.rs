// The NDJSON gate handshake, end to end: the real `anna --json` against the scripted gateway.
#![cfg(unix)]

mod common;

use common::{gateway, text, tool_call, tools, Home, Live};
use serde_json::json;

#[test]
fn a_decision_for_another_gate_never_decides_this_one() {
    let home = Home::new();
    let (url, server) = gateway(vec![
        tools(vec![tool_call("g1", "bash", json!({"command": "touch criado.txt", "why": "create"}))]),
        text("ok"),
    ]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "create the file"}));
    let pedido = live.wait_for(|e| e["type"] == "gate_request");
    assert_eq!(pedido["id"], "g1");
    // A late approval for a gate that no longer exists arrives first; then the person's real answer.
    live.send(json!({"id": "velho", "decision": "approve"}));
    live.send(json!({"id": "g1", "decision": "reject"}));
    live.wait_for(|e| e["type"] == "turn_done");
    let seen = live.seen.clone();
    live.finish();
    let requests = server.join().unwrap();

    assert!(!home.0.join("criado.txt").exists(), "the stale approval ran the command");
    assert!(seen.iter().any(|e| e["type"] == "warn" && e["message"].as_str().unwrap().contains("outro gate (velho)")), "{seen:?}");
    let resultado = requests[1]["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(resultado["tool_call_id"], "g1");
    assert!(resultado["content"].as_str().unwrap().starts_with("RECUSADO"), "{resultado}");
}
