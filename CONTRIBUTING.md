# Contributing

Issues and pull requests are welcome. Open an issue before a large change.

## Requirements

Rust 1.96 or newer. `rust-toolchain.toml` pins a newer stable toolchain for local development. The MSRV checked in CI is 1.96.

The full check set uses [cargo-nextest](https://nexte.st), [cargo-deny](https://embarkstudios.github.io/cargo-deny/), and [typos](https://github.com/crate-ci/typos). `prek` runs fmt, clippy, and typos on commit.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets
cargo deny check
```

## Layout

`main` is the CLI. `host` is everything that differs between Claude Code, Cursor, and Codex. `hook` runs one decision. `classifier` calls the model. `dialog` is the confirm window. `config` and `log` are the XDG files.

## Pull requests

Use a conventional commit (`feat:`, `fix:`, `docs:`, `chore:`). Do not commit secrets or an API key.

## Security

Do not file public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).

## License

Contributions are licensed under MIT, the same as the rest of the project.
