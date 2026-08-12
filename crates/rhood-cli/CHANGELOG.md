# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add global `--account` selection and `account list` discovery for individual and IRA accounts.
- Add `rhood futures positions` with compact raw-object table and CSV rows and raw-array JSON output.
- Add `rhood option aggregate-positions` for account-filtered aggregate option strategies.
- Add `rhood account performance` for the raw Bonfire returns chart model.
- Add read-only `rhood ipo list` and `rhood ipo cards` commands.

### Changed

- Stock orders are fully prepared before confirmation and display their resolved quantity, collar quote, client reference, and form version.
- Dollar buys require at least $1 and use a good-for-day fractional-share estimate.
- Tag JSON now preserves Instrument References and the core-provided, positionally aligned optional Ticker Symbol array. Human tag output falls back to the original Instrument Reference for an unresolved symbol.

### Fixed

- Fundamentals table output now uses each returned record's Ticker Symbol, preventing an omitted earlier result from mislabeling a later record.

## [0.2.0](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.4...v0.2.0) - 2026-08-11

### Added

- created BACKLOG.md, enabled lints

### Fixed

- formatting

### Other

- remove internal planning records
- clarify lint suppression assumptions

## [0.1.4](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.3...v0.1.4) - 2026-08-09

### Other

- *(deps)* Update rmcp to v3.1.2

## [0.1.3](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.2...v0.1.3) - 2026-07-29

### Added

- Upgrade to rmcp v3

### Other

- updated install instructions for homebrew and cargo binstall

## [0.1.2](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.1...v0.1.2) - 2026-06-10

### Added

- Homebrew, cargo install, and cargo binstall distribution ([#3](https://github.com/seferino-fernandez/rhood-rs/pull/3))

## [0.1.1](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.0...v0.1.1) - 2026-06-10

### Other

- *(deps)* Bump the dependencies group with 8 updates
