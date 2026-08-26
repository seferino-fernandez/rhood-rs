# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.3.0...rhood-core-v0.3.1) - 2026-08-26

### Other

- update Cargo.toml dependencies

## [0.3.0](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-core-v0.2.0...rhood-core-v0.3.0) - 2026-08-26

### Added

- add IPO Access read surfaces
- add portfolio performance surfaces
- *(core)* add aggregate option positions
- add futures positions surfaces
- *(core)* pace and retry safe reads
- [**breaking**] add safe multi-account order preparation

### Fixed

- preserve RFC850 retry date semantics
- *(core)* make a recurring write reconcilable
- *(core)* refuse a dividend total that cannot be trusted
- *(cli)* [**breaking**] make order-listing filters fail loudly
- *(core)* attribute fundamentals by instrument reference
- refuse response data that cannot be attributed to its request
- *(core)* isolate the staged-order clock seam per thread
- *(mcp)* give every core error its own model-facing code
- *(futures)* [**breaking**] correlate quotes to requested symbols by instrument ID
- *(core)* chunk market data requests safely
- *(core)* retain refresh credentials after access expiry
- *(core)* use clearing cost for settled positions

### Other

- migrate direct chrono usage to jiff
- characterize chrono temporal contracts
- expand live CLI and MCP coverage
- *(core)* stop claiming the movers route paginates
- correct the write-safety surface and gate the docs build
- let release-plz own the changelogs
- drop internal tracker references from the public tree
- *(order)* close the remaining proof and documentation gaps
- *(core)* force summary coverage and prove staged order lifetimes
- *(workspace)* [**breaking**] close order preparation and isolate MCP confirmation
- explain core instrument identity enrichment
- *(core)* [**breaking**] remove legacy enrichment interfaces
- *(core)* centralize instrument identity enrichment
- *(core)* split configuration module
- *(core)* document position enrichment errors

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
