// The NDJSON gate handshake, end to end: the real `anna --json` against the scripted gateway.
#![cfg(unix)]

mod common;

use common::{gateway, text, tool_call, tools, Home, Live};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn assert_shell_refused(home: &Home, command: &str, env: &[(&str, &str)]) {
    let (url, server) = gateway(vec![
        tools(vec![tool_call("shell", "bash", json!({"command": command}))]),
        text("Done."),
    ]);
    let mut live = Live::start_env(home, &url, env);
    live.send(json!({"type": "user", "text": "Run the check"}));
    let event = live.wait_for(|e| e["type"] == "gate_request" || e["type"] == "turn_done");
    if event["type"] == "gate_request" {
        live.send(json!({"id": "shell", "decision": "reject"}));
        live.wait_for(|e| e["type"] == "turn_done");
    }
    live.finish();
    let requests = server.join().unwrap();
    assert!(!home.0.join("ran.txt").exists(), "shell executed {command:?}");
    assert_eq!(event["type"], "gate_request", "no approval for {command:?}");
    assert_eq!(event["id"], "shell");
    let result = requests[1]["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(result["tool_call_id"], "shell");
    assert!(result["content"].as_str().unwrap().starts_with("RECUSADO"), "{result}");
}

#[test]
fn unquoted_newline_cannot_start_a_second_automatic_command() {
    let home = Home::new();
    assert_shell_refused(&home, "echo reading\nsh -c 'touch ran.txt'", &[]);
}

#[test]
fn ripgrep_preprocessor_cannot_start_without_approval() {
    let home = Home::new();
    // A marker records whether the shell reaches rg, independently of the installed version.
    let rg = home.0.join("rg");
    std::fs::write(&rg, "#!/bin/sh\ntouch ran.txt\n").unwrap();
    std::fs::set_permissions(&rg, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin", home.0.display());
    assert_shell_refused(&home, "rg --pre=sh needle README.md", &[("PATH", &path)]);
}


#[test]
fn automatic_git_reads_skip_external_diff_and_fsmonitor() {
    let home = Home::new();
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .env("HOME", &home.0)
            .current_dir(&home.0)
            .args(args)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    };
    git(&["init", "-q", "--template="]);
    let note = home.0.join("note.txt");
    std::fs::write(&note, "old fixture\n").unwrap();
    git(&["add", "note.txt"]);
    std::fs::write(&note, "new fixture\n").unwrap();
    let driver = home.0.join("driver.sh");
    std::fs::write(&driver, "#!/bin/sh\ntouch ran.txt\n").unwrap();
    std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o700)).unwrap();
    git(&["config", "diff.external", "./driver.sh"]);
    git(&["config", "core.fsmonitor", "./driver.sh"]);

    let (url, server) = gateway(vec![
        tools(vec![
            tool_call("shell", "bash", json!({"command": "git status --short && git diff"})),
            tool_call("status", "git_status", json!({})),
            tool_call("diff", "git_diff", json!({})),
        ]),
        text("Done."),
    ]);
    let mut live = Live::start(&home, &url);
    live.send(json!({"type": "user", "text": "Inspect changes"}));
    let event = live.wait_for(|e| e["type"] == "gate_request" || e["type"] == "turn_done");
    assert_eq!(event["type"], "turn_done", "ordinary reads should stay automatic");
    live.finish();
    let requests = server.join().unwrap();
    assert!(!home.0.join("ran.txt").exists(), "Git ran a configured executable");
    for (id, expected) in [
        ("shell", "+new fixture"),
        ("status", "note.txt"),
        ("diff", "+new fixture"),
    ] {
        let result = requests[1]["messages"].as_array().unwrap().iter()
            .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
            .expect("tool result sent to the gateway");
        assert!(result["content"].as_str().unwrap().contains(expected), "{result}");
    }
}

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
