//! Core-owned Instrument Identity Enrichment.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use super::RobinhoodClient;
use crate::api::paths;
use crate::models::account::Position;
use crate::models::dividend::Dividend;
use crate::models::order::StockOrder;
use crate::models::research::StockSplit;
use crate::models::stock::Instrument;
use crate::resolver_cache::ResolverCache;
use crate::{Result, RhoodError};

#[derive(Clone)]
pub(super) struct InstrumentIdentityResolver {
    cache: ResolverCache,
}

#[derive(Deserialize)]
struct InstrumentIdentityResponse {
    #[serde(default)]
    results: Vec<Option<InstrumentIdentityRow>>,
}

#[derive(Deserialize)]
struct InstrumentIdentityRow {
    id: Option<String>,
    symbol: Option<String>,
}

#[derive(Default)]
struct ErrorClasses {
    first: Option<&'static str>,
    mixed: bool,
}

impl ErrorClasses {
    fn add(&mut self, class: &'static str) {
        match self.first {
            None => self.first = Some(class),
            Some(first) if first != class => self.mixed = true,
            Some(_) => {}
        }
    }

    fn label(&self) -> &'static str {
        if self.mixed {
            "mixed"
        } else {
            self.first.unwrap_or("upstream_miss")
        }
    }
}

pub(crate) trait InstrumentIdentityRecord {
    fn instrument_reference(&self) -> Option<&str>;
    fn has_wire_symbol(&self) -> bool;
    fn fill_symbol(&mut self, symbol: String);
}

macro_rules! identity_record {
    ($record:ty) => {
        impl InstrumentIdentityRecord for $record {
            fn instrument_reference(&self) -> Option<&str> {
                self.instrument.as_deref()
            }

            fn has_wire_symbol(&self) -> bool {
                self.symbol.is_some()
            }

            fn fill_symbol(&mut self, symbol: String) {
                if self.symbol.is_none() {
                    self.symbol = Some(symbol);
                }
            }
        }
    };
}

identity_record!(Position);
identity_record!(Dividend);
identity_record!(StockOrder);
identity_record!(StockSplit);

impl InstrumentIdentityResolver {
    pub(super) fn new(cache: ResolverCache) -> Self {
        Self { cache }
    }

    pub(super) async fn resolve_strict(
        &self,
        client: &RobinhoodClient,
        references: &[String],
    ) -> Result<Vec<Option<String>>> {
        let parsed = validate_all(references)?;
        let symbols = self.resolve_valid_strict(client, &parsed).await?;
        Ok(parsed.iter().map(|id| symbols.get(id).cloned()).collect())
    }

    pub(super) async fn resolve_best_effort(
        &self,
        client: &RobinhoodClient,
        operation: &'static str,
        references: &[String],
    ) -> Vec<Option<String>> {
        if references.is_empty() {
            return Vec::new();
        }

        let mut errors = ErrorClasses::default();
        let parsed = references
            .iter()
            .map(|reference| match canonical_instrument_id(reference) {
                Ok(id) => Some(id),
                Err(_) => {
                    errors.add("invalid_reference");
                    None
                }
            })
            .collect::<Vec<_>>();
        let valid = parsed.iter().flatten().copied().collect::<Vec<_>>();
        let mut symbols = HashMap::new();
        let misses = self
            .cache_misses(&stable_unique(&valid), &mut symbols)
            .await;

        for chunk in misses.chunks(self.cache.enrichment_batch_size.max(1)) {
            match self.fetch_chunk(client, chunk).await {
                Ok(resolved) => {
                    if chunk.iter().any(|id| !resolved.contains_key(id)) {
                        errors.add("upstream_miss");
                    }
                    self.store_resolved(&resolved).await;
                    symbols.extend(resolved);
                }
                Err(error) => errors.add(error_class(&error)),
            }
        }

        let output = parsed
            .iter()
            .map(|id| id.and_then(|id| symbols.get(&id).cloned()))
            .collect::<Vec<_>>();
        let resolved = output.iter().filter(|symbol| symbol.is_some()).count();
        let unresolved = references.len().saturating_sub(resolved);
        if unresolved > 0 {
            tracing::warn!(
                operation,
                requested = references.len(),
                resolved,
                unresolved,
                error_class = errors.label(),
                "instrument identity enrichment incomplete"
            );
        }
        output
    }

    pub(super) async fn enrich_records<T: InstrumentIdentityRecord>(
        &self,
        client: &RobinhoodClient,
        operation: &'static str,
        records: &mut [T],
    ) {
        let requested = records
            .iter()
            .enumerate()
            .filter(|(_, record)| !record.has_wire_symbol())
            .filter_map(|(index, record)| {
                record
                    .instrument_reference()
                    .map(|reference| (index, reference.to_owned()))
            })
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return;
        }
        let references = requested
            .iter()
            .map(|(_, reference)| reference.clone())
            .collect::<Vec<_>>();
        let symbols = self
            .resolve_best_effort(client, operation, &references)
            .await;
        for ((index, _), symbol) in requested.into_iter().zip(symbols) {
            if let (Some(record), Some(symbol)) = (records.get_mut(index), symbol) {
                record.fill_symbol(symbol);
            }
        }
    }

    pub(super) async fn instrument_for_symbol(
        &self,
        client: &RobinhoodClient,
        symbol: &str,
    ) -> Result<Option<Arc<Instrument>>> {
        let key = symbol.to_uppercase();
        if self.cache.enabled
            && let Some(hit) = self.cache.instruments_by_symbol.get(&key).await
        {
            return Ok(Some(hit));
        }
        let Some(instrument) = client.get_instrument_by_symbol(&key).await? else {
            return Ok(None);
        };
        let wrapped = Arc::new(instrument);
        if self.cache.enabled {
            self.cache
                .instruments_by_symbol
                .insert(key.clone(), wrapped.clone())
                .await;
            if let Some(id) = wrapped
                .id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
            {
                self.cache
                    .instruments_by_id
                    .insert(id.to_string(), key)
                    .await;
            }
        }
        Ok(Some(wrapped))
    }

    async fn resolve_valid_strict(
        &self,
        client: &RobinhoodClient,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, String>> {
        let mut symbols = HashMap::new();
        let misses = self.cache_misses(&stable_unique(ids), &mut symbols).await;
        let chunks = misses
            .chunks(self.cache.enrichment_batch_size.max(1))
            .collect::<Vec<_>>();
        let total_chunks = chunks.len();
        for (index, chunk) in chunks.into_iter().enumerate() {
            let resolved = self.fetch_chunk(client, chunk).await.map_err(|source| {
                RhoodError::RequestChunkFailed {
                    endpoint: "instrument identity".to_string(),
                    chunk_index: index.saturating_add(1),
                    total_chunks,
                    source: Box::new(source),
                }
            })?;
            self.store_resolved(&resolved).await;
            symbols.extend(resolved);
        }
        Ok(symbols)
    }

    async fn cache_misses(&self, ids: &[Uuid], symbols: &mut HashMap<Uuid, String>) -> Vec<Uuid> {
        if !self.cache.enabled {
            return ids.to_vec();
        }
        let mut misses = Vec::new();
        for id in ids {
            if let Some(symbol) = self.cache.instruments_by_id.get(&id.to_string()).await {
                symbols.insert(*id, symbol);
            } else {
                misses.push(*id);
            }
        }
        misses
    }

    async fn fetch_chunk(
        &self,
        client: &RobinhoodClient,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, String>> {
        let joined = ids
            .iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let params = [("ids", joined.as_str())];
        let response: InstrumentIdentityResponse = client
            .get_with_params(&client.api_url(paths::INSTRUMENTS), &params)
            .await?;
        let requested = ids.iter().copied().collect::<HashSet<_>>();
        let mut resolved = HashMap::new();
        for row in response.results.into_iter().flatten() {
            let Some(id) = row.id.and_then(|id| Uuid::parse_str(&id).ok()) else {
                continue;
            };
            let Some(symbol) = row
                .symbol
                .map(|symbol| symbol.trim().to_owned())
                .filter(|symbol| !symbol.is_empty())
            else {
                continue;
            };
            if requested.contains(&id) {
                resolved.entry(id).or_insert(symbol);
            }
        }
        Ok(resolved)
    }

    async fn store_resolved(&self, resolved: &HashMap<Uuid, String>) {
        if !self.cache.enabled {
            return;
        }
        for (id, symbol) in resolved {
            self.cache
                .instruments_by_id
                .insert(id.to_string(), symbol.clone())
                .await;
        }
    }
}

fn validate_all(references: &[String]) -> Result<Vec<Uuid>> {
    let parsed = references
        .iter()
        .map(|reference| canonical_instrument_id(reference))
        .collect::<Vec<_>>();
    if let Some(error) = parsed.iter().find_map(|result| result.as_ref().err()) {
        return Err(RhoodError::InvalidParameter(error.clone()));
    }
    Ok(parsed
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect())
}

pub(crate) fn canonical_instrument_id(reference: &str) -> std::result::Result<Uuid, String> {
    if let Ok(id) = parse_bare_instrument_id(reference) {
        return Ok(id);
    }

    let url = reqwest::Url::parse(reference).map_err(
        |_parse_error| "instrument reference must be a UUID or absolute HTTP(S) instrument URL",
    )?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "instrument reference must be an absolute HTTP(S) URL without query or fragment"
                .to_string(),
        );
    }
    let authority_and_path = reference
        .split_once("://")
        .map(|(_, remainder)| remainder)
        .ok_or_else(|| "instrument reference must be an absolute HTTP(S) URL".to_string())?;
    let raw_path = authority_and_path
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or("");
    let path = raw_path.strip_suffix('/').unwrap_or(raw_path);
    let Some(id) = path.strip_prefix("instruments/") else {
        return Err("instrument URL path must be /instruments/{uuid}/".to_string());
    };
    if id.is_empty() || id.contains('/') {
        return Err("instrument URL path must contain exactly one instrument UUID".to_string());
    }
    parse_bare_instrument_id(id)
}

fn parse_bare_instrument_id(reference: &str) -> std::result::Result<Uuid, String> {
    let id = Uuid::parse_str(reference)
        .map_err(|_parse_error| "instrument reference contains a malformed UUID".to_string())?;
    if reference.len() != 36 || !reference.eq_ignore_ascii_case(&id.hyphenated().to_string()) {
        return Err(
            "instrument reference UUID must use the 36-character hyphenated form".to_string(),
        );
    }
    Ok(id)
}

fn stable_unique(ids: &[Uuid]) -> Vec<Uuid> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

fn error_class(error: &RhoodError) -> &'static str {
    match error {
        RhoodError::RequestChunkFailed { source, .. } => error_class(source),
        RhoodError::Http(_) => "transport",
        RhoodError::Json(_) => "decoding",
        RhoodError::RateLimited { .. } => "rate_limit",
        RhoodError::Api { .. } => "upstream",
        RhoodError::NotAuthenticated | RhoodError::TokenExpired => "authentication",
        RhoodError::Timeout(_) => "timeout",
        _ => "resolution",
    }
}

impl RobinhoodClient {
    /// Resolves Instrument References to positionally aligned Ticker Symbols.
    ///
    /// Every reference is validated before HTTP. A valid reference that is not
    /// present in the upstream response produces `None`; transport or decoding
    /// failure aborts the call.
    pub async fn resolve_instrument_symbols(
        &self,
        references: &[String],
    ) -> Result<Vec<Option<String>>> {
        self.instrument_identity
            .resolve_strict(self, references)
            .await
    }

    pub(crate) async fn resolve_instrument_symbols_best_effort(
        &self,
        operation: &'static str,
        references: &[String],
    ) -> Vec<Option<String>> {
        self.instrument_identity
            .resolve_best_effort(self, operation, references)
            .await
    }

    pub(crate) async fn enrich_instrument_identities<T: InstrumentIdentityRecord>(
        &self,
        operation: &'static str,
        records: &mut [T],
    ) {
        self.instrument_identity
            .enrich_records(self, operation, records)
            .await;
    }

    pub(crate) async fn instrument_for_symbol(
        &self,
        symbol: &str,
    ) -> Result<Option<Arc<Instrument>>> {
        self.instrument_identity
            .instrument_for_symbol(self, symbol)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RhoodConfig;
    use secrecy::SecretString;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ID_A: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
    const ID_B: &str = "18226051-6bfa-4c56-bd9a-d7575f0245c1";
    const ID_C: &str = "3f3c5b7a-8f73-4c69-a6b7-cb4f06a95411";

    #[derive(Clone, Default)]
    struct WarningCapture {
        events: Arc<Mutex<Vec<HashMap<String, String>>>>,
    }

    impl Subscriber for WarningCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            *metadata.level() == tracing::Level::WARN
                && metadata.target().ends_with("instrument_identity")
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut fields = HashMap::new();
            event.record(&mut FieldCapture(&mut fields));
            self.events.lock().unwrap().push(fields);
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    struct FieldCapture<'a>(&'a mut HashMap<String, String>);

    impl Visit for FieldCapture<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0
                .insert(field.name().to_string(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().to_string(), value.to_string());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.insert(field.name().to_string(), value.to_string());
        }
    }

    async fn client_for_server(
        server: &MockServer,
        cache_enabled: bool,
        batch_size: usize,
    ) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_string_lossy()
            .into_owned();
        config.api.base_url = server.uri();
        config.cache.enabled = cache_enabled;
        config.cache.enrichment_batch_size = batch_size;
        config.http.read_request_interval_ms = 1;
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .inject_test_auth(
                SecretString::from("access-token"),
                "Bearer".to_string(),
                SecretString::from("refresh-token"),
            )
            .await;
        (dir, client)
    }

    #[test]
    fn instrument_identity_validates_reference_shapes() {
        for valid in [
            ID_A,
            "450DFC6D-5510-4D40-ABFB-F633B7D9BE3E",
            "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "https://api.robinhood.com/instruments/450DFC6D-5510-4D40-ABFB-F633B7D9BE3E/",
            "http://example.test/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e",
        ] {
            assert_eq!(canonical_instrument_id(valid).unwrap().to_string(), ID_A);
        }

        for invalid in [
            "",
            "not-a-uuid",
            "urn:uuid:450dfc6d-5510-4d40-abfb-f633b7d9be3e",
            "{450dfc6d-5510-4d40-abfb-f633b7d9be3e}",
            "450dfc6d55104d40abfbf633b7d9be3e",
            "/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "ftp://example.test/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "https://example.test/instruments/",
            "https://example.test/instruments/not-a-uuid/",
            "https://example.test/instruments/urn:uuid:450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "https://example.test/instruments/{450dfc6d-5510-4d40-abfb-f633b7d9be3e}/",
            "https://example.test/instruments/450dfc6d55104d40abfbf633b7d9be3e/",
            "https://example.test/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/history/",
            "https://example.test/other/../instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "https://example.test/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/?view=full",
            "https://example.test/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/#details",
        ] {
            assert!(
                canonical_instrument_id(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[tokio::test]
    async fn instrument_identity_validates_every_input_before_http() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, true, 50).await;

        let error = client
            .resolve_instrument_symbols(&[ID_A.to_string(), "invalid".to_string()])
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::InvalidParameter(_)));
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_preserves_duplicates_positions_and_misses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", format!("{ID_A},{ID_B}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, true, 50).await;

        let resolved = client
            .resolve_instrument_symbols(&[
                format!("https://elsewhere.test/instruments/{ID_A}/"),
                ID_B.to_string(),
                ID_A.to_string(),
            ])
            .await
            .unwrap();

        assert_eq!(
            resolved,
            vec![Some("AAPL".to_string()), None, Some("AAPL".to_string())]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_cache_enabled_fetches_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, true, 50).await;

        for _ in 0..2 {
            assert_eq!(
                client
                    .resolve_instrument_symbols(&[ID_A.to_string()])
                    .await
                    .unwrap(),
                vec![Some("AAPL".to_string())]
            );
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_cache_disabled_fetches_each_call() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(2)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 50).await;

        for _ in 0..2 {
            client
                .resolve_instrument_symbols(&[ID_A.to_string(), ID_A.to_string()])
                .await
                .unwrap();
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_honors_configured_chunk_boundary_and_plus_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .expect(3)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 2).await;

        client
            .resolve_instrument_symbols(&[ID_A.to_string(), ID_B.to_string()])
            .await
            .unwrap();
        client
            .resolve_instrument_symbols(&[ID_A.to_string(), ID_B.to_string(), ID_C.to_string()])
            .await
            .unwrap();

        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_ignores_unsolicited_incomplete_and_duplicate_rows() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    null,
                    {"symbol": "NOID"},
                    {"id": ID_B},
                    {"id": ID_C, "symbol": "OTHER"},
                    {"id": ID_A, "symbol": "AAPL"},
                    {"id": ID_A, "symbol": "WRONG"},
                    {"id": ID_B, "symbol": "VTI"}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 50).await;

        let resolved = client
            .resolve_instrument_symbols(&[ID_A.to_string(), ID_B.to_string()])
            .await
            .unwrap();

        assert_eq!(
            resolved,
            vec![Some("AAPL".to_string()), Some("VTI".to_string())]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_strict_later_chunk_failure_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_B))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 1).await;

        let error = client
            .resolve_instrument_symbols(&[ID_A.to_string(), ID_B.to_string()])
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RequestChunkFailed {
                chunk_index: 2,
                total_chunks: 2,
                ..
            }
        ));
        server.verify().await;
    }

    #[tokio::test]
    async fn instrument_identity_best_effort_applies_partial_chunks_fill_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_B))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 1).await;
        let mut positions = vec![
            Position {
                instrument: Some(format!("https://example.test/instruments/{ID_A}/")),
                symbol: None,
                ..serde_json::from_str("{}").unwrap()
            },
            Position {
                instrument: Some(ID_B.to_string()),
                symbol: None,
                ..serde_json::from_str("{}").unwrap()
            },
            Position {
                instrument: Some(format!("https://example.test/instruments/{ID_C}/")),
                symbol: Some("WIRE".to_string()),
                ..serde_json::from_str("{}").unwrap()
            },
        ];

        client
            .enrich_instrument_identities("test_positions", &mut positions)
            .await;

        assert_eq!(positions[0].symbol.as_deref(), Some("AAPL"));
        assert!(positions[1].symbol.is_none());
        assert_eq!(positions[2].symbol.as_deref(), Some("WIRE"));
        server.verify().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn instrument_identity_best_effort_emits_one_aggregate_warning() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server, false, 50).await;
        let capture = WarningCapture::default();
        let events = capture.events.clone();
        let subscriber_guard = tracing::subscriber::set_default(capture);

        let resolved = client
            .resolve_instrument_symbols_best_effort(
                "warning_contract",
                &[ID_A.to_string(), ID_B.to_string()],
            )
            .await;

        assert_eq!(resolved, vec![Some("AAPL".to_string()), None]);
        drop(subscriber_guard);
        {
            let events = events.lock().unwrap();
            assert_eq!(events.len(), 1);
            let event = events.first().unwrap();
            assert_eq!(
                event.get("operation").map(String::as_str),
                Some("warning_contract")
            );
            assert_eq!(event.get("requested").map(String::as_str), Some("2"));
            assert_eq!(event.get("resolved").map(String::as_str), Some("1"));
            assert_eq!(event.get("unresolved").map(String::as_str), Some("1"));
            assert_eq!(
                event.get("error_class").map(String::as_str),
                Some("upstream_miss")
            );
        }
        server.verify().await;
    }
}
