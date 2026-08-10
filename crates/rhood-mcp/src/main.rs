//! `rhood-mcp` - Model Context Protocol server exposing the Robinhood API
//! as tools an LLM client can call.
//!
//! Two transports are supported: `stdio` (recommended for local agent
//! integrations like Claude Desktop, Claude Code, Cursor) and `http`
//! (for remote deployments). Authentication can be configured via OAuth
//! 2.1 with PKCE, a static bearer token, or no authentication at all,
//! controlled by the `[mcp].auth_mode` config field.
//!
//! Order placement uses MCP elicitation to obtain human Confirmation before
//! the retried `place_*` tool submits the sealed Staged Order.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod config;
mod http;
mod middleware;
mod oauth;
mod shared;
mod stdio;
mod tools;

use std::io::stderr;

use clap::Parser;
use shared::Args;

use crate::config::ServerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut config = ServerConfig::load(args.config.as_deref())?;

    tracing_subscriber::fmt()
        .with_writer(stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                let level = &config.core.log.level;
                tracing_subscriber::EnvFilter::new(format!(
                    "rhood_core={level},rhood_mcp={level},rmcp={level}"
                ))
            }),
        )
        .init();

    shared::apply_cli_overrides(&mut config, &args);

    tracing::info!(
        transport = %args.transport,
        auth_mode = %config.mcp.auth_mode,
        host = %config.mcp.host,
        port = %config.mcp.port,
        read_only = %config.core.read_only,
        "rhood-mcp starting"
    );

    match args.transport.as_str() {
        "stdio" => stdio::serve(&config).await?,
        "http" => http::serve(&config).await?,
        other => anyhow::bail!("Unknown transport: {other}"),
    }

    Ok(())
}
