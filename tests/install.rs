//! `install` against a temporary home directory.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use predicates::str::contains;
use serde_json::{Value, json};
use tempfile::TempDir;

fn install(agent: &str, home: &Path) -> Assert {
    Command::cargo_bin("mayi")
        .unwrap()
        .args(["--agent", agent, "install"])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .assert()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn claude_gets_a_pre_tool_use_group() {
    let home = TempDir::new().unwrap();
    install("claude", home.path())
        .success()
        .stderr(contains("installed claude hook"));
    let config = read_json(&home.path().join(".claude/settings.json"));
    let handler = &config["hooks"]["PreToolUse"][0]["hooks"][0];
    assert_eq!(config["hooks"]["PreToolUse"][0]["matcher"], "*");
    assert_eq!(handler["args"], json!(["--agent", "claude"]));
    assert!(handler["command"].as_str().unwrap().contains("mayi"));
}

#[test]
fn cursor_gets_version_and_three_events() {
    let home = TempDir::new().unwrap();
    install("cursor", home.path()).success();
    let config = read_json(&home.path().join(".cursor/hooks.json"));
    assert_eq!(config["version"], 1);
    for event in ["beforeShellExecution", "beforeMCPExecution", "preToolUse"] {
        let handler = &config["hooks"][event][0];
        assert_eq!(handler["failClosed"], true);
        assert!(
            handler["command"]
                .as_str()
                .unwrap()
                .ends_with("--agent cursor")
        );
    }
}

#[test]
fn codex_gets_a_shell_command_and_a_trust_reminder() {
    let home = TempDir::new().unwrap();
    install("codex", home.path())
        .success()
        .stderr(contains("/hooks"));
    let config = read_json(&home.path().join(".codex/hooks.json"));
    let command = config["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.ends_with("--agent codex"));
}

#[test]
fn second_install_is_a_no_op() {
    let home = TempDir::new().unwrap();
    let path = home.path().join(".claude/settings.json");
    install("claude", home.path()).success();
    let first = fs::read_to_string(&path).unwrap();
    install("claude", home.path())
        .success()
        .stderr(contains("already installed"));
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
}

#[test]
fn existing_settings_are_kept() {
    let home = TempDir::new().unwrap();
    let path = home.path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, r#"{ "permissions": { "allow": ["Bash"] } }"#).unwrap();
    install("claude", home.path()).success();
    let config = read_json(&path);
    assert_eq!(config["permissions"]["allow"], json!(["Bash"]));
    assert_eq!(config["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
}

#[test]
fn corrupt_config_is_left_alone() {
    let home = TempDir::new().unwrap();
    let path = home.path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not json").unwrap();
    install("claude", home.path())
        .code(1)
        .stderr(contains("invalid JSON"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
}
