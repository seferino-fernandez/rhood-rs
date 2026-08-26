# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/seferino-fernandez/rhood-rs/compare/v0.3.0...v0.3.1) - 2026-08-26

### Other

- update Cargo.lock dependencies

## [0.3.0](https://github.com/seferino-fernandez/rhood-rs/compare/v0.2.0...v0.3.0) - 2026-08-26

### Added

- add IPO Access read surfaces
- add portfolio performance surfaces
- *(cli)* add aggregate option positions
- add futures positions surfaces
- [**breaking**] add safe multi-account order preparation

### Fixed

- preserve RFC850 retry date semantics
- *(core)* make a recurring write reconcilable
- *(cli)* honour --output in the order command
- *(cli)* honour --output for watchlist and auth commands
- *(cli)* quote CSV fields containing a carriage return
- *(cli)* [**breaking**] make order-listing filters fail loudly
- *(cli)* reject impossible calendar dates
- refuse response data that cannot be attributed to its request
- *(mcp)* give every core error its own model-facing code
- *(futures)* [**breaking**] correlate quotes to requested symbols by instrument ID
- *(cli)* render the stop price in the trade confirmation prompt
- *(core)* use clearing cost for settled positions

### Other

- migrate direct chrono usage to jiff
- expand live CLI and MCP coverage
- *(cli)* make two tests able to fail
- *(cli)* describe what the --yes test actually proves
- *(cli)* make a lint suppression's reason true
- correct the write-safety surface and gate the docs build
- let release-plz own the changelogs
- drop internal tracker references from the public tree
- formatting
- *(order)* close the remaining proof and documentation gaps
- *(workspace)* [**breaking**] close order preparation and isolate MCP confirmation
- explain core instrument identity enrichment
- *(cli)* consume core instrument enrichment

## [0.2.0](https://github.com/seferino-fernandez/rhood-rs/compare/v0.1.4...v0.2.0) - 2026-08-11

### Added

- Enable the workspace lint policy

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
