# Homebrew + cargo + binstall Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rhood` (CLI) and `rhood-mcp` (MCP server) installable via Homebrew, `cargo install`, and `cargo binstall`.

**Architecture:** Two repos. In `rhood-rs`: extend the release-binaries workflow to also build/upload the `rhood-mcp` binary, add `cargo-binstall` metadata to both binary crates, and document installation. In `homebrew-tools` (the tap): add one formula per binary plus `just` update recipes. Work splits into **Phase A** (shippable now — `rhood`'s prebuilt assets already exist on the `v0.1.1` release) and **Phase B** (after the next release that ships `rhood-mcp` assets).

**Tech Stack:** GitHub Actions (`taiki-e/upload-rust-binary-action`), Cargo workspace metadata, Homebrew Ruby formulas, `just`, `gh` CLI.

**Spec:** `docs/superpowers/specs/2026-06-09-homebrew-cargo-distribution-design.md`

---

## Key facts (verified)

- Workspace version is `0.1.1`; tag format is `v{version}` (e.g. `v0.1.1`).
- Release archives are named `<binary>-<target>.{tar.gz,zip}` with the binary at the **archive root** (verified: `tar -tzf rhood-x86_64-apple-darwin.tar.gz` → `rhood`).
- The `v0.1.1` GitHub Release already has `rhood-<target>` archives for all 6 targets but **no `rhood-mcp-<target>`** archives (the workflow built only `rhood`).
- All three crates are published to crates.io at `0.1.1`, so `cargo install rhood-cli` / `cargo install rhood-mcp` already work.
- `cargo binstall` reads `[package.metadata.binstall]` from the version being installed, so binstall works only from the next published version (`0.1.2`+).
- Formula name == binary name == release-asset prefix for every tool in the tap (`noaa-weather`, `rhood`, `rhood-mcp`). The recipes rely on this.

**Real `v0.1.1` `rhood` checksums (used in Task 9):**

| Target                       | sha256                                                             |
| ---------------------------- | ----------------------------------------------------------------- |
| `x86_64-apple-darwin`        | `afd0863e9dbc84454c962ddb0e75b29777b992380ef6963b12d5e9ab5feee0c1` |
| `aarch64-apple-darwin`       | `65fd743bfbd965e9dba6fcd2ff63433a1fc4cc99765d9396d862689ab819033e` |
| `x86_64-unknown-linux-gnu`   | `4192c3850405b3ee6e3bdfe6f01508d5037f7bba4e2bdcd7be0c7eabe280eb53` |
| `aarch64-unknown-linux-gnu`  | `eea9047d55b761306c37e0aa16418ef211df564372b1b81dd8d2a882b0366010` |

---

## File Structure

**`rhood-rs` repo** (`/Users/seferinofernandez/open-source/robin-rs`):
- Modify: `.github/workflows/release-binaries.yaml` — build + upload both binaries.
- Modify: `crates/rhood-cli/Cargo.toml` — add `[package.metadata.binstall]`.
- Modify: `crates/rhood-mcp/Cargo.toml` — add `[package.metadata.binstall]`.
- Modify: `README.md` — add `## Installation`.
- Modify: `crates/rhood-cli/README.md` — replace the "out of scope" note with install steps.
- Modify: `crates/rhood-mcp/README.md` — add `## Install`.

**`homebrew-tools` repo** (`/Users/seferinofernandez/open-source/homebrew-tools`):
- Modify: `justfile` — disambiguate the generic recipes; add `rhood` / `rhood-mcp` recipes.
- Create: `Formula/rhood.rb` — CLI formula (real `v0.1.1` checksums, Phase A).
- Create: `Formula/rhood-mcp.rb` — MCP formula (skeleton in Phase A, filled in Phase B).
- Modify: `README.md` — list the new formulas.

---

# Part 1 — `rhood-rs` (Phase A)

All commands in Part 1 run from `/Users/seferinofernandez/open-source/robin-rs`.

### Task 0: Create working branch

- [ ] **Step 1: Branch off main**

```bash
cd /Users/seferinofernandez/open-source/robin-rs
git checkout -b feat/homebrew-cargo-distribution
git status
```

Expected: `On branch feat/homebrew-cargo-distribution`, clean tree (the committed spec is already on main).

---

### Task 1: Build and upload both binaries in the release workflow

**Files:**
- Modify: `.github/workflows/release-binaries.yaml`

- [ ] **Step 1: Remove the single-binary `env` block**

Delete these two lines near the top of the file (just below `permissions: {}`):

```yaml
env:
    BINARY_NAME: "rhood"
```

- [ ] **Step 2: Replace the build + upload steps**

Find the existing block:

```yaml
            - name: Build binary
              run: cargo build --release --locked --bin ${{ env.BINARY_NAME }} --target ${{ matrix.target }}
              env:
                  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: ${{ matrix.target == 'aarch64-unknown-linux-gnu' && 'aarch64-linux-gnu-gcc' || '' }}

            - name: Upload binary to release
              uses: taiki-e/upload-rust-binary-action@v1
              with:
                  bin: ${{ env.BINARY_NAME }}
                  target: ${{ matrix.target }}
                  token: ${{ secrets.GITHUB_TOKEN }}
                  tar: unix
                  zip: windows
```

Replace it with:

```yaml
            - name: Build binaries
              run: cargo build --release --locked --bin rhood --bin rhood-mcp --target ${{ matrix.target }}
              env:
                  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: ${{ matrix.target == 'aarch64-unknown-linux-gnu' && 'aarch64-linux-gnu-gcc' || '' }}

            - name: Upload rhood (CLI) to release
              uses: taiki-e/upload-rust-binary-action@v1
              with:
                  bin: rhood
                  target: ${{ matrix.target }}
                  token: ${{ secrets.GITHUB_TOKEN }}
                  tar: unix
                  zip: windows

            - name: Upload rhood-mcp (MCP server) to release
              uses: taiki-e/upload-rust-binary-action@v1
              with:
                  bin: rhood-mcp
                  target: ${{ matrix.target }}
                  token: ${{ secrets.GITHUB_TOKEN }}
                  tar: unix
                  zip: windows
```

Rationale: the explicit `cargo build` pre-builds both binaries (with the cross-linker env), so each `upload-rust-binary-action` step finds its binary already built and just archives/uploads it as a separate `<bin>-<target>` archive — mirroring the proven single-binary pattern.

- [ ] **Step 3: Validate the YAML parses**

Run:

```bash
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release-binaries.yaml')); print('YAML OK')"
```

Expected: `YAML OK` (no traceback).

- [ ] **Step 4: Confirm both binaries build locally for the host target**

Run:

```bash
cargo build --release --locked --bin rhood --bin rhood-mcp
ls -1 target/release/rhood target/release/rhood-mcp
```

Expected: both paths listed, no build errors.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release-binaries.yaml
git commit -m "ci: build and upload rhood-mcp binary alongside rhood"
```

---

### Task 2: Add cargo-binstall metadata to `rhood-cli`

**Files:**
- Modify: `crates/rhood-cli/Cargo.toml`

- [ ] **Step 1: Append the binstall metadata block**

Add this at the **end** of `crates/rhood-cli/Cargo.toml`. The `pkg-url` hardcodes the `rhood-` prefix because `{ name }` would resolve to the crate name `rhood-cli`, not the binary name `rhood`:

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

- [ ] **Step 2: Verify the manifest still parses and metadata is present**

Run:

```bash
cargo metadata --no-deps --format-version 1 --manifest-path crates/rhood-cli/Cargo.toml \
  | python3 -c "import json,sys; m=json.load(sys.stdin); p=[x for x in m['packages'] if x['name']=='rhood-cli'][0]; print(p['metadata']['binstall']['pkg-url'])"
```

Expected: `{ repo }/releases/download/v{ version }/rhood-{ target }.{ archive-format }`

- [ ] **Step 3: Commit**

```bash
git add crates/rhood-cli/Cargo.toml
git commit -m "build: add cargo-binstall metadata to rhood-cli"
```

---

### Task 3: Add cargo-binstall metadata to `rhood-mcp`

**Files:**
- Modify: `crates/rhood-mcp/Cargo.toml`

- [ ] **Step 1: Append the binstall metadata block**

Add this at the **end** of `crates/rhood-mcp/Cargo.toml`. Here `{ name }` already equals the binary name `rhood-mcp`:

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

- [ ] **Step 2: Verify the manifest still parses and metadata is present**

Run:

```bash
cargo metadata --no-deps --format-version 1 --manifest-path crates/rhood-mcp/Cargo.toml \
  | python3 -c "import json,sys; m=json.load(sys.stdin); p=[x for x in m['packages'] if x['name']=='rhood-mcp'][0]; print(p['metadata']['binstall']['bin-dir'])"
```

Expected: `{ name }{ binary-ext }`

- [ ] **Step 3: Commit**

```bash
git add crates/rhood-mcp/Cargo.toml
git commit -m "build: add cargo-binstall metadata to rhood-mcp"
```

---

### Task 4: Add `## Installation` to the root README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Insert the Installation section**

Insert the following block **between** the `## Crates` table and the `## Quick Start` heading (i.e. immediately before the line `## Quick Start`):

```markdown
## Installation

### Homebrew (macOS & Linux)

```bash
brew tap seferino-fernandez/tools
brew install rhood       # CLI
brew install rhood-mcp   # MCP server
```

### Cargo (build from source)

```bash
cargo install rhood-cli   # installs the `rhood` binary
cargo install rhood-mcp   # installs the `rhood-mcp` binary
```

### cargo-binstall (prebuilt binaries, no compile)

```bash
cargo binstall rhood-cli
cargo binstall rhood-mcp
```

```

- [ ] **Step 2: Verify it rendered into place**

Run:

```bash
rg -n "## Installation|brew install rhood|cargo install rhood-cli|cargo binstall rhood-cli" README.md
```

Expected: matches for each, with `## Installation` appearing before `## Quick Start` (check ordering with `rg -n "^## " README.md`).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add installation instructions to root README"
```

---

### Task 5: Replace the "out of scope" note in the CLI README

**Files:**
- Modify: `crates/rhood-cli/README.md`

- [ ] **Step 1: Replace the line**

Find this exact line (currently line 15):

```markdown
Distribution packaging (pre-built binaries, `cargo install`, system packages) is out of scope for this crate.
```

Replace it with:

```markdown
## Install

```bash
brew install seferino-fernandez/tools/rhood   # Homebrew (macOS & Linux)
cargo install rhood-cli                        # build from source (crates.io)
cargo binstall rhood-cli                       # prebuilt binary, no compile
```

All three install the `rhood` binary.
```

- [ ] **Step 2: Verify**

Run:

```bash
rg -n "out of scope" crates/rhood-cli/README.md; echo "exit: $?"
rg -n "brew install seferino-fernandez/tools/rhood\b" crates/rhood-cli/README.md
```

Expected: the first `rg` prints nothing and `exit: 1` (note gone); the second matches the new Homebrew line.

- [ ] **Step 3: Commit**

```bash
git add crates/rhood-cli/README.md
git commit -m "docs: add install instructions to rhood-cli README"
```

---

### Task 6: Add `## Install` to the MCP README

**Files:**
- Modify: `crates/rhood-mcp/README.md`

- [ ] **Step 1: Insert the Install section**

Insert this block **between** the line `Built on [rmcp](https://github.com/modelcontextprotocol/rust-sdk) and [rhood-core](../rhood-core/).` and the `## Transports` heading:

```markdown
## Install

```bash
brew install seferino-fernandez/tools/rhood-mcp   # Homebrew (macOS & Linux)
cargo install rhood-mcp                            # build from source (crates.io)
cargo binstall rhood-mcp                           # prebuilt binary, no compile
```

```

- [ ] **Step 2: Verify**

Run:

```bash
rg -n "## Install|brew install seferino-fernandez/tools/rhood-mcp" crates/rhood-mcp/README.md
```

Expected: both lines match, with `## Install` appearing before `## Transports`.

- [ ] **Step 3: Commit**

```bash
git add crates/rhood-mcp/README.md
git commit -m "docs: add install instructions to rhood-mcp README"
```

---

### Task 7: Run the full CI gate locally

**Files:** none (verification only)

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --all --check
```

Expected: no output, exit 0.

- [ ] **Step 2: Clippy (matches CI exactly)**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: `Finished`, no warnings/errors.

- [ ] **Step 3: Tests**

Run:

```bash
cargo nextest run --all-targets --all-features
```

Expected: all tests pass (includes the `rhood-core` endpoint tests and the `test-helpers` integration tests).

- [ ] **Step 4: Push the branch and open a PR**

```bash
git push -u origin feat/homebrew-cargo-distribution
gh pr create --fill --title "feat: Homebrew, cargo install, and cargo binstall distribution"
```

Expected: PR URL printed. Do not merge until reviewed.

---

# Part 2 — `homebrew-tools` tap (Phase A)

All commands in Part 2 run from `/Users/seferinofernandez/open-source/homebrew-tools`.

### Task 8: Branch and disambiguate the `just` recipes; add `rhood` recipes

**Files:**
- Modify: `justfile`

**Why the recipe change is required:** the current `update-formula` / `apply-update` recipes download `*.tar.gz` and glob `ls *x86_64-apple-darwin*.tar.gz`. A future release that contains both `rhood-*` and `rhood-mcp-*` archives makes that glob match **two** files, breaking the update. The fix: select each target archive by its **exact** filename `<formula>-<target>.tar.gz`. This also keeps `noaa-weather` working (its assets follow the same naming).

- [ ] **Step 1: Branch off main**

```bash
cd /Users/seferinofernandez/open-source/homebrew-tools
git checkout -b feat/rhood-formulas
```

- [ ] **Step 2: Replace the `update-formula` recipe**

Replace the entire existing `update-formula formula repo:` recipe with:

```just
# Show the latest release version and per-arch checksums for a formula
update-formula formula repo:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "🔍 Fetching latest release for {{repo}}..."
    LATEST_TAG=$(gh release view --repo {{repo}} --json tagName --jq '.tagName')
    echo "📦 Latest version: $LATEST_TAG"

    TEMP_DIR=$(mktemp -d)

    # Exact per-target archive names. The formula name is the asset prefix, so an
    # exact filename avoids matching a sibling tool that shares this prefix
    # (e.g. "rhood" must not pick up "rhood-mcp" archives).
    INTEL_MAC="{{formula}}-x86_64-apple-darwin.tar.gz"
    ARM_MAC="{{formula}}-aarch64-apple-darwin.tar.gz"
    INTEL_LINUX="{{formula}}-x86_64-unknown-linux-gnu.tar.gz"
    ARM_LINUX="{{formula}}-aarch64-unknown-linux-gnu.tar.gz"

    echo "⬇️  Downloading release assets..."
    for f in "$INTEL_MAC" "$ARM_MAC" "$INTEL_LINUX" "$ARM_LINUX"; do
        gh release download "$LATEST_TAG" --repo {{repo}} --pattern "$f" --dir "$TEMP_DIR" 2>/dev/null || true
    done

    sha() { if [[ -f "$TEMP_DIR/$1" ]]; then shasum -a 256 "$TEMP_DIR/$1" | cut -d' ' -f1; fi; }

    echo "🔐 Checksums:"
    [[ -f "$TEMP_DIR/$INTEL_MAC"   ]] && echo "  macOS Intel : $(sha "$INTEL_MAC")"
    [[ -f "$TEMP_DIR/$ARM_MAC"     ]] && echo "  macOS ARM   : $(sha "$ARM_MAC")"
    [[ -f "$TEMP_DIR/$INTEL_LINUX" ]] && echo "  Linux Intel : $(sha "$INTEL_LINUX")"
    [[ -f "$TEMP_DIR/$ARM_LINUX"   ]] && echo "  Linux ARM   : $(sha "$ARM_LINUX")"

    rm -rf "$TEMP_DIR"
    echo ""
    echo "Run 'just apply-update {{formula}} {{repo}} $LATEST_TAG' to apply changes"
```

- [ ] **Step 3: Replace the `apply-update` recipe**

Replace the entire existing `apply-update formula repo version:` recipe with:

```just
# Apply a version + checksum update to a formula file
apply-update formula repo version:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "🔄 Updating Formula/{{formula}}.rb to version {{version}}..."
    TEMP_DIR=$(mktemp -d)

    INTEL_MAC="{{formula}}-x86_64-apple-darwin.tar.gz"
    ARM_MAC="{{formula}}-aarch64-apple-darwin.tar.gz"
    INTEL_LINUX="{{formula}}-x86_64-unknown-linux-gnu.tar.gz"
    ARM_LINUX="{{formula}}-aarch64-unknown-linux-gnu.tar.gz"

    for f in "$INTEL_MAC" "$ARM_MAC" "$INTEL_LINUX" "$ARM_LINUX"; do
        gh release download "{{version}}" --repo {{repo}} --pattern "$f" --dir "$TEMP_DIR" 2>/dev/null || true
    done

    sha() { if [[ -f "$TEMP_DIR/$1" ]]; then shasum -a 256 "$TEMP_DIR/$1" | cut -d' ' -f1; fi; }
    INTEL_MAC_SHA=$(sha "$INTEL_MAC")
    ARM_MAC_SHA=$(sha "$ARM_MAC")
    INTEL_LINUX_SHA=$(sha "$INTEL_LINUX")
    ARM_LINUX_SHA=$(sha "$ARM_LINUX")

    rm -rf "$TEMP_DIR"

    FORMULA_FILE="Formula/{{formula}}.rb"
    BASE="https://github.com/{{repo}}/releases/download/{{version}}"
    VERSION_NUM=$(echo "{{version}}" | sed 's/^v//')

    sed -i '' "s/version \".*\"/version \"$VERSION_NUM\"/" "$FORMULA_FILE"

    if [[ -n "$INTEL_MAC_SHA" ]]; then
        sed -i '' "s|url \".*x86_64-apple-darwin.*\"|url \"$BASE/$INTEL_MAC\"|" "$FORMULA_FILE"
        sed -i '' "/x86_64-apple-darwin/,/sha256/ s/sha256 \".*\"/sha256 \"$INTEL_MAC_SHA\"/" "$FORMULA_FILE"
    fi
    if [[ -n "$ARM_MAC_SHA" ]]; then
        sed -i '' "s|url \".*aarch64-apple-darwin.*\"|url \"$BASE/$ARM_MAC\"|" "$FORMULA_FILE"
        sed -i '' "/aarch64-apple-darwin/,/sha256/ s/sha256 \".*\"/sha256 \"$ARM_MAC_SHA\"/" "$FORMULA_FILE"
    fi
    if [[ -n "$INTEL_LINUX_SHA" ]]; then
        sed -i '' "s|url \".*x86_64-unknown-linux-gnu.*\"|url \"$BASE/$INTEL_LINUX\"|" "$FORMULA_FILE"
        sed -i '' "/x86_64-unknown-linux-gnu/,/sha256/ s/sha256 \".*\"/sha256 \"$INTEL_LINUX_SHA\"/" "$FORMULA_FILE"
    fi
    if [[ -n "$ARM_LINUX_SHA" ]]; then
        sed -i '' "s|url \".*aarch64-unknown-linux-gnu.*\"|url \"$BASE/$ARM_LINUX\"|" "$FORMULA_FILE"
        sed -i '' "/aarch64-unknown-linux-gnu/,/sha256/ s/sha256 \".*\"/sha256 \"$ARM_LINUX_SHA\"/" "$FORMULA_FILE"
    fi

    echo "✅ Updated $FORMULA_FILE to version {{version}}"
```

- [ ] **Step 4: Add the `rhood` / `rhood-mcp` convenience recipes**

Add these recipes after the existing `apply-noaa version:` recipe:

```just
# Inspect + show checksums for rhood (CLI)
update-rhood:
    just update-formula rhood seferino-fernandez/rhood-rs

# Apply a version update to the rhood (CLI) formula
apply-rhood version:
    just apply-update rhood seferino-fernandez/rhood-rs {{version}}

# Inspect + show checksums for rhood-mcp (MCP server)
update-rhood-mcp:
    just update-formula rhood-mcp seferino-fernandez/rhood-rs

# Apply a version update to the rhood-mcp formula
apply-rhood-mcp version:
    just apply-update rhood-mcp seferino-fernandez/rhood-rs {{version}}
```

- [ ] **Step 5: Verify just parses the file and lists the new recipes**

Run:

```bash
just --list | rg "apply-rhood|update-rhood|apply-rhood-mcp|update-rhood-mcp"
```

Expected: all four recipes listed (no `just` parse error).

- [ ] **Step 6: Commit**

```bash
git add justfile
git commit -m "build: add rhood/rhood-mcp recipes and disambiguate archive selection"
```

---

### Task 9: Create the `rhood` formula with real `v0.1.1` checksums

**Files:**
- Create: `Formula/rhood.rb`

- [ ] **Step 1: Write the formula**

Create `Formula/rhood.rb` with exactly:

```ruby
class Rhood < Formula
  desc "Terminal CLI for the Robinhood trading API"
  homepage "https://github.com/seferino-fernandez/rhood-rs"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-x86_64-apple-darwin.tar.gz"
      sha256 "afd0863e9dbc84454c962ddb0e75b29777b992380ef6963b12d5e9ab5feee0c1"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-aarch64-apple-darwin.tar.gz"
      sha256 "65fd743bfbd965e9dba6fcd2ff63433a1fc4cc99765d9396d862689ab819033e"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "4192c3850405b3ee6e3bdfe6f01508d5037f7bba4e2bdcd7be0c7eabe280eb53"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.1.1/rhood-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "eea9047d55b761306c37e0aa16418ef211df564372b1b81dd8d2a882b0366010"
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

- [ ] **Step 2: Syntax + style check**

Run:

```bash
ruby -c Formula/rhood.rb
brew style Formula/rhood.rb
```

Expected: `Syntax OK` and `brew style` reports no offenses.

- [ ] **Step 3: Strict audit (downloads assets, verifies checksums)**

Run:

```bash
brew audit --strict --online Formula/rhood.rb
```

Expected: no failures. (This downloads the macOS asset for your arch and checks the sha256 matches the formula — proving the checksums are correct.)

- [ ] **Step 4: Real install smoke test**

Run:

```bash
brew install --build-from-source ./Formula/rhood.rb
brew test rhood
rhood --version
brew uninstall rhood
```

Expected: install succeeds, `brew test` passes, `rhood --version` prints a version line.

- [ ] **Step 5: Commit**

```bash
git add Formula/rhood.rb
git commit -m "feat: add rhood CLI formula (v0.1.1)"
```

---

### Task 10: Add the `rhood` row to the tap README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add the table row**

In the "Available Formulas" table, add a row below the existing `noaa-weather` row:

```markdown
| `rhood`        | Terminal CLI for the Robinhood trading API.        | [seferino-fernandez/rhood-rs](https://github.com/seferino-fernandez/rhood-rs)         |
```

- [ ] **Step 2: Verify**

Run:

```bash
rg -n "\| \`rhood\`" README.md
```

Expected: one match (the new row). `rhood-mcp` is intentionally **not** added yet (its formula lands in Phase B).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: list rhood formula in tap README"
```

- [ ] **Step 4: Push and open a PR**

```bash
git push -u origin feat/rhood-formulas
gh pr create --fill --title "feat: add rhood formula + rhood/rhood-mcp update recipes"
```

Expected: PR URL printed.

---

# Part 3 — Phase B (run AFTER the next `rhood-rs` release that ships `rhood-mcp` assets)

> **Do not start Part 3 until:** the `rhood-rs` PR from Part 1 is merged, a new release (e.g. `v0.1.2`) has been cut by release-plz, and the `Release Binaries` workflow has uploaded `rhood-mcp-<target>` archives to that release. Verify with:
>
> ```bash
> gh release view v0.1.2 --repo seferino-fernandez/rhood-rs --json assets --jq '.assets[].name' | rg "rhood-mcp"
> ```
>
> Expected: four `rhood-mcp-*-apple-darwin`/`*-linux-gnu` archives (plus Windows zips). Substitute the actual tag for `v0.1.2` throughout.

### Task 11: Create and fill the `rhood-mcp` formula

**Files:**
- Create: `Formula/rhood-mcp.rb`

All commands run from `/Users/seferinofernandez/open-source/homebrew-tools` on a fresh branch:

```bash
cd /Users/seferinofernandez/open-source/homebrew-tools
git checkout main && git pull
git checkout -b feat/rhood-mcp-formula
```

- [ ] **Step 1: Write the formula skeleton**

Create `Formula/rhood-mcp.rb` with placeholder version/checksums (the `apply-rhood-mcp` recipe overwrites them by matching the target strings in the `url`/`sha256` lines):

```ruby
class RhoodMcp < Formula
  desc "Model Context Protocol server exposing the Robinhood API to LLM clients"
  homepage "https://github.com/seferino-fernandez/rhood-rs"
  version "0.0.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.0.0/rhood-mcp-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.0.0/rhood-mcp-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.0.0/rhood-mcp-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    if Hardware::CPU.arm?
      url "https://github.com/seferino-fernandez/rhood-rs/releases/download/v0.0.0/rhood-mcp-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "rhood-mcp"
  end

  test do
    system "#{bin}/rhood-mcp", "--version"
  end
end
```

- [ ] **Step 2: Fill version + real checksums from the release**

Run (substitute the real tag):

```bash
just apply-rhood-mcp v0.1.2
```

Expected: `✅ Updated Formula/rhood-mcp.rb to version v0.1.2`. Confirm no `0000…` checksums remain:

```bash
rg -n "0000000000000000" Formula/rhood-mcp.rb; echo "exit: $?"
```

Expected: no matches, `exit: 1`.

- [ ] **Step 3: Validate, audit, and smoke test**

Run:

```bash
ruby -c Formula/rhood-mcp.rb
brew style Formula/rhood-mcp.rb
brew audit --strict --online Formula/rhood-mcp.rb
brew install --build-from-source ./Formula/rhood-mcp.rb
brew test rhood-mcp
rhood-mcp --version
brew uninstall rhood-mcp
```

Expected: syntax OK, no style offenses, audit passes, install + test succeed, `--version` prints.

- [ ] **Step 4: Commit**

```bash
git add Formula/rhood-mcp.rb
git commit -m "feat: add rhood-mcp formula"
```

---

### Task 12: Align `rhood` formula to the new release and add the README row

**Files:**
- Modify: `Formula/rhood.rb`
- Modify: `README.md`

- [ ] **Step 1: Bump the `rhood` formula to the same release**

Keep both formulas on the same version. Run (substitute the real tag):

```bash
just apply-rhood v0.1.2
```

Expected: `✅ Updated Formula/rhood.rb to version v0.1.2`. Re-audit:

```bash
brew audit --strict --online Formula/rhood.rb
```

Expected: passes.

- [ ] **Step 2: Add the `rhood-mcp` README row**

In the "Available Formulas" table in `README.md`, add below the `rhood` row:

```markdown
| `rhood-mcp`    | MCP server exposing the Robinhood API to LLMs.     | [seferino-fernandez/rhood-rs](https://github.com/seferino-fernandez/rhood-rs)         |
```

- [ ] **Step 3: Verify**

Run:

```bash
rg -n "\| \`rhood-mcp\`" README.md
```

Expected: one match.

- [ ] **Step 4: Commit, push, open PR**

```bash
git add Formula/rhood.rb README.md
git commit -m "chore: align rhood formula to v0.1.2 and list rhood-mcp in README"
git push -u origin feat/rhood-mcp-formula
gh pr create --fill --title "feat: add rhood-mcp formula and align rhood to latest release"
```

Expected: PR URL printed.

---

## Final verification checklist

After both phases are merged:

- [ ] `brew tap seferino-fernandez/tools && brew install rhood && rhood --version` works on macOS and Linux.
- [ ] `brew install rhood-mcp && rhood-mcp --version` works (Phase B).
- [ ] `cargo install rhood-cli` installs the `rhood` binary; `cargo install rhood-mcp` installs `rhood-mcp`.
- [ ] After the `0.1.2`+ release: `cargo binstall rhood-cli` and `cargo binstall rhood-mcp` download prebuilt binaries without compiling.
- [ ] The `Release Binaries` workflow run for the latest release shows both `rhood-<target>` and `rhood-mcp-<target>` archives for all six targets.
- [ ] No real credentials/tokens appear in any formula, recipe, or README (only public URLs and crate names).
