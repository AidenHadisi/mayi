//! Hook mode end to end: fixture on stdin, mocked Jev, host-shaped JSON on stdout.

#![allow(clippy::unwrap_used)]

use std::fmt::Write;
use std::fs;
use std::path::Path;

use assert_cmd::Command;
use httpmock::{Method::POST, MockServer};
use predicates::str::contains;
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixture(rel: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("{rel}.json")),
    )
    .unwrap()
}

const GATE_QUESTION: &str = r#"{"questions":{"gate":{"type":"noul"}}}"#;

/// Jev answers `noul`; the request body must include `expects`.
fn jev_expecting(expects: &str, noul: f64) -> MockServer {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/systemone")
            .header("authorization", "Bearer test-key")
            .json_body_includes(expects);
        then.status(200).json_body(json!({
            "answers": { "gate": { "type": "noul", "noul": noul } }
        }));
    });
    server
}

/// Jev answers `noul` to a `gate` noul question.
fn jev_answering(noul: f64) -> MockServer {
    jev_expecting(GATE_QUESTION, noul)
}

/// An `XDG_CONFIG_HOME` holding `mayi/config.toml` with `toml` as its content.
fn config_home(toml: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mayi/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml).unwrap();
    dir
}

/// The hook with `config_home` as both config and state dir.
fn hook(agent: &str, stdin: String, config_home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("mayi").unwrap();
    cmd.args(["--agent", agent])
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("XDG_STATE_HOME", config_home.path())
        .write_stdin(stdin);
    cmd
}

fn decisions(home: &TempDir) -> Vec<Value> {
    fs::read_to_string(home.path().join("mayi/decisions.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn stdout_json(cmd: &mut Command) -> Value {
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).unwrap()
}

fn gate_with(agent: &str, server: &MockServer, stdin: String, config_home: &TempDir) -> Value {
    let path = config_home.path().join("mayi/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut config = fs::read_to_string(&path).unwrap_or_default();
    if !config.is_empty() && !config.ends_with('\n') {
        config.push('\n');
    }
    write!(
        config,
        "api_key = \"test-key\"\napi_url = \"{}\"\n",
        server.base_url()
    )
    .unwrap();
    fs::write(path, config).unwrap();
    stdout_json(&mut hook(agent, stdin, config_home))
}

/// Run the hook with an empty config home so the developer's real file cannot leak in.
fn gate(agent: &str, server: &MockServer, stdin: String) -> Value {
    gate_with(agent, server, stdin, &TempDir::new().unwrap())
}

fn permission_decision(out: &Value) -> &Value {
    &out["hookSpecificOutput"]["permissionDecision"]
}

#[test]
fn claude_allow() {
    let server = jev_answering(1.0);
    let out = gate("claude", &server, fixture("claude/bash"));
    assert_eq!(permission_decision(&out), "allow");
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
}

#[test]
fn jev_sees_the_action_line() {
    let server = jev_expecting(r#"{"state":"Bash npm test"}"#, 1.0);
    let out = gate("claude", &server, fixture("claude/bash"));
    assert_eq!(permission_decision(&out), "allow");
}

#[test]
fn codex_allow() {
    let server = jev_answering(1.0);
    let out = gate("codex", &server, fixture("codex/bash"));
    assert_eq!(permission_decision(&out), "allow");
}

#[test]
fn cursor_allow_is_bare() {
    let server = jev_answering(1.0);
    assert_eq!(
        gate("cursor", &server, fixture("cursor/mcp")),
        json!({ "permission": "allow" })
    );
}

#[test]
fn jev_error_denies() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(500);
    });
    let out = gate("claude", &server, fixture("claude/bash"));
    assert_eq!(permission_decision(&out), "deny");
}

#[test]
fn garbage_stdin_still_answers() {
    let server = jev_answering(1.0);
    let out = gate("codex", &server, "not json".into());
    assert_eq!(permission_decision(&out), "allow");
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
}

#[test]
fn missing_api_key_denies() {
    let home = TempDir::new().unwrap();
    let out = stdout_json(&mut hook("claude", fixture("claude/bash"), &home));
    assert_eq!(permission_decision(&out), "deny");
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("api_key"), "{reason}");
}

#[test]
fn missing_agent_is_a_usage_error() {
    Command::cargo_bin("mayi")
        .unwrap()
        .write_stdin("{}")
        .assert()
        .code(2)
        .stderr(contains("--agent"));
}

#[test]
fn hook_records_a_decision() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/systemone")
            .header("authorization", "Bearer test-key");
        then.status(200).json_body(json!({
            "answers": { "gate": { "type": "noul", "noul": 1.0 } },
            "usage": { "input_tokens": 42, "output_tokens": 0 }
        }));
    });
    let home = TempDir::new().unwrap();
    let out = gate_with("claude", &server, fixture("claude/bash"), &home);
    assert_eq!(permission_decision(&out), "allow");
    let events = decisions(&home);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["agent"], "claude");
    assert_eq!(events[0]["tool"], "Bash");
    assert_eq!(events[0]["verdict"], "allow");
    assert_eq!(events[0]["input_tokens"], 42);
    assert_eq!(events[0]["output_tokens"], 0);
    assert_eq!(events[0]["error"], false);
    assert!(events[0]["latency_ms"].as_u64().is_some());
    assert!(events[0].get("command").is_none());
    let line = events[0].to_string();
    assert!(!line.contains("npm test"), "{line}");
}

#[test]
fn hook_records_classify_errors() {
    let home = TempDir::new().unwrap();
    let out = stdout_json(&mut hook("cursor", fixture("cursor/shell"), &home));
    assert_eq!(out["permission"], "deny");
    let events = decisions(&home);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["verdict"], "deny");
    assert_eq!(events[0]["error"], true);
    assert_eq!(events[0]["tool"], "Bash");
}

// Config file

#[test]
fn leftover_allow_min_confidence_denies() {
    let server = jev_answering(1.0);
    let home = config_home("allow_min_confidence = 0.6\n");
    let out = gate_with("claude", &server, fixture("claude/bash"), &home);
    assert_eq!(permission_decision(&out), "deny");
}

#[test]
fn config_model_is_sent_to_jev() {
    let server = jev_expecting(r#"{"model":"jev-preview"}"#, 1.0);
    let home = config_home(r#"model = "jev-preview""#);
    let out = gate_with("claude", &server, fixture("claude/bash"), &home);
    assert_eq!(permission_decision(&out), "allow");
}

#[test]
fn invalid_config_denies_with_path() {
    let server = jev_answering(1.0);
    let home = config_home("model = \n");
    let out = gate_with("claude", &server, fixture("claude/bash"), &home);
    assert_eq!(permission_decision(&out), "deny");
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("config.toml"), "{reason}");
}

#[test]
fn unknown_config_key_denies() {
    let server = jev_answering(1.0);
    let home = config_home("min_confidnce = 0.5\n");
    let out = gate_with("claude", &server, fixture("claude/bash"), &home);
    assert_eq!(permission_decision(&out), "deny");
}

#[test]
fn config_credentials_are_used() {
    let server = jev_answering(1.0);
    let home = config_home(&format!(
        "api_key = \"test-key\"\napi_url = \"{}\"\n",
        server.base_url()
    ));
    let out = stdout_json(&mut hook("claude", fixture("claude/bash"), &home));
    assert_eq!(permission_decision(&out), "allow");
}

#[test]
fn environment_credentials_are_ignored() {
    let server = jev_answering(1.0);
    let home = config_home(&format!(
        "api_key = \"test-key\"\napi_url = \"{}\"\n",
        server.base_url()
    ));
    let out = stdout_json(
        hook("claude", fixture("claude/bash"), &home)
            .env("TYPESAFE_API_KEY", "wrong-key")
            .env("TYPESAFE_API_URL", "https://wrong.example"),
    );
    assert_eq!(permission_decision(&out), "allow");
}
