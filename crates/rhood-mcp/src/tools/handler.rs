use rhood_core::RobinhoodClient;
use rmcp::{
    Peer, RoleServer,
    handler::server::tool::ToolRouter,
    model::{RequestStateCodec, RequestStateError},
};
use sha2::{Digest, Sha256};

use crate::config::McpConfig;
use std::collections::{HashMap, hash_map::Entry};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

use uuid::Uuid;

const ORDER_CONFIRMATION_TTL: Duration = Duration::from_secs(600);
// A consumed Confirmation stays recorded for twice the sealed state's TTL so
// its replay guard cannot disappear while that state might still be valid.
const ORDER_REDEMPTION_RETENTION: Duration = Duration::from_secs(1_200);

fn process_request_state_codec() -> RequestStateCodec {
    static CODEC: OnceLock<RequestStateCodec> = OnceLock::new();
    CODEC
        .get_or_init(|| {
            let mut key = Vec::with_capacity(32);
            key.extend_from_slice(Uuid::new_v4().as_bytes());
            key.extend_from_slice(Uuid::new_v4().as_bytes());
            RequestStateCodec::new(key)
        })
        .clone()
}

fn process_order_redemptions() -> Arc<OrderRedemptionStore> {
    static REDEMPTIONS: OnceLock<Arc<OrderRedemptionStore>> = OnceLock::new();
    Arc::clone(
        REDEMPTIONS.get_or_init(|| Arc::new(OrderRedemptionStore::new(ORDER_REDEMPTION_RETENTION))),
    )
}

pub(super) struct OrderRedemptionStore {
    retention: Duration,
    redeemed: Mutex<HashMap<[u8; 32], Instant>>,
}

impl OrderRedemptionStore {
    pub(super) fn new(retention: Duration) -> Self {
        Self {
            retention,
            redeemed: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn consume(&self, request_state: &str) -> Result<(), String> {
        let digest: [u8; 32] = Sha256::digest(request_state.as_bytes()).into();
        let mut redeemed = self.redeemed.lock().map_err(|poison_error| {
            format!("Confirmation redemption store is unavailable: {poison_error}")
        })?;
        self.consume_locked(&mut redeemed, digest, Instant::now())
    }

    #[cfg(test)]
    fn consume_at(&self, request_state: &str, now: Instant) -> Result<(), String> {
        let digest: [u8; 32] = Sha256::digest(request_state.as_bytes()).into();
        let mut redeemed = self.redeemed.lock().map_err(|poison_error| {
            format!("Confirmation redemption store is unavailable: {poison_error}")
        })?;
        self.consume_locked(&mut redeemed, digest, now)
    }

    fn consume_locked(
        &self,
        redeemed: &mut HashMap<[u8; 32], Instant>,
        digest: [u8; 32],
        now: Instant,
    ) -> Result<(), String> {
        let expires_at = now.checked_add(self.retention).ok_or_else(|| {
            "Confirmation redemption retention exceeds the server clock range".to_string()
        })?;
        redeemed.retain(|_digest, recorded_expiry| *recorded_expiry > now);
        match redeemed.entry(digest) {
            Entry::Occupied(_entry) => Err(
                "Confirmation was already used; stage a fresh order before submitting again"
                    .to_string(),
            ),
            Entry::Vacant(entry) => {
                entry.insert(expires_at);
                Ok(())
            }
        }
    }
}

/// Boxed future returned by a [`LazyAuthHook`].
///
/// Authentication is IO-bound and dynamically dispatched, so the hook must
/// erase the concrete future type. `Send` lets the future be awaited across
/// `tokio::spawn` boundaries if callers choose to.
type LazyAuthFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Hook invoked on the first tool call when the client is not yet
/// authenticated.
///
/// Receives a clone of the shared [`RobinhoodClient`] (internally an
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
/// Wraps a shared [`RobinhoodClient`] and a per-process request-state codec.
/// Supports read-only mode that hides write tools.
///
/// The client is a plain value that derives `Clone` internally over
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
    /// OAuth grant - duplicate device-verification challenges on the second
    /// caller. The guard is acquired only when the client is unauthenticated;
    /// already-authenticated fast-path skips it entirely. On login failure the
    /// guard releases so the next caller can retry.
    lazy_auth_gate: Arc<AsyncMutex<()>>,
    pub tool_router: ToolRouter<Self>,
    pub(super) request_state_codec: RequestStateCodec,
    pub(super) order_redemptions: Arc<OrderRedemptionStore>,
    pub(super) order_confirmation_ttl: Duration,
    pub(super) read_only: bool,
    /// Ceiling on the first-call lazy-auth wait, so a stuck login can't hold
    /// `lazy_auth_gate` and wedge every other first call.
    auth_timeout: Duration,
    /// Ceiling on a single tool response payload, in bytes. Oversized responses
    /// are replaced with a bounded JSON error in `call_tool`.
    pub(super) max_response_bytes: usize,
}

impl RhoodTools {
    /// Creates a new `RhoodTools` with an already-authenticated client.
    ///
    /// Used by the HTTP transport, which authenticates once at startup and
    /// shares the resulting client across sessions.
    pub fn new_eager(client: RobinhoodClient, read_only: bool, mcp_config: &McpConfig) -> Self {
        Self {
            client,
            lazy_auth_hook: None,
            lazy_auth_gate: Arc::new(AsyncMutex::new(())),
            tool_router: Self::combined_router(),
            request_state_codec: process_request_state_codec(),
            order_redemptions: process_order_redemptions(),
            order_confirmation_ttl: ORDER_CONFIRMATION_TTL,
            read_only,
            auth_timeout: Duration::from_secs(mcp_config.lazy_auth_timeout_secs),
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
            lazy_auth_gate: Arc::new(AsyncMutex::new(())),
            tool_router: Self::combined_router(),
            request_state_codec: process_request_state_codec(),
            order_redemptions: process_order_redemptions(),
            order_confirmation_ttl: ORDER_CONFIRMATION_TTL,
            read_only,
            auth_timeout: Duration::from_secs(mcp_config.lazy_auth_timeout_secs),
            max_response_bytes: mcp_config.max_response_bytes,
        }
    }

    pub(super) fn request_state_error(error: &RequestStateError) -> String {
        match error {
            RequestStateError::Expired => {
                "Confirmation expired; stage the order again before submitting".to_string()
            }
            _ => "Confirmation is no longer valid; stage the order again (the state may have been altered or belongs to a different Principal)".to_string(),
        }
    }

    /// Combines all domain-specific tool routers into one.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the operator is a router-combining Add impl, not numeric addition"
    )]
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
#[expect(
    clippy::assertions_on_result_states,
    reason = "these tests deliberately assert successful authentication results without unwrapping them"
)]
mod tests {
    use super::{
        ORDER_CONFIRMATION_TTL, ORDER_REDEMPTION_RETENTION, OrderRedemptionStore,
        run_auth_with_timeout,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn redemption_pruning_removes_expired_record_without_affecting_live_one() -> Result<(), String>
    {
        if ORDER_REDEMPTION_RETENTION <= ORDER_CONFIRMATION_TTL {
            return Err("redemption retention must exceed the sealed-state TTL".to_string());
        }

        let retention = Duration::from_secs(10);
        let store = OrderRedemptionStore::new(retention);
        let started_at = Instant::now();
        let live_recorded_at = started_at
            .checked_add(Duration::from_secs(5))
            .ok_or_else(|| "test clock cannot represent live record time".to_string())?;
        let prune_at = started_at
            .checked_add(Duration::from_secs(11))
            .ok_or_else(|| "test clock cannot represent pruning time".to_string())?;

        store.consume_at("expired-state", started_at)?;
        store.consume_at("live-state", live_recorded_at)?;
        store.consume_at("pruning-trigger", prune_at)?;

        let record_count = store
            .redeemed
            .lock()
            .map_err(|poison_error| format!("test redemption store poisoned: {poison_error}"))?
            .len();
        if record_count != 2 {
            return Err(format!(
                "pruning should leave the live record and trigger; found {record_count} records"
            ));
        }
        store.consume_at("expired-state", prune_at)?;
        let Err(live_error) = store.consume_at("live-state", prune_at) else {
            return Err("live redemption record was incorrectly pruned".to_string());
        };
        if !live_error.contains("already used") {
            return Err(format!(
                "live record returned the wrong error: {live_error}"
            ));
        }
        Ok(())
    }

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
