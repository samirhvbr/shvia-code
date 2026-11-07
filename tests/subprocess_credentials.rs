#![cfg(unix)]

mod common;

use common::{gateway, text, tool_call, tools, Home, Live};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const EXTRA_ENV: &[(&str, &str)] = &[
    ("AI_MEMORY_TOKEN", "synthetic-memory-secret"),
    ("OPENAI_API_KEY", "synthetic-openai-secret"),
    ("ANTHROPIC_AUTH_TOKEN", "synthetic-anthropic-secret"),
    ("AWS_SESSION_TOKEN", "synthetic-aws-secret"),
    ("GITHUB_TOKEN", "synthetic-github-secret"),
    ("DATABASE_URL", "synthetic-database-secret"),
    ("CUSTOM_CREDENTIAL", "synthetic-unlisted-secret"),
    (
        "http_proxy",
        "http://synthetic-proxy-user:synthetic-proxy-secret@proxy.invalid:8080",
    ),
    ("NO_PROXY", "127.0.0.1"),
];

fn tool_result(home: &Home, name: &str, args: serde_json::Value) -> String {
    let (url, server) = gateway(vec![
        tools(vec![tool_call("call", name, args)]),
        text("Done."),
    ]);
    let mut anna = Live::start_env(home, &url, EXTRA_ENV);
    anna.send(json!({"type": "user", "text": "Run the check"}));
    anna.wait_for(|event| event["type"] == "gate_request");
    anna.send(json!({"id": "call", "decision": "approve"}));
    anna.wait_for(|event| event["type"] == "turn_done");
    anna.finish();
    let requests = server.join().unwrap();
    for request in &requests {
        assert_eq!(request["__api_key"], "synthetic-test-key");
    }
    let result = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "call")
        .expect("tool result sent to the gateway");
    result["content"].as_str().unwrap().to_owned()
}

fn assert_clean_environment(home: &Home, output: &str) {
    assert!(
        output.contains(&format!("HOME={}", home.0.display())),
        "{output}"
    );
    assert!(output.contains("TERM=dumb"), "{output}");
    assert!(
        output.contains("http_proxy=http://proxy.invalid:8080"),
        "{output}"
    );
    assert!(output.contains("NO_PROXY=127.0.0.1"), "{output}");
    assert!(
        !output.contains("SHVIA_API_KEY="),
        "gateway key reached the subprocess"
    );
    assert!(
        !output.contains("synthetic-proxy-user"),
        "proxy username reached the subprocess"
    );
    assert!(
        !output.contains("synthetic-proxy-secret"),
        "proxy password reached the subprocess"
    );
    for (name, value) in EXTRA_ENV
        .iter()
        .filter(|(name, _)| !["http_proxy", "NO_PROXY"].contains(name))
    {
        assert!(
            !output.contains(&format!("{name}=")) && !output.contains(value),
            "{name} reached the subprocess"
        );
    }
}

#[test]
fn approved_shell_and_its_children_keep_credentials_out_of_tool_output() {
    let home = Home::new();
    let output = tool_result(
        &home,
        "bash",
        json!({"command": "sh -c env", "why": "check inherited environment"}),
    );
    assert_clean_environment(&home, &output);
}

#[test]
fn approved_git_commit_runs_its_hook_without_parent_credentials() {
    let home = Home::new();
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .env("HOME", &home.0)
            .current_dir(&home.0)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    git(&["init", "-q", "--template="]);
    git(&["config", "user.name", "Anna Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["config", "core.hooksPath", ".git/hooks"]);
    std::fs::create_dir(home.0.join(".git/hooks")).unwrap();
    let hook = home.0.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nprintf 'hook ran\\n'\nenv\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(home.0.join("note.txt"), "fixture\n").unwrap();
    git(&["add", "note.txt"]);

    let output = tool_result(
        &home,
        "git_commit",
        json!({"message": "Test hook environment", "add_all": false}),
    );
    assert!(
        output.contains("hook ran") && output.contains("Test hook environment"),
        "{output}"
    );
    assert_clean_environment(&home, &output);
}
