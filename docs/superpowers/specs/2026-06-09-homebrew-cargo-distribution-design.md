# Design: Homebrew + cargo + binstall distribution for `rhood` and `rhood-mcp`

**Date:** 2026-06-09
**Status:** Approved design — ready for implementation plan
**Repos touched:** `seferino-fernandez/rhood-rs` (this repo) and `seferino-fernandez/homebrew-tools`

## Goal

Make both binaries shipped by this workspace installable through the three
channels a Rust CLI user expects:

- **Homebrew** via the existing personal tap (`brew tap seferino-fernandez/tools`)
- **`cargo install`** from crates.io
- **`cargo binstall`** (prebuilt-binary fast path)

The two binaries:

| Crate        | Binary      | Purpose                          |
| ------------ | ----------- | -------------------------------- |
| `rhood-cli`  | `rhood`     | Terminal CLI for the Robinhood API |
| `rhood-mcp`  | `rhood-mcp` | MCP server for LLM clients         |

Decision (confirmed during brainstorming): **two separate Homebrew formulas**
(`brew install rhood`, `brew install rhood-mcp`), and **manual `just`-based
formula updates** matching the current tap workflow.

## Current state (post-`v0.1.1`)

- Workspace version is `0.1.1`. Tag `v0.1.1` and a GitHub Release exist.
- release-plz published all three crates (`rhood-core`, `rhood-cli`, `rhood-mcp`)
  to crates.io at `0.1.1`.
- `release-binaries.yaml` still builds **only** `rhood` (`env.BINARY_NAME: "rhood"`).
  The `v0.1.1` Release therefore has `rhood-<target>` archives for all six targets
  but **no `rhood-mcp-<target>` archives**.
- Release tag format is `v{version}` (set in `release-plz.toml`, anchored on `rhood-cli`).
- Release archives are produced by `taiki-e/upload-rust-binary-action` with
  `tar: unix` / `zip: windows`, named `<bin>-<target>.<ext>` with the binary at
  the archive **root** (e.g. `rhood-x86_64-apple-darwin.tar.gz`).
- The tap (`homebrew-tools`) has one formula (`noaa-weather.rb`) that downloads
  prebuilt release tarballs, branches on `Hardware::CPU`, pins `sha256`, and is
  updated through `just` recipes in `justfile`.

## What works when (consequence of the above)

| Channel                  | `rhood` (CLI)                     | `rhood-mcp`                                  |
| ------------------------ | --------------------------------- | -------------------------------------------- |
| `cargo install <crate>`  | Now (`0.1.1` on crates.io)        | Now (`0.1.1` on crates.io)                   |
| Homebrew                 | Now — real `v0.1.1` checksums     | After next release that ships `rhood-mcp` assets |
| `cargo binstall <crate>` | Next release (needs binstall metadata) | Next release (needs metadata + mcp assets) |

`cargo binstall` reads `[package.metadata.binstall]` from the crate version it is
installing. `0.1.1` was published without that metadata, so binstall only works
from the next published version (`0.1.2`+) for both crates.

## Changes — `rhood-rs` (this repo)

### 1. `release-binaries.yaml` — build and upload both binaries

Each matrix job must produce a **separate** archive per binary so each maps to its
own formula. Plan:

- Remove the single-value `env.BINARY_NAME: "rhood"`.
- Build both binaries in one step:
  `cargo build --release --locked --bin rhood --bin rhood-mcp --target <target>`
  (keep the existing `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER` env for the
  Linux aarch64 cross job).
- Invoke `taiki-e/upload-rust-binary-action` **twice**, once with `bin: rhood`
  and once with `bin: rhood-mcp`, keeping `tar: unix` / `zip: windows`.
  Each call uploads `<bin>-<target>.{tar.gz,zip}`.
- Matrix targets unchanged: Linux x86_64/aarch64, macOS x86_64/aarch64,
  Windows x86_64/aarch64.

Result after the next release: `rhood-<target>` **and** `rhood-mcp-<target>`
archives attached to the Release.

### 2. `[package.metadata.binstall]` in both binary crates

Required because (a) `rhood-cli`'s crate name differs from its binary name,
(b) the tag is `v{version}`, and (c) the binary sits at the archive root (binstall's
default `bin-dir` assumes a `<name>-<target>/` subdirectory).

`crates/rhood-cli/Cargo.toml` — note the hardcoded `rhood-` prefix because
`{ name }` would resolve to `rhood-cli`:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/rhood-{ target }.{ archive-format }"
bin-dir = "rhood{ binary-ext }"
pkg-fmt = "tgz"

[package.metadata.binstall.overrides.x86_64-pc-windows-msvc]
pkg-fmt = "zip"
[package.metadata.binstall.overrides.aarch64-pc-windows-msvc]
pkg-fmt = "zip"
```

`crates/rhood-mcp/Cargo.toml` — `{ name }` already equals the binary name:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }.{ archive-format }"
bin-dir = "{ name }{ binary-ext }"
pkg-fmt = "tgz"

[package.metadata.binstall.overrides.x86_64-pc-windows-msvc]
pkg-fmt = "zip"
[package.metadata.binstall.overrides.aarch64-pc-windows-msvc]
pkg-fmt = "zip"
```

These are metadata-only additions; they don't affect normal builds, clippy, or
the published library API. CI (`fmt`, `clippy --all-targets --all-features`,
`nextest`) must still pass.

### 3. README updates

- **`README.md` (root):** add an `## Installation` section before `## Quick Start`
  covering Homebrew (tap + both installs), `cargo install`, and `cargo binstall`
  for both binaries.
- **`crates/rhood-cli/README.md`:** replace the line that reads
  *"Distribution packaging (pre-built binaries, `cargo install`, system packages)
  is out of scope for this crate."* with real instructions:
  `brew install rhood`, `cargo install rhood-cli`, `cargo binstall rhood-cli`
  (each installs the `rhood` binary).
- **`crates/rhood-mcp/README.md`:** add an install block (`brew install rhood-mcp`,
  `cargo install rhood-mcp`, `cargo binstall rhood-mcp`) near the top, before
  `## Quick Start`.

Match the existing terse, professional tone. Keep the unofficial/community
disclaimer intact.

## Changes — `homebrew-tools` (tap repo)

### 4. `Formula/rhood.rb` (class `Rhood`) — ship with real `v0.1.1` checksums

Mirror `noaa-weather.rb`, but cover **four** bottle targets (macOS intel/arm and
Linux intel/arm — `noaa-weather` only had Linux intel):

```ruby
class Rhood < Formula
  desc "Terminal CLI for the Robinhood trading API"
  homepage "https://github.com/seferino-fernandez/rhood-rs"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-x86_64-apple-darwin.tar.gz"
      sha256 "<fill from release asset>"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-aarch64-apple-darwin.tar.gz"
      sha256 "<fill from release asset>"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "<fill from release asset>"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "<fill from release asset>"
    end
  end

  def install
    bin.install "rhood"
  end

  test do
    system "#{bin}/rhood", "--version"
  end
end
```

Checksums are filled by running the `just` recipe (below) against the existing
`v0.1.1` assets. Windows is not a Homebrew target — those users use cargo/binstall.

### 5. `Formula/rhood-mcp.rb` (class `RhoodMcp`)

Identical structure to `rhood.rb` with:
- `desc "Model Context Protocol server exposing the Robinhood API to LLM clients"`
- URLs using the `rhood-mcp-<target>` archive names
- `bin.install "rhood-mcp"` and `test` calling `#{bin}/rhood-mcp --version`

Authored now, but **real checksums require the next release** (the one that ships
`rhood-mcp` assets). Until then this formula cannot be valid/committed with real
`sha256` values. It is filled and committed in Phase B.

### 6. `justfile` — update recipes for both formulas

Add recipes mirroring the existing `update-noaa` / `apply-noaa`:

```
update-rhood:           just update-formula rhood     seferino-fernandez/rhood-rs
apply-rhood version:    just apply-update   rhood     seferino-fernandez/rhood-rs {{version}}
update-rhood-mcp:       just update-formula rhood-mcp seferino-fernandez/rhood-rs
apply-rhood-mcp version: just apply-update  rhood-mcp seferino-fernandez/rhood-rs {{version}}
```

The generic `update-formula` / `apply-update` / `inspect-release` recipes already
compute multi-arch checksums and rewrite per-arch `url`/`sha256`, so they are
reused unchanged.

**Risk to verify during implementation:** `apply-update`'s `sed` selects the
archive by globbing `*x86_64-apple-darwin*.tar.gz` etc. In a `v0.1.2` release that
contains both `rhood-*` and `rhood-mcp-*` archives, `ls *aarch64-apple-darwin*`
matches **two** files. The recipe must operate on the single formula's archive
(e.g. by globbing `<formula>-<target>` or filtering the download with
`--pattern "<formula>-*"`). Implementation must confirm `apply-rhood` and
`apply-rhood-mcp` each pick the correct archive and don't cross-contaminate. If the
existing recipe can't disambiguate, scope the `gh release download --pattern` to
the formula name.

### 7. `README.md` (tap) — list the new formulas

Add rows to the "Available Formulas" table:

| Formula     | Description                            | Source Repository                |
| ----------- | -------------------------------------- | -------------------------------- |
| `rhood`     | CLI for the Robinhood trading API      | `seferino-fernandez/rhood-rs`    |
| `rhood-mcp` | MCP server for the Robinhood API       | `seferino-fernandez/rhood-rs`    |

## Sequencing

**Phase A — mergeable immediately (no new release required):**

1. `rhood-rs`: edit `release-binaries.yaml`; add binstall metadata to both crate
   manifests; update the three READMEs. Verify `fmt` + `clippy --all-targets
   --all-features` + `nextest` pass locally.
2. `homebrew-tools`: add `rhood.rb` with **real `v0.1.1` checksums** (run
   `just inspect-release` / `just apply-rhood v0.1.1`); add the `just` recipes;
   add `rhood` (and `rhood-mcp`) rows to the tap README. Author `rhood-mcp.rb`
   structurally but it stays Phase B for checksums. Validate with
   `just validate rhood` / `brew audit --strict`.

After Phase A, `brew install rhood`, `cargo install rhood-cli`, and
`cargo install rhood-mcp` all work.

**Phase B — after the next release (`v0.1.2`, first to ship `rhood-mcp` assets):**

3. Run `just apply-rhood-mcp v0.1.2` to fill `rhood-mcp.rb` checksums; commit.
4. Optionally bump `rhood.rb` to `v0.1.2` (`just apply-rhood v0.1.2`) to keep the
   two formulas version-aligned.
5. `cargo binstall rhood-cli` and `cargo binstall rhood-mcp` now work (metadata is
   in the `0.1.2` crates).

## Verification

- **rhood-rs:** `cargo fmt --all --check`; `cargo clippy --all-targets
  --all-features -- -D warnings`; `cargo nextest run --all-targets --all-features`.
  Confirm the workflow YAML is valid and both upload steps reference the right
  `bin`. (Markdown-only changes are skipped by CI via `paths-ignore`, so run
  checks locally.)
- **homebrew-tools:** `just validate rhood` (`ruby -c` + `brew style`);
  `brew audit --strict Formula/rhood.rb`; optionally
  `brew install --build-from-source ./Formula/rhood.rb && brew test rhood`.
- Confirm every formula `url` resolves and each `sha256` matches the published
  asset (`shasum -a 256` vs the downloaded archive).
- Confirm no real credentials/tokens appear anywhere (formulas and READMEs only
  reference public URLs and the public crate names).

## Out of scope

- Cross-repo automation that auto-bumps the tap from CI (explicitly deferred;
  manual `just` chosen).
- Windows packaging (scoop/winget) — cargo/binstall cover Windows.
- Changing `release-plz` / tag conventions.
