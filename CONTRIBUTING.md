# Contributing to rhood-rs

Thanks for your interest in contributing. This document describes how to build,
test, and submit changes.

## Workspace layout

`rhood-rs` is a Cargo workspace with three crates:

- [`rhood-core`](crates/rhood-core) - async client library for the Robinhood API
- [`rhood-cli`](crates/rhood-cli) - terminal CLI built on `rhood-core`
- [`rhood-mcp`](crates/rhood-mcp) - Model Context Protocol server exposing the API to LLM clients

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
warranted, use `#[expect(lint_name, reason = "…")]` with a clear reason. This is
enforced by the `allow_attributes` and `allow_attributes_without_reason` lints.

A suppression's reason must stay true. Tooling cannot check this: the lint still
fires, so the expectation is still fulfilled and the gate stays green while the
stated justification has quietly become false. When you change the code around a
suppression, re-read its reason.

## Tests must constrain the code, not agree with it

A test that is written from the same information as the code will agree with the
code instead of constraining it, and it reports success either way. This has been
caught five times in this repository, so it is worth stating plainly.

Two rules:

- **Never assert against a value the production code also computes.** Do not read
  the constant a test exists to guard, and do not rebuild the production
  expression inside the test and assert on your copy. Write the expected value as
  a literal. A test that reads `STAGED_STOCK_ORDER_TTL_MS` cannot notice that
  constant changing; a test that reconstructs a table row cannot notice the
  renderer reordering its columns.
- **Watch every new assertion fail once before you trust it.** Break the thing it
  guards, see it go red, put it back. This costs a minute and is the only check
  that distinguishes a passing test from a test that cannot fail.

Choose fixture values that cannot be satisfied by accident. A fixture using
`quantity: "2"` once passed a `contains("2")` assertion via an unrelated account
label in the same string, and would have survived the field being deleted. Long,
mutually non-overlapping values remove that whole class.

## Pull requests

- Keep PRs focused and reasonably small.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit
  messages and PR titles (e.g. `feat:`, `fix:`, `docs:`, `refactor:`). Releases
  are automated with [release-plz](https://release-plz.dev/) and rely on this.
- Do not edit `CHANGELOG.md` or any `version` field by hand. release-plz owns
  both and derives them from commit messages; a hand-written entry is published
  twice, once as written and once as generated from the same commit.
- A change that removes or alters a public item needs a `BREAKING CHANGE:`
  footer on its commit. That footer is the only thing that moves the version,
  so without it a removed API ships as a patch release.
- Write commit messages for someone who has only this repository. Internal
  issue or decision-record numbers do not resolve for anyone else, and
  release-plz copies commit subjects verbatim into the public changelog.
- Ensure `fmt`, `clippy`, and the test suite all pass locally.
- Never commit real credentials, tokens, MFA secrets, or account identifiers
  in code, fixtures, or examples. Use synthetic values (the existing tests use
  random UUIDs and the canonical TOTP test vector).

## Reporting security issues

Please report vulnerabilities privately - see [SECURITY.md](SECURITY.md). Do not
open a public issue for security problems.
