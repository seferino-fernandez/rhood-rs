use rhood_core::RobinhoodClient;
use rmcp::model::LoggingLevel;
use rmcp::{Peer, RoleServer, handler::server::tool::ToolRouter};

use crate::config::McpConfig;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use super::types::PendingOrder;

/// Boxed future returned by a [`LazyAuthHook`].
///
/// Authentication is IO-bound and dynamically dispatched, so the hook must
/// erase the concrete future type. `Send` lets the future be awaited across
/// `tokio::spawn` boundaries if callers choose to.
type LazyAuthFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Hook invoked on the first tool call when the client is not yet
/// authenticated.
///
/// Receives a clone of the shared [`RobinhoodClient`] (cheap — internally an
/// `Arc<RwLock<AuthState>>`) and the MCP peer so the hook can drive the login
/// flow while sending progress notifications back to the client.
///
/// Only set for the stdio transport, which defers authentication to keep the
/// MCP handshake snappy. HTTP transport authenticates eagerly at startup and
/// sets this to `None`.
pub type LazyAuthHook =
    Arc<dyn Fn(RobinhoodClient, Peer<RoleServer>) -> LazyAuthFuture + Send + Sync>;

/// MCP tool handler exposing Robinhood trading operations via the rmcp framework.
///
/// Wraps a shared [`RobinhoodClient`] and holds staged pending-order state.
/// Supports read-only mode that hides write tools.
///
/// The client is a plain value — it derives `Clone` internally over
/// `Arc`-wrapped state, so cloning `RhoodTools` across concurrent tool
/// invocations is cheap and does not serialize tool calls.
#[derive(Clone)]
pub struct RhoodTools {
    client: RobinhoodClient,
    lazy_auth_hook: Option<LazyAuthHook>,
    /// Serializes concurrent first-call logins on the lazy path.
    ///
    /// Without this, two tool calls arriving before the first login resolves
    /// can both see `is_authenticated() == false` and each trigger a full
    /// OAuth grant — duplicate device-verification challenges on the second
    /// caller. The guard is acquired only when the client is unauthenticated;
    /// already-authenticated fast-path skips it entirely. On login failure the
    /// guard releases so the next caller can retry.
    lazy_auth_gate: Arc<Mutex<()>>,
    pub tool_router: ToolRouter<Self>,
    pub(super) pending_orders: Arc<Mutex<HashMap<String, PendingOrder>>>,
    pub(super) read_only: bool,
    /// Ceiling on the first-call lazy-auth wait, so a stuck login can't hold
    /// `lazy_auth_gate` and wedge every other first call.
    auth_timeout: Duration,
    pub(super) min_log_level: Arc<tokio::sync::RwLock<LoggingLevel>>,
    /// Ceiling on a single tool response payload, in bytes. Oversized responses
    /// are replaced with a bounded JSON error in `call_tool`.
    pub(super) max_response_bytes: usize,
}

impl RhoodTools {
    /// Creates a new `RhoodTools` with an already-authenticated client.
    ///
    /// Used by the HTTP transport, which authenticates once at startup and
    /// shares the resulting client across sessions. The provided pending-order
    /// maps are shared with other `RhoodTools` instances spawned by the same
    /// HTTP service factory.
    pub fn new_eager(
        client: RobinhoodClient,
        read_only: bool,
        mcp_config: &McpConfig,
        pending_orders: Arc<Mutex<HashMap<String, PendingOrder>>>,
    ) -> Self {
        Self {
            client,
            lazy_auth_hook: None,
            lazy_auth_gate: Arc::new(Mutex::new(())),
            tool_router: Self::combined_router(),
            pending_orders,
            read_only,
            auth_timeout: Duration::from_secs(mcp_config.lazy_auth_timeout_secs),
            min_log_level: Arc::new(tokio::sync::RwLock::new(LoggingLevel::Info)),
            max_response_bytes: mcp_config.max_response_bytes,
        }
    }

    /// Creates a new `RhoodTools` that authenticates on the first tool call.
    ///
    /// Used by the stdio transport: the client is constructed eagerly (no
    /// network calls) but authentication is deferred until the first tool
    /// invocation so the MCP handshake completes instantly. The `lazy_auth_hook`
    /// receives the client and peer, drives the login flow, and sends progress
    /// notifications to the connected MCP client.
    pub fn new_lazy(
        client: RobinhoodClient,
        lazy_auth_hook: LazyAuthHook,
        read_only: bool,
        mcp_config: &McpConfig,
    ) -> Self {
        Self {
            client,
            lazy_auth_hook: Some(lazy_auth_hook),
            lazy_auth_gate: Arc::new(Mutex::new(())),
            tool_router: Self::combined_router(),
            pending_orders: Arc::new(Mutex::new(HashMap::new())),
            read_only,
            auth_timeout: Duration::from_secs(mcp_config.lazy_auth_timeout_secs),
            min_log_level: Arc::new(tokio::sync::RwLock::new(LoggingLevel::Info)),
            max_response_bytes: mcp_config.max_response_bytes,
        }
    }

    /// Combines all domain-specific tool routers into one.
    fn combined_router() -> ToolRouter<Self> {
        Self::stock_router()
            + Self::option_router()
            + Self::order_router()
            + Self::account_router()
            + Self::income_router()
            + Self::market_router()
            + Self::futures_router()
            + Self::index_router()
            + Self::recurring_router()
            + Self::research_router()
            + Self::user_router()
            + Self::watchlist_router()
    }

    /// Returns an authenticated [`RobinhoodClient`], triggering deferred login
    /// via the lazy-auth hook if the client is not yet authenticated.
    ///
    /// When a lazy-auth hook is configured (stdio transport), the hook runs
    /// only once: subsequent calls observe an already-authenticated client and
    /// skip the hook entirely. When no hook is configured (HTTP transport),
    /// the client is assumed to be authenticated at startup and is returned
    /// directly.
    ///
    /// The returned client shares its auth state with `self.client` through
    /// internal `Arc`s, so auth updates performed by the hook are immediately
    /// visible to all subsequent callers.
    pub(super) async fn ensure_client(
        &self,
        peer: &Peer<RoleServer>,
    ) -> Result<RobinhoodClient, String> {
        if self.client.is_authenticated().await {
            return Ok(self.client.clone());
        }
        if let Some(hook) = &self.lazy_auth_hook {
            // Serialize concurrent first-call logins. Double-check inside the
            // guard so only the first winner actually invokes the hook; the
            // others observe the authenticated state and skip it. On failure
            // the guard releases and the next caller can retry.
            let _guard = self.lazy_auth_gate.lock().await;
            if !self.client.is_authenticated().await {
                run_auth_with_timeout(hook(self.client.clone(), peer.clone()), self.auth_timeout)
                    .await?;
            }
        }
        Ok(self.client.clone())
    }
}

/// Awaits an auth future, failing with a clear error if it exceeds `timeout`.
///
/// Bounds the first-call login so a stuck device-verification wait cannot hold
/// the auth gate indefinitely and stall every other tool's first call.
async fn run_auth_with_timeout(
    auth: impl Future<Output = Result<(), String>>,
    timeout: Duration,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, auth).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "authentication timed out after {}s; check device-approval prompts and retry",
            timeout.as_secs()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::run_auth_with_timeout;
    use std::time::Duration;

    #[tokio::test]
    async fn auth_times_out_when_hook_stalls() {
        let stalled = async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(())
        };
        let result = run_auth_with_timeout(stalled, Duration::from_millis(20)).await;
        assert!(result.is_err(), "a stalled login must time out");
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[tokio::test]
    async fn auth_passes_through_quick_success() {
        let quick = async { Ok(()) };
        let result = run_auth_with_timeout(quick, Duration::from_secs(5)).await;
        assert!(result.is_ok());
    }
}
