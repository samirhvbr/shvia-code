// The loop against gateway failures, end to end: the real `anna --json` against the
// scripted loopback gateway in `common`.
#![cfg(unix)]

mod common;

use common::{events, gateway, text, Home, Reply};
use serde_json::json;

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
