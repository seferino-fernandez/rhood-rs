# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add `get_accounts` and optional canonical account selectors to account-sensitive tools.
- Add the read-only, no-parameter `get_futures_positions` tool with full raw-object preservation.
- Add the read-only, account-filtered `get_aggregate_option_positions` tool with bounded paging.
- Add read-only `get_portfolio_performance` for the raw Bonfire returns chart model.
- Add read-only IPO Access list and card tools with tolerant raw output.

### Changed

- Stock and option elicitation now seal the exact core Staged Order, including the selected account, resolved instrument or contract, broker payload, client reference, typed summary, and expiry.
- One Confirmation module owns Principal binding, effective TTL, MRTR response handling, and atomic terminal redemption. Accept, decline, and cancel consume state; malformed responses remain correctable until expiry.
- Multi-account requests fail before staging unless `account_number` is supplied. Stock state expires after 60 seconds, option state after 600 seconds, and submission timeouts return `submission_uncertain`.
- Position, dividend, stock-order, news, tag, and split outputs now serialize core-owned Instrument Identity Enrichment. The MCP adapters do not resolve Instrument References, and aligned research symbol arrays use `null` for misses.
- The order-expiry error code changed from `prepared_order_expired` to `staged_order_expired`; this is a breaking change for clients matching on the code.

### Fixed

- `get_futures_quotes` no longer attaches `requested_symbol` by response position. Each quote now carries the alias core correlated it to by instrument ID, so a reordered or partial upstream batch can no longer label a quote with another contract's symbol and shift every alias after it. A requested symbol the batch does not answer is simply absent from the results.
- Tool errors no longer collapse unmapped core failures into the generic code `error`. Order validation now reports `invalid_order`, and challenge, token-expiry, device-verification, network, and timeout failures each report their own code. Serialization and filesystem failures report `internal_error` and no longer echo their source.

## [0.2.0](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.1.4...rhood-mcp-v0.2.0) - 2026-08-11

### Added

- *(mcp)* paginate tool discovery
- *(mcp)* paginate large collection tools
- *(options)* support explicit position effects
- *(mcp)* quote held options by instrument URL
- *(mcp)* rate limit tool invocations
- *(options)* Expose equity contract search
- *(mcp)* Confirm trades with MRTR elicitation instead of a staged-order map
- *(mcp)* Serve MCP 2026-07-28 only, with per-request metadata enforced
- *(mcp)* Enforce the OAuth write scope on write tools
- *(config)* Refuse secret-bearing config files with loose permissions
- Enable the workspace lint policy

### Fixed

- formatting
- *(core)* sanitize unusable instrument diagnostics
- *(mcp)* Harden tool rate limiter and config overrides
- *(mcp)* Bind consent to trusted client
- *(mcp)* Validate OAuth redirects first
- *(mcp)* preregister OAuth clients
- *(mcp)* Add server info to tool results
- *(mcp)* Make an accepted Confirmation single-use
- Extract API error diagnostics once, render them per surface

### Other

- remove internal planning records
- clarify lint suppression assumptions
- *(core)* make API diagnostics extensible
- *(mcp)* document strict protocol support
- *(mcp)* cover security boundary entry points
- *(mcp)* apply rustfmt to rate limiter hardening
- *(mcp)* Clarify stock price tool selection
- *(mcp)* Guide opaque ID discovery
- Describe the elicitation flow in both READMEs

## [0.1.4](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.1.3...rhood-mcp-v0.1.4) - 2026-08-09

### Other

- *(deps)* Update rmcp to v3.1.2

## [0.1.3](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.1.2...rhood-mcp-v0.1.3) - 2026-07-29

### Added

- Upgrade to rmcp v3

### Other

- *(deps)* Bump the dependencies group with 2 updates ([#7](https://github.com/seferino-fernandez/rhood-rs/pull/7))
- updated install instructions for homebrew and cargo binstall

## [0.1.2](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.1.1...rhood-mcp-v0.1.2) - 2026-06-10

### Added

- Homebrew, cargo install, and cargo binstall distribution ([#3](https://github.com/seferino-fernandez/rhood-rs/pull/3))

## [0.1.1](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.1.0...rhood-mcp-v0.1.1) - 2026-06-10

### Other

- *(deps)* Bump the dependencies group with 8 updates
