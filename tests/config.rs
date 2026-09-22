//! `mayi config` against a temporary XDG config dir.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use predicates::str::contains;
use serde_json::Value;
use tempfile::TempDir;

/// The binary with `home` as config dir.
fn cmd(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("mayi").unwrap();
    cmd.env("XDG_CONFIG_HOME", home.path());
    cmd
}

/// `mayi config <args>`.
fn config<'a>(home: &TempDir, args: impl IntoIterator<Item = &'a str>) -> Assert {
    cmd(home).arg("config").args(args).assert()
}

/// `mayi config show <args>`, parsed.
fn show<'a>(home: &TempDir, args: impl IntoIterator<Item = &'a str>) -> Value {
    let stdout = config(home, ["show"].into_iter().chain(args))
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&stdout).unwrap()
}

fn toml_path(home: &TempDir) -> PathBuf {
    home.path().join("mayi/config.toml")
}

#[test]
fn path_is_under_xdg_config_home() {
    let home = TempDir::new().unwrap();
    config(&home, ["path"])
        .success()
        .stdout(contains("mayi/config.toml"));
}

#[test]
fn config_does_not_need_agent() {
    let home = TempDir::new().unwrap();
    config(&home, ["path"]).success();
}

#[test]
fn get_model_default() {
    let home = TempDir::new().unwrap();
    config(&home, ["get", "model"])
        .success()
        .stdout("jev-latest\n");
}

#[test]
fn get_provider_default() {
    let home = TempDir::new().unwrap();
    config(&home, ["get", "provider"]).success().stdout("jev\n");
}

#[test]
fn openai_follows_provider_defaults() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "provider", "openai"]).success();
    config(&home, ["get", "api_url"])
        .success()
        .stdout("https://api.openai.com\n");
    config(&home, ["get", "model"])
        .success()
        .stdout("gpt-5.6-luna\n");
}

#[test]
fn invalid_provider_does_not_write() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "provider", "gemini"])
        .code(1)
        .stderr(contains("jev"));
    assert!(!toml_path(&home).exists());
}

#[test]
fn set_writes_only_that_key() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "api_key", "tsk_test"])
        .success()
        .stderr(contains("set api_key"));
    let text = fs::read_to_string(toml_path(&home)).unwrap();
    assert!(text.contains("tsk_test"), "{text}");
    assert!(!text.contains("model"), "{text}");
    assert!(!text.contains("instructions"), "{text}");
}

#[test]
fn set_then_get() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "model", "jev-preview"]).success();
    config(&home, ["get", "model"])
        .success()
        .stdout("jev-preview\n");
}

#[test]
fn set_keeps_other_keys() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "api_key", "tsk_test"]).success();
    config(&home, ["set", "model", "jev-preview"]).success();
    let text = fs::read_to_string(toml_path(&home)).unwrap();
    assert!(text.contains("tsk_test"), "{text}");
    assert!(text.contains("jev-preview"), "{text}");
}

#[test]
fn unset_restores_default() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "model", "jev-preview"]).success();
    config(&home, ["unset", "model"])
        .success()
        .stderr(contains("unset model"));
    config(&home, ["get", "model"])
        .success()
        .stdout("jev-latest\n");
    let text = fs::read_to_string(toml_path(&home)).unwrap();
    assert!(!text.contains("model"), "{text}");
}

#[test]
fn show_redacts_api_key() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "api_key", "tsk_secret"]).success();
    let v = show(&home, []);
    assert_eq!(v["api_key"], "****");
    assert_eq!(v["provider"], "jev");
    assert_eq!(v["model"], "jev-latest");
    assert!(!v.to_string().contains("tsk_secret"), "{v}");
    assert!(
        v["instructions"].as_str().unwrap().contains("ordinary"),
        "{v}"
    );
}

#[test]
fn show_secrets_includes_api_key() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "api_key", "tsk_secret"]).success();
    assert_eq!(show(&home, ["--show-secrets"])["api_key"], "tsk_secret");
}

#[test]
fn unknown_key_does_not_write() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "min_confidnce", "0.5"])
        .code(2)
        .stderr(contains("invalid value"));
    assert!(!toml_path(&home).exists());
}

#[test]
fn invalid_timeout_does_not_write() {
    let home = TempDir::new().unwrap();
    config(&home, ["set", "timeout_ms", "fast"])
        .code(1)
        .stderr(contains("integer"));
    assert!(!toml_path(&home).exists());
}

#[test]
fn corrupt_file_is_left_alone() {
    let home = TempDir::new().unwrap();
    let path = toml_path(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not toml").unwrap();
    config(&home, ["set", "model", "jev-preview"])
        .code(1)
        .stderr(contains("invalid config"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "not toml");
}

#[test]
fn leftover_criteria_is_invalid() {
    let home = TempDir::new().unwrap();
    let path = toml_path(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[criteria]\nsafe = \"x\"\n").unwrap();
    config(&home, ["show"])
        .code(1)
        .stderr(contains("invalid config"));
}

#[test]
fn leftover_allow_min_confidence_is_invalid() {
    let home = TempDir::new().unwrap();
    let path = toml_path(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "allow_min_confidence = 0.6\n").unwrap();
    config(&home, ["show"])
        .code(1)
        .stderr(contains("invalid config"));
}

#[test]
fn leftover_min_confidence_is_invalid() {
    let home = TempDir::new().unwrap();
    let path = toml_path(&home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "min_confidence = 0.5\n").unwrap();
    config(&home, ["show"])
        .code(1)
        .stderr(contains("invalid config"));
}

#[test]
fn get_unknown_key_fails() {
    let home = TempDir::new().unwrap();
    config(&home, ["get", "nope"])
        .code(2)
        .stderr(contains("invalid value"));
}

#[cfg(unix)]
#[test]
fn file_is_mode_600() {
    use std::os::unix::fs::PermissionsExt;
    let home = TempDir::new().unwrap();
    config(&home, ["set", "api_key", "tsk_test"]).success();
    let mode = fs::metadata(toml_path(&home)).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn empty_api_key_get_is_blank() {
    let home = TempDir::new().unwrap();
    config(&home, ["get", "api_key"]).success().stdout("\n");
}
