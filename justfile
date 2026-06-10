default:
    @just --list

# Run all tests: full feature matrix, default features, and doctests
test:
	cargo nextest run --all-targets --all-features
	cargo nextest run
	cargo test --doc

# Run live-API integration tests against Robinhood (requires credentials)
integration:
	cargo test --test integration -p rhood-cli -- --ignored --nocapture

# Lint the code with Clippy
lint:
    cargo clippy

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
    cargo doc --open

# Generate an HTML coverage report from nextest runs and open it in a browser
coverage:
    cargo llvm-cov nextest --open --html
