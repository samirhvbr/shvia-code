// How a turn ends when it cannot go on: the round ceiling and the stall guard, end to end, with
// the real `anna --json` against the scripted loopback gateway in `common`.
#![cfg(unix)]

mod common;

use common::{gateway, text, tool_call, tools, Home, Live};
use serde_json::{json, Value};

fn last(request: &Value) -> Value {
    request["messages"].as_array().unwrap().last().unwrap().clone()
}

#[test]
fn the_ceiling_ends_the_turn_with_a_summary_that_stays_and_a_request_that_does_not() {
    let home = Home::new();
    std::fs::write(home.0.join("a.txt"), "alpha").unwrap();
    std::fs::write(home.0.join("b.txt"), "beta").unwrap();
    let (url, server) = gateway(vec![
        tools(vec![tool_call("c1", "read_file", json!({"path": "a.txt"}))]),
        tools(vec![tool_call("c2", "read_file", json!({"path": "b.txt"}))]),
        text("Done: read a.txt and b.txt. Left: c.txt."),
        text("ok"),
    ]);
    let mut live = Live::start_env(&home, &url, &[("SHVIA_MAX_VOLTAS", "2")]);
    live.send(json!({"type": "user", "text": "read everything"}));
    live.wait_for(|e| e["type"] == "turn_done");
    live.send(json!({"type": "user", "text": "continua"}));
    live.wait_for(|e| e["type"] == "turn_done");
    let seen = live.seen.clone();
    live.finish();
    let requests = server.join().unwrap();

    // Two rounds, then one more request asking for the last word, with the same tools.
    let pedido = last(&requests[2]);
    assert_eq!(pedido["role"], "user");
    assert!(pedido["content"].as_str().unwrap().contains("teto de 2 voltas"), "{pedido}");
    assert_eq!(requests[2]["tools"], requests[1]["tools"], "the closing request keeps the tools (cached prefix)");
    assert!(seen.iter().any(|e| e["type"] == "text" && e["delta"] == "Done: read a.txt and b.txt. Left: c.txt."));
    assert!(seen.iter().any(|e| e["type"] == "warn" && e["message"].as_str().unwrap().contains("teto de 2 voltas")));

    // The next turn carries the summary as the assistant's last word, and not the asking message.
    let msgs = requests[3]["messages"].as_array().unwrap();
    let n = msgs.len();
    assert_eq!(msgs[n - 3]["tool_call_id"], "c2");
    assert_eq!(msgs[n - 2], json!({"role": "assistant", "content": "Done: read a.txt and b.txt. Left: c.txt."}));
    assert_eq!(msgs[n - 1], json!({"role": "user", "content": "continua"}));
}
