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

#[test]
fn the_same_call_with_the_same_result_is_told_at_three_and_stopped_at_five() {
    let home = Home::new();
    std::fs::write(home.0.join("a.txt"), "alpha").unwrap();
    // The model reads the same unchanged file round after round, then answers the closing request.
    let mut replies: Vec<_> = (1..=5).map(|i| tools(vec![tool_call(&format!("s{i}"), "read_file", json!({"path": "a.txt"}))])).collect();
    replies.push(text("I kept reading a.txt; I need to know what to change in it."));
    let (url, server) = gateway(replies);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "fix a.txt"}));
    live.wait_for(|e| e["type"] == "turn_done");
    let seen = live.seen.clone();
    live.finish();
    let requests = server.join().unwrap();

    // Five rounds and the closing request: the 100-round ceiling never came into it.
    assert_eq!(requests.len(), 6);
    // The 3rd identical result reached the model with the note, next to the output it repeated.
    let terceira = last(&requests[3]);
    assert_eq!(terceira["tool_call_id"], "s3");
    assert!(terceira["content"].as_str().unwrap().contains("[aviso do anna: read_file com os mesmos argumentos"), "{terceira}");
    assert!(!last(&requests[2])["content"].as_str().unwrap().contains("aviso do anna"), "not before the 3rd");
    // The 5th stopped the turn: a host is never asked, the closing request names the repeat.
    assert!(last(&requests[5])["content"].as_str().unwrap().contains("você repetiu read_file"), "{}", last(&requests[5]));
    assert!(seen.iter().any(|e| e["type"] == "warn" && e["message"].as_str().unwrap().starts_with("turno parado")));
    assert!(seen.iter().any(|e| e["type"] == "text" && e["delta"] == "I kept reading a.txt; I need to know what to change in it."));
}

#[test]
fn the_prompt_carries_the_project_rules_and_a_subfolder_with_its_own_is_pointed_out_on_first_touch() {
    let home = Home::new();
    std::fs::create_dir_all(home.0.join(".git")).unwrap();
    std::fs::write(home.0.join("CLAUDE.md"), "# Root\nRoot rules.").unwrap();
    std::fs::create_dir_all(home.0.join("pkg/src")).unwrap();
    std::fs::write(home.0.join("pkg/AGENTS.md"), "# pkg\nUse pnpm.").unwrap();
    std::fs::write(home.0.join("pkg/src/main.ts"), "export {}").unwrap();
    let (url, server) = gateway(vec![
        tools(vec![tool_call("r1", "read_file", json!({"path": "pkg/src/main.ts"}))]),
        text("ok"),
    ]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "look at pkg"}));
    live.wait_for(|e| e["type"] == "turn_done");
    live.finish();
    let requests = server.join().unwrap();

    let sistema = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(sistema.contains("--- CLAUDE.md ---\n# Root\nRoot rules."), "{sistema}");
    let leitura = last(&requests[1]);
    assert_eq!(leitura["tool_call_id"], "r1");
    assert!(leitura["content"].as_str().unwrap().contains("instruções próprias em pkg/AGENTS.md"), "{leitura}");
}
