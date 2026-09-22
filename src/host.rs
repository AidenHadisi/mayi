//! What differs between Claude Code, Cursor, and Codex: where the config lives,
//! how stdin is flattened, what a hook entry looks like, and how a verdict is
//! written to stdout.

use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde_json::{Value, json};

use crate::Result;

/// Coding agent whose hooks we speak.
///
/// Each variant knows its config path, how to parse stdin, how to install a hook
/// entry, and how to shape stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Cursor,
    Codex,
}

/// One tool call, flattened from host stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Tool id for the decision log (`Bash`, `mcp__memory__search`). No args.
    pub tool: Option<String>,
    /// Action line sent to the classifier: `{tool} {args}`.
    pub line: String,
}

/// The gate's decision on one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Deny,
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
        })
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        })
    }
}

impl Agent {
    /// The hooks config file under the user's home directory.
    pub fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Self::Claude => home.join(".claude/settings.json"),
            Self::Cursor => home.join(".cursor/hooks.json"),
            Self::Codex => home.join(".codex/hooks.json"),
        }
    }

    /// Write the hook into this agent's config unless it is already present.
    pub fn install_hook(self) -> Result<()> {
        let home = env::home_dir().ok_or("cannot determine home directory")?;
        let path = self.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        if existing.contains("mayi") {
            eprintln!("{self} hook already installed in {}", path.display());
            return Ok(());
        }

        let mut config: Value = if existing.is_empty() {
            json!({})
        } else {
            serde_json::from_str(&existing)
                .map_err(|e| format!("invalid JSON in {}: {e}", path.display()))?
        };

        self.install(&mut config, &env::current_exe()?);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, format!("{config:#}\n"))?;

        eprintln!("installed {self} hook → {}", path.display());
        if self == Self::Codex {
            eprintln!("Trust this hook in Codex with /hooks, or it will not run.");
        }
        Ok(())
    }

    /// Add our hook entry to the parsed config document.
    ///
    /// Overwrites the hook arrays for the events we own. Callers that want to
    /// keep a prior install should check the file first.
    fn install(self, config: &mut Value, exe: &Path) {
        match self {
            Self::Claude => {
                config["hooks"]["PreToolUse"] = json!([{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": exe.to_string_lossy(),
                        "args": ["--agent", "claude"],
                    }],
                }]);
            }
            Self::Cursor => {
                let handler = json!({
                    "command": format!("{} --agent cursor", exe.display()),
                    "timeout": 10,
                    "failClosed": true,
                });
                config["version"] = json!(1);
                for event in ["beforeShellExecution", "beforeMCPExecution", "preToolUse"] {
                    config["hooks"][event] = json!([handler.clone()]);
                }
            }
            Self::Codex => {
                config["hooks"]["PreToolUse"] = json!([{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": format!("{} --agent codex", exe.display()),
                        "timeout": 10,
                        "statusMessage": "mayi",
                    }],
                }]);
            }
        }
    }

    /// Flatten host stdin to a tool id and an action line.
    ///
    /// Non-JSON passes through as `line` with no tool.
    pub fn parse(self, raw: &str) -> Call {
        let input: Value = serde_json::from_str(raw).unwrap_or_default();

        let event = input.get("hook_event_name").and_then(Value::as_str);
        let (tool, mut args) = if self == Self::Cursor && event == Some("beforeShellExecution") {
            (
                Some("Bash".to_string()),
                input.get("command").cloned().unwrap_or_default(),
            )
        } else {
            let name = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
            let tool = match input.get("mcp_server_name").and_then(Value::as_str) {
                _ if name.is_empty() => None,
                Some(server) if !name.starts_with("mcp__") => {
                    Some(format!("mcp__{server}__{name}"))
                }
                _ => Some(name.to_string()),
            };
            let args = match input.get("tool_input") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
                Some(other) => other.clone(),
                None => Value::Null,
            };
            (tool, args)
        };

        if let Value::Object(map) = &mut args {
            for key in ["description", "timeout", "run_in_background"] {
                map.remove(key);
            }
        }

        let detail = match &args {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Object(map) if map.is_empty() => String::new(),
            Value::Object(map) if map.len() == 1 => match map.values().next() {
                Some(Value::String(s)) => s.clone(),
                _ => serde_json::to_string(&args).unwrap_or_default(),
            },
            other => serde_json::to_string(other).unwrap_or_default(),
        };

        let line = match (tool.as_deref(), detail.as_str()) {
            (None, "") => raw.to_string(),
            (None, d) => d.to_string(),
            (Some(t), "") => t.to_string(),
            (Some(t), d) => format!("{t} {d}"),
        };
        Call { tool, line }
    }

    /// Shape stdout for a permission.
    pub fn respond(self, input: &Value, permission: Permission, reason: &str) -> Value {
        match self {
            Self::Claude | Self::Codex => {
                let event = input["hook_event_name"].as_str().unwrap_or("PreToolUse");
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": event,
                        "permissionDecision": permission.to_string(),
                        "permissionDecisionReason": reason,
                    }
                })
            }
            Self::Cursor if permission == Permission::Allow => json!({ "permission": "allow" }),
            Self::Cursor => json!({
                "permission": permission.to_string(),
                "user_message": reason,
                "agent_message": reason,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Agent, Call};

    fn fixture(rel: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(format!("{rel}.json")),
        )
        .expect("fixture")
    }

    fn parse(agent: Agent, rel: &str) -> Call {
        agent.parse(&fixture(rel))
    }

    #[test]
    fn flattens_host_json() {
        assert_eq!(parse(Agent::Claude, "claude/bash").line, "Bash npm test");
        assert_eq!(
            parse(Agent::Claude, "claude/edit").line,
            r#"Edit {"file_path":"/tmp/project/src/main.rs","old_string":"todo","new_string":"ok"}"#
        );
        assert_eq!(
            parse(Agent::Claude, "claude/mcp").line,
            "mcp__memory__search notes"
        );
        assert_eq!(parse(Agent::Claude, "claude/other").line, "Glob **/*.rs");
        assert_eq!(parse(Agent::Codex, "codex/bash").line, "Bash ls");
        assert_eq!(
            parse(Agent::Codex, "codex/apply_patch").line,
            "apply_patch *** Begin Patch"
        );
        assert_eq!(
            parse(Agent::Cursor, "cursor/shell").line,
            "Bash curl https://example.com"
        );
        assert_eq!(
            parse(Agent::Cursor, "cursor/mcp").line,
            "mcp__memory__search notes"
        );
        assert_eq!(
            parse(Agent::Cursor, "cursor/write").line,
            r##"Write {"file_path":"/tmp/project/README.md","content":"# Project"}"##
        );
        assert_eq!(Agent::Claude.parse("not json").line, "not json");
    }

    #[test]
    fn tool_omits_args() {
        assert_eq!(
            parse(Agent::Claude, "claude/bash").tool.as_deref(),
            Some("Bash")
        );
        assert_eq!(
            parse(Agent::Claude, "claude/edit").tool.as_deref(),
            Some("Edit")
        );
        assert_eq!(
            parse(Agent::Claude, "claude/mcp").tool.as_deref(),
            Some("mcp__memory__search")
        );
        assert_eq!(
            parse(Agent::Cursor, "cursor/shell").tool.as_deref(),
            Some("Bash")
        );
        assert_eq!(
            parse(Agent::Cursor, "cursor/mcp").tool.as_deref(),
            Some("mcp__memory__search")
        );
        assert_eq!(Agent::Claude.parse("not json").tool, None);
    }

    #[test]
    fn cursor_mcp_ignores_server_launch_command() {
        let call = parse(Agent::Cursor, "cursor/mcp");
        assert_eq!(call.line, "mcp__memory__search notes");
        assert!(!call.line.contains("npx"));
    }
}
