# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-22

### Added

- Initial release of mayi

### Added

- `mayi` asks a classifier whether a tool call is safe, then allows it or opens a confirmation dialog.
- Providers: Jev (default), OpenAI, and Anthropic. Chat providers must return `Verdict`'s JSON Schema.
- `mayi stats` summarizes local decisions. `--since 7d` (also `Nh` / `Nm`) and `--agent` filter the log.
- Each hook call appends one JSONL line to `$XDG_STATE_HOME/mayi/decisions.jsonl` (tool name and verdict, not the action). Logging is fail-open.

### Changed

- The project is named mayi. Config lives at `~/.config/mayi/config.toml`.
- A Jev noul of at least 0.85 allows the call. Below that, every agent gets the same dialog: Approve and Save allow, Deny denies. Errors deny.
