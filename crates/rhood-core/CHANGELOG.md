# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
