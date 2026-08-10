use std::sync::Arc;

use rhood_core::RobinhoodClient;
use rmcp::ServiceExt;

use crate::config::ServerConfig;
use crate::shared::authenticate_on_demand;
use crate::tools::{LazyAuthHook, RhoodTools, ToolRateLimiter};

pub async fn serve(config: &ServerConfig) -> anyhow::Result<()> {
    let rate_limiter = ToolRateLimiter::new(&config.mcp.rate_limit)?;
    let client = RobinhoodClient::with_config(config.core.clone())?;
    let config_for_hook = config.clone();
    // The hook still receives the peer even though the auth flow no longer uses
    // it: it is the handle any future in-band progress reporting would need, and
    // dropping it from `LazyAuthHook` would ripple out to every tool signature.
    let hook: LazyAuthHook = Arc::new(move |client, _peer| {
        let config = config_for_hook.clone();
        Box::pin(async move { authenticate_on_demand(&client, &config).await })
    });
    let tools = RhoodTools::new_lazy(
        client,
        hook,
        config.core.read_only,
        &config.mcp,
        rate_limiter,
    );
    let service = tools.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
