use std::sync::Arc;

use rhood_core::RobinhoodClient;
use rmcp::ServiceExt;

use crate::config::ServerConfig;
use crate::shared::authenticate_on_demand;
use crate::tools::{LazyAuthHook, RhoodTools};

pub async fn serve(config: &ServerConfig) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config.core.clone())?;
    let config_for_hook = config.clone();
    let hook: LazyAuthHook = Arc::new(move |client, peer| {
        let config = config_for_hook.clone();
        Box::pin(async move { authenticate_on_demand(&client, &config, &peer).await })
    });
    let tools = RhoodTools::new_lazy(client, hook, config.core.read_only, &config.mcp);
    let service = tools.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
