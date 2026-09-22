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

/// Longest action line (in chars) sent whole to the classifier and the dialog.
const MAX_CHARS: usize = 500;
/// Chars kept at each end of a clipped line, and the longest file body or
/// program list kept verbatim.
// A clip is head 160 + ellipsis + tail 160 + a program list of at most 160, so it stays under 500.
const EDGE_CHARS: usize = 160;

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
    /// Action line sent to the classifier and shown in the dialog: `{tool} {args}`,
    /// clipped when longer than `MAX_CHARS`.
    pub line: String,
    /// True when `line` dropped part of the call. A trimmed call is never auto-allowed.
    pub trimmed: bool,
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

        let mut trimmed = false;
        if let Value::Object(map) = &mut args {
            for key in ["description", "timeout", "run_in_background"] {
                map.remove(key);
            }
            for key in ["content", "old_string", "new_string"] {
                if let Some(Value::String(s)) = map.get_mut(key)
                    && s.chars().count() > EDGE_CHARS
                {
                    *s = format!("<{} bytes>", s.len());
                    trimmed = true;
                }
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

        let (prefix, mut detail) = match (tool.as_deref(), detail.is_empty()) {
            (None, true) => (String::new(), raw.to_string()),
            (None, false) => (String::new(), detail),
            (Some(t), true) => (t.to_string(), detail),
            (Some(t), false) => (format!("{t} "), detail),
        };
        if prefix.chars().count() + detail.chars().count() > MAX_CHARS {
            let shell = tool.as_deref() == Some("Bash") && !detail.starts_with('{');
            detail = Self::clip(&detail, shell);
            trimmed = true;
        }
        Call {
            tool,
            line: prefix + &detail,
            trimmed,
        }
    }

    /// Head and tail of an overlong line, led by the programs a shell command runs.
    ///
    /// The program list splits on `&&`, `||`, `;`, `|` and newlines without
    /// regard for quoting, and is omitted when it would itself be long.
    fn clip(text: &str, shell: bool) -> String {
        let head_end = text
            .char_indices()
            .nth(EDGE_CHARS)
            .map_or(text.len(), |(i, _)| i);
        let tail_start = text
            .char_indices()
            .nth_back(EDGE_CHARS - 1)
            .map_or(0, |(i, _)| i);

        let mut out = String::new();
        if shell {
            let programs = text
                .replace("&&", "\n")
                .replace("||", "\n")
                .replace(';', "\n")
                .split(['|', '\n'])
                .filter_map(|piece| piece.split_whitespace().next())
                .collect::<Vec<_>>()
                .join(" | ");
            if programs.chars().count() <= EDGE_CHARS {
                out = programs + "\n";
            }
        }
        out + &text[..head_end] + "\n…\n" + &text[tail_start..]
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
    use serde_json::json;

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

    #[test]
    fn short_lines_stay_whole() {
        let call = parse(Agent::Cursor, "cursor/shell");
        assert_eq!(call.line, "Bash curl https://example.com");
        assert!(!call.trimmed);
    }

    #[test]
    fn long_file_body_becomes_a_byte_count() {
        let body = "é".repeat(200);
        let input = json!({
            "tool_name": "Write",
            "tool_input": { "file_path": "/tmp/project/big.txt", "content": body },
        });
        let call = Agent::Claude.parse(&input.to_string());
        assert!(call.line.contains("<400 bytes>"), "{}", call.line);
        assert!(!call.line.contains(&body));
        assert!(call.line.contains("/tmp/project/big.txt"));
        assert!(call.trimmed);
    }

    #[test]
    fn long_shell_keeps_both_ends() {
        let command = format!("echo {} && rm -rf /", "a".repeat(600));
        let input = json!({ "tool_name": "Bash", "tool_input": { "command": command } });
        let call = Agent::Claude.parse(&input.to_string());
        assert!(
            call.line.starts_with("Bash echo | rm\necho aaaa"),
            "{}",
            call.line
        );
        assert!(call.line.contains('…'));
        assert!(call.line.ends_with("rm -rf /"));
        assert!(call.trimmed);
    }
}
