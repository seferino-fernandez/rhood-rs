# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add Bonfire unified account discovery and explicit canonical account selection across account-sensitive endpoints.
- Add account-filtered aggregate option strategy positions with full pagination.
- Add the account-specific Bonfire portfolio performance chart view model.
- Add tolerant, authenticated IPO Access list and card view models.
- Add `prepare_stock_order` and `submit_prepared_stock_order` for exact pre-confirmation stock payloads.
- Add `RobinhoodClient::get_futures_positions` with authenticated full-URL pagination and tolerant raw position objects.

### Changed

- Account-sensitive methods now require a canonical account number instead of selecting the first standard account.
- Stock orders now send form version 7, a call-scoped client reference, derived session flags, and strict quote collars. Dollar buys convert to good-for-day fractional shares using the current ask.
- Authenticated GET requests now share paced starts and 429 cooldowns across client clones, retry connection errors, timeouts, and bounded rate limits up to three total attempts by default, and expose strict TOML/environment controls under `HttpConfig`.

### Fixed

- Reject stale prepared orders, redirects, and automatic stock-order retries. Transport timeouts now return `SubmissionUncertain` with a reconciliation reference.

## [0.2.0](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.1.4...rhood-core-v0.2.0) - 2026-08-11

### Added

- *(options)* support explicit position effects
- *(core)* serialize bulk cancellation outcomes
- *(mcp)* quote held options by instrument URL
- *(options)* Expose equity contract search
- *(core)* Harden the OAuth token cache file permissions
- *(config)* Refuse secret-bearing config files with loose permissions
- created BACKLOG.md, enabled lints

### Fixed

- formatting
- *(core)* sanitize unusable instrument diagnostics
- *(core)* persist token cache atomically
- *(core)* follow watchlist and news pagination
- *(core)* reject malformed numeric environment overrides
- Extract API error diagnostics once, render them per surface
- *(core)* Check HTTP status before trusting the login payload
- *(core)* Repair main after a cross-order interaction between BL-25 and BL-9
- *(core)* Keep credentials out of logs and errors without losing diagnostics
- *(core)* Report per-order results from the cancel-all methods

### Other

- remove internal planning records
- *(core)* make API diagnostics extensible
- *(mcp)* cover security boundary entry points
- *(core)* Propagate cache-load errors directly from login

## [0.1.4](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.1.3...rhood-core-v0.1.4) - 2026-08-09

### Other

- *(deps)* Update rmcp to v3.1.2

## [0.1.3](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.1.2...rhood-core-v0.1.3) - 2026-07-29

### Added

- Upgrade to rmcp v3

## [0.1.1](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.1.0...rhood-core-v0.1.1) - 2026-06-10

### Other

- unit tests for rhood-core dividends and recurring
