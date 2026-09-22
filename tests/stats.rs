//! `mayi stats` over a temporary state dir.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{Value, json};
use tempfile::TempDir;

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn state(events: &[Value]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mayi/decisions.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut body = String::new();
    for event in events {
        body.push_str(&event.to_string());
        body.push('\n');
    }
    fs::write(path, body).unwrap();
    dir
}

fn stats(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("mayi").unwrap();
    cmd.arg("stats").env("XDG_STATE_HOME", home.path());
    cmd
}

#[test]
fn empty_log_is_quiet() {
    let home = TempDir::new().unwrap();
    stats(&home)
        .assert()
        .success()
        .stdout(contains("no decisions yet"));
}

#[test]
fn stats_does_not_need_agent() {
    let home = TempDir::new().unwrap();
    stats(&home).assert().success();
}

#[test]
fn aggregates_verdicts_and_tokens() {
    let now = unix_secs();
    let home = state(&[
        json!({"ts": now, "agent": "claude", "tool": "Bash", "verdict": "allow", "input_tokens": 10, "output_tokens": 0, "latency_ms": 100}),
        json!({"ts": now, "agent": "claude", "tool": "Edit", "verdict": "ask", "input_tokens": 5, "latency_ms": 200}),
        json!({"ts": now, "agent": "cursor", "tool": "Bash", "verdict": "deny", "error": true, "latency_ms": 50}),
        json!({"ts": now, "agent": "codex", "verdict": "weird"}),
        json!("not an object"),
    ]);
    stats(&home)
        .assert()
        .success()
        .stdout(contains("4 checks"))
        .stdout(contains("allow"))
        .stdout(contains("ask"))
        .stdout(contains("deny"))
        .stdout(contains("1 errors"))
        .stdout(contains("15 in / 0 out"))
        .stdout(contains("p50 100ms"))
        .stdout(contains("p95 200ms"));
}

#[test]
fn human_summary_aligns_counts() {
    let now = unix_secs();
    let home = state(&[json!({
        "ts": now,
        "agent": "claude",
        "tool": "Bash",
        "verdict": "allow",
        "input_tokens": 3,
        "latency_ms": 12
    })]);
    stats(&home)
        .assert()
        .success()
        .stdout(contains("1 checks"))
        .stdout(contains("allow"))
        .stdout(contains("tokens"))
        .stdout(contains("latency"));
}

#[test]
fn agent_flag_filters() {
    let now = unix_secs();
    let home = state(&[
        json!({"ts": now, "agent": "claude", "verdict": "allow"}),
        json!({"ts": now, "agent": "cursor", "verdict": "deny"}),
    ]);
    for args in [
        ["--agent", "claude", "stats"].as_slice(),
        ["stats", "--agent", "claude"].as_slice(),
    ] {
        Command::cargo_bin("mayi")
            .unwrap()
            .args(args)
            .env("XDG_STATE_HOME", home.path())
            .assert()
            .success()
            .stdout(contains("1 checks"))
            .stdout(contains("allow"));
    }
}

#[test]
fn since_drops_old_events() {
    let now = unix_secs();
    let home = state(&[
        json!({"ts": now - 10 * 86_400, "agent": "claude", "verdict": "deny"}),
        json!({"ts": now, "agent": "claude", "verdict": "allow"}),
    ]);
    stats(&home)
        .args(["--since", "7d"])
        .assert()
        .success()
        .stdout(contains("1 checks"))
        .stdout(contains("allow"));
}

#[test]
fn invalid_since_is_an_error() {
    let home = TempDir::new().unwrap();
    stats(&home)
        .args(["--since", "7s"])
        .assert()
        .failure()
        .stderr(contains("--since"));
}
