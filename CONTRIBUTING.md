# Contributing to rhood-rs

Thanks for your interest in contributing. This document describes how to build,
test, and submit changes.

## Workspace layout

`rhood-rs` is a Cargo workspace with three crates:

- [`rhood-core`](crates/rhood-core) — async client library for the Robinhood API
- [`rhood-cli`](crates/rhood-cli) — terminal CLI built on `rhood-core`
- [`rhood-mcp`](crates/rhood-mcp) — Model Context Protocol server exposing the API to LLM clients

The minimum supported Rust version is **1.96** (edition 2024). Install Rust via
[rustup](https://rustup.rs/).

## Local checks

Run the same gates as CI before opening a pull request. These mirror
[`.github/workflows/pull-request-validation.yml`](.github/workflows/pull-request-validation.yml):

```sh
# Format
cargo fmt --all --check

# Lint (warnings are denied)
cargo clippy --all-targets --all-features -- -D warnings

# Build
cargo check --locked

# Test (uses cargo-nextest: https://nexte.st)
cargo nextest run --all-targets --all-features
```

If you don't have nextest installed: `cargo install cargo-nextest`. You can also
run tests with the built-in runner via `cargo test --all-targets --all-features`.

## Lints

The workspace enables a strict Clippy lint set in
[`Cargo.toml`](Cargo.toml) under `[workspace.lints.clippy]` (no panics/unwraps,
no swallowed errors, careful numeric casts, and more).

Do not silence lints with bare `#[allow(...)]`. When a suppression is genuinely
warranted, use `#[expect(lint_name, reason = "…")]` with a clear reason — this is
enforced by the `allow_attributes` and `allow_attributes_without_reason` lints.

## Pull requests

- Keep PRs focused and reasonably small.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit
  messages and PR titles (e.g. `feat:`, `fix:`, `docs:`, `refactor:`). Releases
  are automated with [release-plz](https://release-plz.dev/) and rely on this.
- Ensure `fmt`, `clippy`, and the test suite all pass locally.
- Never commit real credentials, tokens, MFA secrets, or account identifiers —
  in code, fixtures, or examples. Use synthetic values (the existing tests use
  random UUIDs and the canonical TOTP test vector).

## Reporting security issues

Please report vulnerabilities privately — see [SECURITY.md](SECURITY.md). Do not
open a public issue for security problems.
