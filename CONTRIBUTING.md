# Contributing

## Licensing

By contributing to this repository, you agree that your contributions are licensed under Apache License 2.0.

## Pull Requests

1. Keep changes scoped and explain behavior changes clearly in the PR description.
2. Add or update tests for code changes unless the change is docs-only.
3. Keep commits reviewable and prefer one logical change per commit.

## Development Setup

1. Install Rust stable toolchain.
2. Configure the bridge using `config.yaml` (or set `CONFIG_PATH`).
3. Start a PostgreSQL instance for local development if you need persistence-backed flows.

## Local Validation

Run these before opening a PR:

```bash
cargo check -p matrix-bridge-slack
cargo test -p matrix-bridge-slack
```

If you modify formatting-sensitive code, also run:

```bash
cargo fmt --all
```
