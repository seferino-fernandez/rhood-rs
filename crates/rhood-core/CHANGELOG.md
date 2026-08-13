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
- Add one `prepare_order` and `submit_order` interface that freezes exact stock and option execution state before user confirmation.
- Add `RobinhoodClient::get_futures_positions` with authenticated full-URL pagination and tolerant raw position objects.
- `OrderAmount::required_time_in_force` exposes the duration the brokerage requires for an amount, so adapters ask instead of restating the rule. Preparation validates against the same function.

### Changed

- Account-sensitive methods now require a canonical account number instead of selecting the first standard account.
- Order preparation and submission are now one two-step interface for both asset types, built on `OrderIntent`, `StagedOrder`, typed `OrderSummary`, and `SubmittedOrder`. See `### Removed` for the methods and types this replaced.
- `get_futures_quotes` now returns `Vec<(String, FuturesQuote)>`, pairing each quote with the caller-supplied symbol it answers. Correlation uses the resolved instrument ID rather than response position, so a reordered or short upstream batch can no longer attribute a quote to the wrong symbol. A symbol the batch does not answer is now absent from the results instead of shifting the symbols after it, and a response item with no instrument ID, or one outside the request, fails the call with `RhoodError::InvalidMarketDataResponse`. Breaking for callers that consumed the previous flat `Vec<FuturesQuote>`.
- Order symbol validation now accepts 1 to 12 ASCII letters, numbers, or periods, where the retired MCP adapter accepted at most 6 characters. The bound is a pre-flight sanity check, not a brokerage invariant: unknown symbols are still rejected by instrument resolution. Seven-character symbols such as warrant tickers are now accepted before dispatch.
- Stock orders now send form version 7, a call-scoped client reference, derived session flags, and strict quote collars. Dollar buys convert to good-for-day fractional shares using the current ask.
- Authenticated GET requests now share paced starts and 429 cooldowns across client clones, retry connection errors, timeouts, and bounded rate limits up to three total attempts by default, and expose strict TOML/environment controls under `HttpConfig`.
- Replace the prior instrument-identity source and output contract with core-owned enrichment. Positions, dividends, stock orders, news, tags, and splits retain Instrument References, preserve Wire Symbols, and expose aligned optional Ticker Symbols. Use strict `resolve_instrument_symbols` for explicit resolution; normal reads enrich best-effort.

### Removed

- Remove `place_stock_order`, `place_option_order`, `prepare_stock_order`, `submit_prepared_stock_order`, and `PreparedStockOrder`, with no compatibility wrappers. Use `prepare_order` and `submit_order` with `OrderIntent`; both asset types now freeze their exact executable order before user confirmation.
- Remove the legacy public instrument-identity cache, parser, map resolver, and record-enrichment interfaces. Instrument Identity Enrichment is owned by the client and applied during normal reads; use `resolve_instrument_symbols` where explicit resolution is required.

### Fixed

- Reject stale or internally inconsistent Staged Orders, redirects, and automatic stock or option retries. Transport timeouts now return `SubmissionUncertain` with a reconciliation reference.

## [0.2.0](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.1.4...rhood-core-v0.2.0) - 2026-08-11

### Added

- *(options)* support explicit position effects
- *(core)* serialize bulk cancellation outcomes
- *(mcp)* quote held options by instrument URL
- *(options)* Expose equity contract search
- *(core)* Harden the OAuth token cache file permissions
- *(config)* Refuse secret-bearing config files with loose permissions
- Enable the workspace lint policy

### Fixed

- formatting
- *(core)* sanitize unusable instrument diagnostics
- *(core)* persist token cache atomically
- *(core)* follow watchlist and news pagination
- *(core)* reject malformed numeric environment overrides
- Extract API error diagnostics once, render them per surface
- *(core)* Check HTTP status before trusting the login payload
- *(core)* Repair main after an interaction between two concurrent changes
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
