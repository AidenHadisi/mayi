# MayI

<p align="center">
  <a href="https://github.com/AidenHadisi/mayi/actions/workflows/ci.yml"><img src="https://github.com/AidenHadisi/mayi/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.96%2B-orange.svg?logo=rust&logoColor=white" alt="Rust 1.96+"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
</p>

<p align="center"><strong>A tool-call gate for coding agents, powered by <a href="https://typesafe.ai">TypeSafe Jev</a>.</strong></p>

mayi hooks into Claude Code, Cursor, and Codex. Every shell command, file edit, and MCP call is scored by TypeSafe Jev before it runs. Routine work passes silently; anything else asks you first.

Jev answers yes/no questions with a calibrated score instead of prose, so the check is fast and cheap enough to run on every call. OpenAI and Anthropic work too.

## Install

Requires Rust 1.96 or newer.

```bash
cargo install --git https://github.com/AidenHadisi/mayi --locked
```

Prebuilt installers for macOS, Linux, and Windows are published on the [releases page](https://github.com/AidenHadisi/mayi/releases).

## Quick start

```bash
mayi config set api_key tsk_...
mayi --agent claude install   # or cursor, codex
```

`install` registers the current binary in the agent's local hooks file:

| Agent | File |
| --- | --- |
| Claude Code | `~/.claude/settings.json` |
| Cursor | `~/.cursor/hooks.json` |
| Codex | `~/.codex/hooks.json` |

It replaces only the hook entries it owns and leaves other hooks in place. Cloud and remote sessions are not configured. In Codex, approve the entry with `/hooks` or it will not run.

## How decisions work

The classifier answers safe or not safe. With Jev, a score of 0.85 or higher is safe. With OpenAI or Anthropic, the model returns `safe: true` or `safe: false` in a fixed JSON schema.

- **Safe** calls are allowed. No dialog.
- **Not safe** calls open a dialog. **Approve** and **Approve and save** allow the call; **Deny**, closing the window, or a dialog error denies it.
- **Errors** (network, configuration, internal) deny the call.

The process always exits 0, because some hosts treat a crashed hook as an allow.

## Configuration

Settings live in a TOML file:

```text
~/.config/mayi/config.toml
```

`$XDG_CONFIG_HOME` overrides the base directory. Any key left out keeps its default.

| Key | Default | Description |
| --- | --- | --- |
| `provider` | `jev` | Classifier backend: `jev`, `openai`, or `anthropic`. |
| `api_key` | none | API key for the provider. Required. |
| `model` | per provider | `jev-latest`, `gpt-5.6-luna`, or `claude-haiku-4-5`. Override to pick another model. |
| `api_url` | per provider | Base URL for the provider API. Set for a proxy or a mock server. |
| `timeout_ms` | `2500` | Request timeout. A timeout denies the call. |
| `instructions` | see below | The question sent with every tool call. |

The default `instructions` is: *"Is this coding-agent tool call ordinary development work? Prefer yes unless it is clearly harmful."* Tightening or loosening this sentence is the main way to tune how often the dialog appears.

Manage the file from the command line:

```bash
mayi config show                 # print settings; api_key is redacted
mayi config set provider openai
mayi config set model gpt-5.6-luna
mayi config path                 # print the file location
```

Or edit it directly:

```toml
provider = "anthropic"
api_key = "sk-ant-..."
timeout_ms = 4000
instructions = "Is this tool call safe to run unattended in a git repository?"
```

## Activity

```bash
mayi stats
mayi stats --since 7d --agent claude
```

Each decision is appended to `~/.local/state/mayi/decisions.jsonl` (`$XDG_STATE_HOME` when set). The log records the agent, tool name, verdict, token counts, and latency. It does not record the command or its arguments.

## Privacy

Tool names and arguments are sent to the configured provider. Depending on the call, that can include shell commands, file paths, file contents, and MCP arguments. Nothing is sent anywhere else.

Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
