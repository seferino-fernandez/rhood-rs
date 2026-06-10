//! In-memory caches for identity/metadata lookups.
//!
//! See [`ResolverCache`] for the full policy. Financial data is **never**
//! stored here — only immutable identifiers and metadata that the Robinhood
//! API returns repeatedly over the lifetime of a client.

use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use tokio::sync::OnceCell;

use crate::config::CacheConfig;
use crate::models::futures::FuturesContract;
use crate::models::stock::{IndexInstrument, Instrument};

/// Per-client caches backing symbol↔id resolution.
///
/// Contents are cloneable because all fields are reference-counted internally
/// (moka caches clone by bumping an atomic, [`OnceCell`] is shared via
/// [`Arc`]). Dropping the last clone releases the allocations.
///
/// # Policy
///
/// The cache stores only identity and metadata lookups that are effectively
/// immutable over the server lifetime:
///
/// - `symbol → Instrument` (equity metadata)
/// - `uuid → symbol` (reverse lookup for enrichment)
/// - `symbol → IndexInstrument`
/// - `symbol → FuturesContract`
/// - The singleton futures account id
///
/// Financial data — quotes, prices, candles, positions, orders, fundamentals,
/// news, ratings — is **never** stored here.
#[derive(Clone)]
pub struct ResolverCache {
    pub(crate) enabled: bool,
    pub(crate) enrichment_batch_size: usize,
    pub(crate) instruments_by_symbol: Cache<String, Arc<Instrument>>,
    pub(crate) instruments_by_id: Cache<String, String>,
    pub(crate) index_instruments: Cache<String, Arc<IndexInstrument>>,
    pub(crate) futures_contracts: Cache<String, Arc<FuturesContract>>,
    pub(crate) futures_account_id: Arc<OnceCell<String>>,
}

impl ResolverCache {
    /// Builds a cache from a configuration.
    ///
    /// The returned value is cheap to clone and share across
    /// [`RobinhoodClient`](crate::RobinhoodClient) clones: every field is
    /// internally reference-counted.
    pub fn from_config(config: &CacheConfig) -> Self {
        let instruments_by_symbol = Cache::builder()
            .max_capacity(config.instrument_max_entries)
            .time_to_live(Duration::from_secs(config.instrument_ttl_secs))
            .build();
        let instruments_by_id = Cache::builder()
            .max_capacity(config.instrument_id_max_entries)
            .time_to_live(Duration::from_secs(config.instrument_id_ttl_secs))
            .build();
        let index_instruments = Cache::builder()
            .max_capacity(config.index_max_entries)
            .time_to_live(Duration::from_secs(config.index_ttl_secs))
            .build();
        let futures_contracts = Cache::builder()
            .max_capacity(config.futures_max_entries)
            .time_to_live(Duration::from_secs(config.futures_ttl_secs))
            .build();
        Self {
            enabled: config.enabled,
            enrichment_batch_size: config.enrichment_batch_size,
            instruments_by_symbol,
            instruments_by_id,
            index_instruments,
            futures_contracts,
            futures_account_id: Arc::new(OnceCell::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn from_config_builds_empty_caches() {
        let cache = ResolverCache::from_config(&CacheConfig::default());
        assert!(cache.enabled);
        assert_eq!(cache.instruments_by_symbol.entry_count(), 0);
        assert_eq!(cache.instruments_by_id.entry_count(), 0);
        assert_eq!(cache.index_instruments.entry_count(), 0);
        assert_eq!(cache.futures_contracts.entry_count(), 0);
        assert!(cache.futures_account_id.get().is_none());
    }

    #[tokio::test]
    async fn disabled_flag_propagates() {
        let cfg = CacheConfig {
            enabled: false,
            ..CacheConfig::default()
        };
        let cache = ResolverCache::from_config(&cfg);
        assert!(!cache.enabled);
    }
}
