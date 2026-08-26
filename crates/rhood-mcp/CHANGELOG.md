# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.3.0...rhood-mcp-v0.3.1) - 2026-08-26

### Other

- update deps to latest rmcp, moka

## [0.3.0](https://github.com/seferino-fernandez/rhood-rs/compare/rhood-mcp-v0.2.0...rhood-mcp-v0.3.0) - 2026-08-26

### Added

- add IPO Access read surfaces
- add portfolio performance surfaces
- *(mcp)* add aggregate option positions
- add futures positions surfaces
- [**breaking**] add safe multi-account order preparation

### Fixed

- *(core)* make a recurring write reconcilable
- *(cli)* [**breaking**] make order-listing filters fail loudly
- *(mcp)* give every core error its own model-facing code
- *(futures)* [**breaking**] correlate quotes to requested symbols by instrument ID
- *(cli)* render the stop price in the trade confirmation prompt
- *(core)* chunk market data requests safely

### Other

- migrate direct chrono usage to jiff
- expand live CLI and MCP coverage
- let release-plz own the changelogs
- drop internal tracker references from the public tree
- *(order)* close the remaining proof and documentation gaps
- *(mcp)* assemble HTTP runtime
- *(mcp)* move params into tool modules
- *(mcp)* cover confirmation invariants
- *(mcp)* type confirmation errors
- *(mcp)* derive policy from annotations
- *(mcp)* changelog the order-expiry error code rename
- *(workspace)* [**breaking**] close order preparation and isolate MCP confirmation
- explain core instrument identity enrichment
- *(mcp)* consume core instrument enrichment
- *(core)* centralize instrument identity enrichment

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
