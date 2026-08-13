default:
    @just --list

# The gate: mirrors CI's pull-request-validation, mutates nothing, red on any failure
verify:
    cargo fmt --all --check
    cargo check --locked --workspace --all-targets --all-features
    cargo clippy --all-targets --all-features -- -D warnings
    cargo nextest run --all-targets --all-features
    just docs-check

# The docs gate. `cargo nextest` skips doctests by design, so without the first
# command the examples in the documentation are never run. The lints that catch
# documentation rot -- broken intra-doc links, bare URLs, invalid Rust code
# blocks -- only warn, so without `-D warnings` the second exits zero over a
# page of dead links.
docs-check:
    cargo test --doc --all-features
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# Run all tests: full feature matrix, default features, doctests, and the
# thread-parallel runner. `cargo nextest` gives every test its own process, so
# it cannot see tests leaking shared state into each other; `cargo test` shares
# one process and does. Keep both: a process-wide test override is invisible to
# nextest and fails a different set of tests on every `cargo test` run.
test:
	cargo nextest run --all-targets --all-features
	cargo nextest run
	cargo test --workspace --all-features
	cargo test --doc

# Run live-API integration tests against Robinhood (requires credentials)
integration:
	cargo test --test integration -p rhood-cli -- --ignored --nocapture

# Lint the code with Clippy, at the same severity CI enforces
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Format code with rustfmt and apply Clippy's auto-fixable suggestions
format:
    cargo fmt --all
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged

# Build the project in debug mode
build:
    cargo build

# Install both the CLI and MCP server in parallel
[parallel]
install: install-cli install-mcp

# Install the rhood CLI tool
install-cli:
    cargo install --path crates/rhood-cli

# Install the rhood MCP server
install-mcp:
    cargo install --path crates/rhood-mcp

# Run rhood-mcp (HTTP transport) and the MCP Inspector UI in parallel; point the UI at http://127.0.0.1:8080/mcp
[parallel]
inspect: debug-mcp inspector-ui

# Run the debug build of rhood-mcp over HTTP on 127.0.0.1:8080 (requires `just build` first)
debug-mcp:
    ./target/debug/rhood-mcp --transport http

# Launch the MCP Inspector UI at http://localhost:6274 (proxy prints a session token)
inspector-ui:
    npx -y @modelcontextprotocol/inspector@latest

# Build the project in release mode for production
release:
    cargo build --release

# Build with dependency metadata embedded, then scan the binary for known vulnerabilities
audit:
    cargo auditable build --release
    cargo audit bin target/release/rhood
    cargo audit bin target/release/rhood-mcp

# Remove the target directory and all build artifacts
clean:
    cargo clean

# Generate and open the project's API documentation in a browser
docs:
    cargo doc --no-deps --all-features --open

# Generate an HTML coverage report from nextest runs and open it in a browser
coverage:
    cargo llvm-cov nextest --open --html
