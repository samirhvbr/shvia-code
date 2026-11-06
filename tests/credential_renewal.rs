// A host replaces the gateway key while `anna` runs (0.16.0): the real `anna --json` against
// the scripted loopback gateway in `common`.
//
// Measured on 06/11/2026: SHVIA-DESKTOP passes the web session's Code token, valid for 12
// hours; an autonomous run paused overnight resumed with an expired token, the first request
// got a 401 and the run ended. These tests pin the way out that keeps the session alive.
#![cfg(unix)]

mod common;

use common::{gateway, text, Home, Live, Reply};
use serde_json::json;

fn recusada() -> Reply {
    Reply::Status { code: 401, headers: vec![], body: json!({"message": "Unauthenticated."}) }
}

#[test]
fn a_401_asks_the_host_for_a_key_and_repeats_the_same_request_with_it() {
    let home = Home::new();
    let (url, server) = gateway(vec![recusada(), text("Done.")]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "go on"}));
    live.wait_for(|e| e["type"] == "credential_expired");
    live.send(json!({"type": "credential", "api_key": "renewed-key"}));
    live.wait_for(|e| e["type"] == "turn_done");
    let seen = live.seen.clone();
    live.finish();
    let requests = server.join().unwrap();

    assert_eq!(requests.len(), 2, "the refused request was not repeated");
    assert_eq!(requests[0]["__api_key"], "synthetic-test-key");
    assert_eq!(requests[1]["__api_key"], "renewed-key", "the retry must carry the host's new key");
    assert_eq!(requests[0]["messages"], requests[1]["messages"], "the retry must resend the same history");
    assert!(!seen.iter().any(|e| e["type"] == "error"), "{seen:?}");
    // The key never comes back out: not in an event, not in a warning.
    assert!(!serde_json::to_string(&seen).unwrap().contains("renewed-key"), "{seen:?}");
}

#[test]
fn a_key_sent_between_turns_is_the_one_the_next_turn_uses() {
    let home = Home::new();
    let (url, server) = gateway(vec![text("One."), text("Two.")]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "first"}));
    live.wait_for(|e| e["type"] == "turn_done");
    live.send(json!({"type": "credential", "api_key": "fresh-key"}));
    live.send(json!({"type": "user", "text": "second"}));
    live.wait_for(|e| e["type"] == "turn_done");
    live.finish();
    let requests = server.join().unwrap();

    assert_eq!(requests[0]["__api_key"], "synthetic-test-key");
    assert_eq!(requests[1]["__api_key"], "fresh-key");
    // The credential line is not a turn: the second request holds the two user messages only.
    let users = requests[1]["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "user").count();
    assert_eq!(users, 2);
}

#[test]
fn a_second_401_with_the_new_key_ends_the_turn_with_the_error() {
    let home = Home::new();
    let (url, server) = gateway(vec![recusada(), recusada()]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "go on"}));
    live.wait_for(|e| e["type"] == "credential_expired");
    live.send(json!({"type": "credential", "api_key": "also-refused"}));
    let erro = live.wait_for(|e| e["type"] == "error");
    let pedidos = live.seen.iter().filter(|e| e["type"] == "credential_expired").count();
    live.finish();
    let requests = server.join().unwrap();

    assert_eq!(requests.len(), 2, "a fresh key refused again is not an expired key: no third try");
    assert_eq!(pedidos, 1);
    assert!(erro["message"].as_str().unwrap().contains("401"), "{erro}");
}
