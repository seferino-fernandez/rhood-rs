//! Helpers that splice ticker symbols into MCP tool responses.
//!
//! Robinhood's wire payloads reference instruments by URL (orders, positions,
//! tags, news) or not at all (fundamentals). Agents consuming these
//! responses shouldn't have to do N extra resolution calls; instead we
//! batch-resolve server-side via [`RobinhoodClient::resolve_symbols`] and
//! emit an enriched JSON payload with `symbol` / `related_symbols` /
//! `instrument_symbols` fields attached.
//!
//! Enrichment is best-effort: a batch lookup failure logs a warning and
//! returns the unenriched payload rather than aborting the tool call.

use rhood_core::RobinhoodClient;
use rhood_core::models::futures::FuturesQuote;
use rhood_core::models::research::{NewsArticle, StockSplit, TagResult};
use rhood_core::models::stock::Fundamentals;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Pulls the UUID from a Robinhood instrument URL.
///
/// Robinhood formats instrument URLs like
/// `https://api.robinhood.com/instruments/abc12345-.../`. Returns `None` on
/// any input that doesn't end in a non-empty trailing segment.
pub(super) fn extract_uuid_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next()?;
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

/// Calls [`RobinhoodClient::resolve_symbols`] with best-effort semantics.
///
/// On upstream error, logs a warning and returns an empty map so callers
/// can fall back to unenriched output.
pub(super) async fn safe_resolve_symbols(
    client: &RobinhoodClient,
    ids: &[String],
) -> HashMap<String, String> {
    if ids.is_empty() {
        return HashMap::new();
    }
    match client.resolve_symbols(ids).await {
        Ok(map) => map,
        Err(error) => {
            tracing::warn!(
                error = %error,
                count = ids.len(),
                "resolve_symbols failed; returning unenriched payload"
            );
            HashMap::new()
        }
    }
}

/// Extracts distinct UUIDs from an iterator of optional instrument URLs.
pub(super) fn collect_uuids<'a>(urls: impl IntoIterator<Item = &'a Option<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for url in urls.into_iter().flatten() {
        if let Some(uuid) = extract_uuid_from_url(url)
            && seen.insert(uuid.clone())
        {
            ordered.push(uuid);
        }
    }
    ordered
}

/// Decorated `NewsArticle` with an `instrument_symbols` list mirroring the
/// `related_instruments` URL list.
#[derive(Serialize)]
pub(super) struct EnrichedNewsArticle<'a> {
    #[serde(flatten)]
    pub(super) article: &'a NewsArticle,
    pub(super) related_symbols: Vec<String>,
}

/// Decorated `TagResult` with an `instrument_symbols` list mirroring the
/// `instruments` URL list.
#[derive(Serialize)]
pub(super) struct EnrichedTagResult<'a> {
    #[serde(flatten)]
    pub(super) tag: &'a TagResult,
    pub(super) instrument_symbols: Vec<String>,
}

/// Decorated `StockSplit` with a resolved `symbol`.
#[derive(Serialize)]
pub(super) struct EnrichedSplit<'a> {
    #[serde(flatten)]
    pub(super) split: &'a StockSplit,
    pub(super) symbol: Option<String>,
}

/// Resolves a list of instrument URLs to symbols via a single batch call.
///
/// Returns a parallel `Vec<Option<String>>` matching the input order;
/// entries that resolve to a symbol are `Some("AAPL")`, misses are `None`.
pub(super) async fn resolve_urls_to_symbols(
    client: &RobinhoodClient,
    urls: &[Option<String>],
) -> Vec<Option<String>> {
    let uuids = collect_uuids(urls.iter());
    let resolved = safe_resolve_symbols(client, &uuids).await;
    urls.iter()
        .map(|maybe_url| {
            maybe_url
                .as_deref()
                .and_then(extract_uuid_from_url)
                .and_then(|uuid| resolved.get(&uuid).cloned())
        })
        .collect()
}

/// Models that carry an `Option<String>` ticker `symbol` we can backfill in
/// place, producing exactly one `symbol` JSON key (no `#[serde(flatten)]`
/// collision).
pub(super) trait HasSymbol {
    fn symbol_slot(&mut self) -> &mut Option<String>;
}

impl HasSymbol for rhood_core::models::account::Position {
    fn symbol_slot(&mut self) -> &mut Option<String> {
        &mut self.symbol
    }
}

impl HasSymbol for rhood_core::models::dividend::Dividend {
    fn symbol_slot(&mut self) -> &mut Option<String> {
        &mut self.symbol
    }
}

impl HasSymbol for rhood_core::models::order::StockOrder {
    fn symbol_slot(&mut self) -> &mut Option<String> {
        &mut self.symbol
    }
}

/// Backfills each record's `symbol` from the parallel `resolved` list.
///
/// A `Some(sym)` entry overwrites the record's `symbol`; a `None` entry leaves
/// the wire value untouched (so a resolution miss never clobbers a symbol the
/// payload already carried). Operates on owned records so the result serializes
/// with a single `symbol` key.
pub(super) fn apply_resolved_symbols<T: HasSymbol>(
    mut records: Vec<T>,
    resolved: &[Option<String>],
) -> Vec<T> {
    for (record, sym) in records.iter_mut().zip(resolved.iter()) {
        if let Some(symbol) = sym {
            *record.symbol_slot() = Some(symbol.clone());
        }
    }
    records
}

/// A futures quote with the caller's requested alias echoed under a distinct
/// `requested_symbol` key.
///
/// The flattened `FuturesQuote` keeps its authoritative wire `symbol` (e.g.
/// `"/ESM26:XCME"`); `requested_symbol` is the alias the caller passed (e.g.
/// `"ESM26"`). Because the added key is `requested_symbol`, never `symbol`,
/// there is no duplicate-key collision.
#[derive(Serialize)]
pub(super) struct FuturesQuoteView {
    #[serde(flatten)]
    pub(super) quote: FuturesQuote,
    pub(super) requested_symbol: String,
}

/// Wraps each owned `FuturesQuote` with its requested alias, backfilling the
/// wire `symbol` from the alias only when the upstream payload omitted it.
pub(super) fn enrich_futures_quotes(
    quotes: Vec<FuturesQuote>,
    requested: &[String],
) -> Vec<FuturesQuoteView> {
    quotes
        .into_iter()
        .zip(requested.iter())
        .map(|(mut quote, req)| {
            if quote.symbol.is_none() {
                quote.symbol = Some(req.clone());
            }
            FuturesQuoteView {
                quote,
                requested_symbol: req.clone(),
            }
        })
        .collect()
}

/// Returns owned `Fundamentals` records with `symbol` backfilled from the
/// request symbols when the wire payload omitted it. Produces exactly one
/// `symbol` field per record (no duplicate JSON key).
pub(super) fn enrich_fundamentals(
    records: &[Fundamentals],
    request_symbols: &[String],
) -> Vec<Fundamentals> {
    records
        .iter()
        .zip(request_symbols.iter())
        .map(|(record, requested)| {
            let mut owned = record.clone();
            if owned.symbol.is_none() {
                owned.symbol = Some(requested.clone());
            }
            owned
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_uuid_from_full_url() {
        let url = "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/";
        assert_eq!(
            extract_uuid_from_url(url).as_deref(),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
    }

    #[test]
    fn extract_uuid_without_trailing_slash() {
        let url = "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        assert_eq!(
            extract_uuid_from_url(url).as_deref(),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
    }

    #[test]
    fn extract_uuid_from_empty_path_returns_none() {
        assert!(extract_uuid_from_url("/").is_none());
        assert!(extract_uuid_from_url("").is_none());
    }

    #[test]
    fn collect_uuids_dedups_and_preserves_order() {
        let inputs = [
            Some("https://api.robinhood.com/instruments/aaa/".to_string()),
            Some("https://api.robinhood.com/instruments/bbb/".to_string()),
            Some("https://api.robinhood.com/instruments/aaa/".to_string()),
            None,
        ];
        let refs: Vec<&Option<String>> = inputs.iter().collect();
        let uuids = collect_uuids(refs);
        assert_eq!(uuids, vec!["aaa".to_string(), "bbb".to_string()]);
    }

    #[test]
    fn enrich_fundamentals_has_single_symbol_key() {
        let record = Fundamentals {
            symbol: None,
            ..Default::default()
        };
        let out = enrich_fundamentals(std::slice::from_ref(&record), &["AAPL".to_string()]);
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(json.matches("\"symbol\"").count(), 1);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "AAPL");
    }

    #[test]
    fn enrich_fundamentals_keeps_existing_symbol() {
        let record = Fundamentals {
            symbol: Some("TSLA".into()),
            ..Default::default()
        };
        let out = enrich_fundamentals(std::slice::from_ref(&record), &["tsla".to_string()]);
        assert_eq!(out[0].symbol.as_deref(), Some("TSLA"));
    }

    #[test]
    fn apply_resolved_overwrites_and_keeps_single_symbol_key() {
        use rhood_core::models::account::Position;
        let position: Position = serde_json::from_str(
            r#"{"instrument":"https://api.robinhood.com/instruments/abc/","symbol":"RKLB"}"#,
        )
        .unwrap();
        // Resolution miss (None) must NOT replace the wire symbol, and must not
        // add a second `symbol` key.
        let out = apply_resolved_symbols(vec![position], &[None]);
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "dup symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "RKLB");
    }

    #[test]
    fn apply_resolved_sets_symbol_when_resolved() {
        use rhood_core::models::account::Position;
        let position: Position = serde_json::from_str(
            r#"{"instrument":"https://api.robinhood.com/instruments/abc/","symbol":null}"#,
        )
        .unwrap();
        let out = apply_resolved_symbols(vec![position], &[Some("AAPL".to_string())]);
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "dup symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "AAPL");
    }

    #[test]
    fn enriched_split_carries_symbol() {
        use rhood_core::models::research::StockSplit;
        let split = StockSplit {
            instrument: Some("https://api.robinhood.com/instruments/abc/".into()),
            ..Default::default()
        };
        let decorated = EnrichedSplit {
            split: &split,
            symbol: Some("AAPL".into()),
        };
        let json = serde_json::to_value(&decorated).unwrap();
        assert_eq!(json["symbol"], "AAPL");
    }

    #[test]
    fn futures_view_has_single_symbol_and_requested_symbol() {
        use rhood_core::models::futures::FuturesQuote;
        // Wire symbol present and DIFFERENT from the requested alias.
        let quote: FuturesQuote = serde_json::from_str(r#"{"symbol":"/ESM26:XCME"}"#).unwrap();
        let out = enrich_futures_quotes(vec![quote], &["ESM26".to_string()]);
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "dup symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "/ESM26:XCME");
        assert_eq!(value[0]["requested_symbol"], "ESM26");
    }

    #[test]
    fn futures_view_backfills_symbol_when_wire_missing() {
        use rhood_core::models::futures::FuturesQuote;
        let quote: FuturesQuote = serde_json::from_str(r#"{"symbol":null}"#).unwrap();
        let out = enrich_futures_quotes(vec![quote], &["ESM26".to_string()]);
        let value: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(value[0]["symbol"], "ESM26");
        assert_eq!(value[0]["requested_symbol"], "ESM26");
    }
}
