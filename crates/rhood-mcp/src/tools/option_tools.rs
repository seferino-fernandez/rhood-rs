use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use self::params::{
    FindOptionsParams, OptionChainParams, OptionOrderHistoryParams, OptionQuoteParams,
};
use super::handler::{AccountSelectorParams, RhoodTools};
use super::pagination::{AccountCollectionPageParams, CollectionPager};
use super::types::format_tool_error;

mod params {
    use rhood_core::models::option::OptionType;
    use schemars::JsonSchema;
    use serde::{Deserialize, Deserializer};
    use std::borrow::Cow;
    use url::Url;

    const OPTION_INSTRUMENT_URL_PATTERN: &str =
        r"^https://api\.robinhood\.com/options/instruments/[^/?#]+/?$";

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct OptionChainParams {
        /// Stock symbol to get option chain for
        #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
        pub(super) symbol: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct FindOptionsParams {
        /// Stock symbol (e.g. "AAPL")
        #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
        pub(super) symbol: String,
        /// Expiration date in YYYY-MM-DD format
        #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
        pub(super) expiration_date: String,
        /// "call" or "put"
        pub(super) option_type: OptionType,
        /// Strike price (optional filter)
        pub(super) strike_price: Option<f64>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct OptionQuoteContract {
        /// Strike price (e.g. 50.0)
        #[schemars(range(min = 0.0))]
        pub(super) strike_price: f64,
        /// Expiration date in YYYY-MM-DD format
        #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
        pub(super) expiration_date: String,
        /// Option type
        pub(super) option_type: OptionType,
    }

    pub(super) enum OptionQuoteParams {
        /// Discover contracts from their attributes, then quote them.
        Contracts {
            symbol: String,
            contracts: Vec<OptionQuoteContract>,
        },
        /// Quote held positions directly from their option-instrument URLs.
        InstrumentUrls { instrument_urls: Vec<String> },
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OptionQuoteParamsInput {
        symbol: Option<String>,
        contracts: Option<Vec<OptionQuoteContract>>,
        instrument_urls: Option<Vec<String>>,
    }

    #[expect(
        dead_code,
        reason = "this schema-only mirror is consumed through its JsonSchema implementation"
    )]
    #[derive(JsonSchema)]
    #[serde(untagged, deny_unknown_fields)]
    enum OptionQuoteParamsSchema {
        Contracts {
            /// Stock symbol (e.g. "NKE")
            #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
            symbol: String,
            /// Non-empty list of option contracts to discover and quote
            #[schemars(length(min = 1))]
            contracts: Vec<OptionQuoteContract>,
        },
        InstrumentUrls {
            /// Non-empty list of option-instrument URLs returned by get_option_positions
            #[schemars(
                length(min = 1),
                inner(regex(pattern = OPTION_INSTRUMENT_URL_PATTERN))
            )]
            instrument_urls: Vec<String>,
        },
    }

    impl<'de> Deserialize<'de> for OptionQuoteParams {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let input = OptionQuoteParamsInput::deserialize(deserializer)?;
            match (input.symbol, input.contracts, input.instrument_urls) {
                (Some(symbol), Some(contracts), None) => {
                    if contracts.is_empty() {
                        return Err(serde::de::Error::custom(
                            "contracts must contain at least one option contract",
                        ));
                    }
                    Ok(Self::Contracts { symbol, contracts })
                }
                (None, None, Some(instrument_urls)) => {
                    if instrument_urls.is_empty() {
                        return Err(serde::de::Error::custom(
                            "instrument_urls must contain at least one option-instrument URL",
                        ));
                    }
                    let instrument_urls = instrument_urls
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            canonicalize_option_instrument_url(&value).map_err(|error| {
                                serde::de::Error::custom(format!(
                                    "invalid instrument_urls[{index}]: {error}"
                                ))
                            })
                        })
                        .collect::<Result<Vec<_>, D::Error>>()?;
                    Ok(Self::InstrumentUrls { instrument_urls })
                }
                (None, None, None) => Err(serde::de::Error::custom(
                    "provide exactly one quote mode: {symbol, contracts} or {instrument_urls}",
                )),
                (Some(_), None, None) | (None, Some(_), None) => Err(serde::de::Error::custom(
                    "symbol and contracts must be provided together",
                )),
                _ => Err(serde::de::Error::custom(
                    "instrument_urls cannot be combined with symbol or contracts; choose exactly one quote mode",
                )),
            }
        }
    }

    impl JsonSchema for OptionQuoteParams {
        fn schema_name() -> Cow<'static, str> {
            "OptionQuoteParams".into()
        }

        fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
            let mut schema = OptionQuoteParamsSchema::json_schema(generator);
            let object = schema.ensure_object();
            if let Some(variants) = object.remove("anyOf") {
                let mut properties = serde_json::Map::new();
                if let Some(branches) = variants.as_array() {
                    for branch in branches {
                        if let Some(branch_properties) = branch
                            .get("properties")
                            .and_then(serde_json::Value::as_object)
                        {
                            properties.extend(branch_properties.clone());
                        }
                    }
                }
                object.insert("type".to_string(), serde_json::json!("object"));
                object.insert("properties".to_string(), properties.into());
                object.insert("oneOf".to_string(), variants);
            }
            schema
        }
    }

    fn canonicalize_option_instrument_url(value: &str) -> Result<String, String> {
        let parsed = Url::parse(value)
            .map_err(|error| format!("expected an absolute HTTPS URL: {error}"))?;
        if parsed.scheme() != "https" {
            return Err("URL scheme must be https".to_string());
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("URL must not contain userinfo".to_string());
        }
        if parsed.host_str() != Some("api.robinhood.com") {
            return Err("URL host must be exactly api.robinhood.com".to_string());
        }

        let (_raw_scheme, after_scheme) = value
            .split_once("://")
            .ok_or_else(|| "expected an absolute HTTPS URL".to_string())?;
        let authority_end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        let authority = after_scheme
            .get(..authority_end)
            .ok_or_else(|| "URL authority is not valid UTF-8".to_string())?;
        if authority.contains(':') {
            return Err("URL must not contain an explicit port".to_string());
        }
        if !authority.eq_ignore_ascii_case("api.robinhood.com") {
            return Err("URL host must be exactly api.robinhood.com".to_string());
        }
        if parsed.query().is_some() {
            return Err("URL must not contain a query".to_string());
        }
        if parsed.fragment().is_some() {
            return Err("URL must not contain a fragment".to_string());
        }

        fn exact_instrument_id(path: &str) -> Option<&str> {
            path.strip_prefix("/options/instruments/")
                .map(|remainder| remainder.strip_suffix('/').unwrap_or(remainder))
                .filter(|instrument_id| !instrument_id.is_empty() && !instrument_id.contains('/'))
        }
        let raw_path = after_scheme
            .get(authority_end..)
            .ok_or_else(|| "URL path is not valid UTF-8".to_string())?;
        let Some(raw_instrument_id) = exact_instrument_id(raw_path) else {
            return Err(
                "URL path must be /options/instruments/<non-empty-id> with no additional segment"
                    .to_string(),
            );
        };
        if exact_instrument_id(parsed.path()).is_none() {
            return Err(
                "URL path must be /options/instruments/<non-empty-id> with no additional segment"
                    .to_string(),
            );
        }

        let normalized_id = raw_instrument_id.to_ascii_lowercase();
        let has_encoded_separator = normalized_id.contains("%2f") || normalized_id.contains("%5c");
        let has_encoded_traversal = normalized_id.contains("%2e%2e")
            || normalized_id.contains("%2e.")
            || normalized_id.contains(".%2e");
        if has_encoded_separator || has_encoded_traversal {
            return Err(
                "option-instrument ID must not contain percent-encoded path separators or traversal"
                    .to_string(),
            );
        }

        let mut canonical = parsed.to_string();
        if !canonical.ends_with('/') {
            canonical.push('/');
        }
        Ok(canonical)
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct OptionOrderHistoryParams {
        /// Canonical brokerage account number from get_accounts. May be omitted only when exactly one account exists.
        pub(super) account_number: Option<String>,
        /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
        #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
        pub(super) since: Option<String>,
    }
}

/// Pairs each `OptionMarketData` result with the contract identity the caller
/// supplied (underlying symbol, strike, expiration, type), so multi-contract
/// responses don't have to be correlated by array position. `OptionMarketData`
/// has none of these fields, so flattening it adds no duplicate keys.
#[derive(serde::Serialize)]
struct OptionQuoteView {
    symbol: String,
    strike_price: f64,
    expiration_date: String,
    option_type: String,
    #[serde(flatten)]
    data: rhood_core::models::option::OptionMarketData,
}

fn aggregate_option_positions_cursor_scope(account_number: &str) -> String {
    format!("aggregate-option-positions:{account_number}")
}

#[tool_router(router = option_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_option_positions",
        description = "Get one cursor-paginated page of open option positions (contracts currently held)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_positions(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountCollectionPageParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let pager = CollectionPager::new(
            format!("option-positions:{account}"),
            params.cursor.as_deref(),
            params.limit,
        )?;
        let positions = client
            .get_open_option_positions(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        pager.serialize(positions)
    }

    #[tool(
        name = "get_aggregate_option_positions",
        description = "Get one cursor-paginated page of aggregate option strategies for an account",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_aggregate_option_positions(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountCollectionPageParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let pager = CollectionPager::new(
            aggregate_option_positions_cursor_scope(&account),
            params.cursor.as_deref(),
            params.limit,
        )?;
        let positions = client
            .get_aggregate_option_positions(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        pager.serialize(positions)
    }

    #[tool(
        name = "get_option_chain",
        description = "Get option chain metadata (available expiration dates, multiplier) for a stock symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_chain(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionChainParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let chain = client
            .get_option_chain(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&chain).map_err(|error| error.to_string())
    }

    #[tool(
        name = "find_options",
        description = "Search for equity option contracts by expiration, type, and optional strike",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn find_options(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FindOptionsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let strike_str = params.strike_price.map(|price| format!("{price:.4}"));
        let option_type = params.option_type.to_string();
        let options = client
            .find_options(
                &params.symbol,
                &params.expiration_date,
                &option_type,
                strike_str.as_deref(),
            )
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&options).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_option_quotes",
        description = "Get live option quotes by either symbol plus contract attributes or option-instrument URLs from get_option_positions; the modes are mutually exclusive",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_quotes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionQuoteParams>,
    ) -> Result<String, String> {
        use rhood_core::models::option::OptionContractSpec;

        let client = self.ensure_client(&peer).await?;
        match params {
            OptionQuoteParams::Contracts { symbol, contracts } => {
                let strike_strings: Vec<String> = contracts
                    .iter()
                    .map(|contract| format!("{:.4}", contract.strike_price))
                    .collect();
                let type_strings: Vec<String> = contracts
                    .iter()
                    .map(|contract| contract.option_type.to_string())
                    .collect();

                let specs: Vec<OptionContractSpec<'_>> = contracts
                    .iter()
                    .zip(strike_strings.iter())
                    .zip(type_strings.iter())
                    .map(|((contract, strike_str), type_str)| OptionContractSpec {
                        strike_price: strike_str,
                        expiration_date: &contract.expiration_date,
                        option_type: type_str,
                    })
                    .collect();

                let data = client
                    .get_option_market_data(&symbol, &specs)
                    .await
                    .map_err(|rhood_error| format_tool_error(&rhood_error))?;
                // Upstream preserves request order, so zip the input contracts back
                // onto each result to restore strike/expiration/type identity.
                let views: Vec<OptionQuoteView> = data
                    .into_iter()
                    .zip(contracts.iter())
                    .map(|(data, contract)| OptionQuoteView {
                        symbol: symbol.clone(),
                        strike_price: contract.strike_price,
                        expiration_date: contract.expiration_date.clone(),
                        option_type: contract.option_type.to_string(),
                        data,
                    })
                    .collect();
                serde_json::to_string_pretty(&views).map_err(|error| error.to_string())
            }
            OptionQuoteParams::InstrumentUrls { instrument_urls } => {
                let data = client
                    .get_option_market_data_by_instrument_urls(&instrument_urls)
                    .await
                    .map_err(|rhood_error| format_tool_error(&rhood_error))?;
                serde_json::to_string_pretty(&data).map_err(|error| error.to_string())
            }
        }
    }

    #[tool(
        name = "get_option_orders",
        description = "Get full option order history (all states: filled, cancelled, queued, etc.)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionOrderHistoryParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let orders = client
            .get_all_option_orders(&account, params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_open_option_orders",
        description = "List all open option orders",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_open_option_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let orders = client
            .get_open_option_orders(&account, None)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "these deserialization tests verify exact parsing of decimal literals"
)]
mod tests {
    use super::{
        OptionQuoteParams, OptionQuoteView, RhoodTools, aggregate_option_positions_cursor_scope,
    };
    use crate::tools::pagination::CollectionPager;
    use crate::tools::schema::close_object_schemas;
    use rhood_core::models::option::OptionMarketData;

    #[test]
    fn option_quote_view_carries_contract_identity() {
        let data: OptionMarketData =
            serde_json::from_str(r#"{"instrument":null,"instrument_id":"x","bid_price":"1.20"}"#)
                .unwrap();
        let view = OptionQuoteView {
            symbol: "AAPL".into(),
            strike_price: 310.0,
            expiration_date: "2026-06-18".into(),
            option_type: "call".into(),
            data,
        };
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value["symbol"], "AAPL");
        assert_eq!(value["strike_price"], 310.0);
        assert_eq!(value["expiration_date"], "2026-06-18");
        assert_eq!(value["option_type"], "call");
        assert_eq!(value["bid_price"], "1.20");
    }

    #[test]
    fn aggregate_option_position_cursors_are_bound_to_the_selected_account() {
        let first_account = aggregate_option_positions_cursor_scope("ABC123");
        let second_account = aggregate_option_positions_cursor_scope("DEF456");
        assert_eq!(first_account, "aggregate-option-positions:ABC123");
        assert_ne!(first_account, second_account);

        let cursor = CollectionPager::new(&first_account, None, Some(1))
            .unwrap()
            .serialize(vec![1, 2])
            .unwrap();
        let next_cursor =
            serde_json::from_str::<serde_json::Value>(&cursor).unwrap()["next_cursor"]
                .as_str()
                .unwrap()
                .to_owned();
        assert!(CollectionPager::new(&second_account, Some(&next_cursor), Some(1)).is_err());
    }

    const CONTRACT: &str =
        r#"{"strike_price":310.0,"expiration_date":"2026-06-18","option_type":"call"}"#;

    #[test]
    fn option_quotes_accepts_attribute_mode_unchanged() {
        let json = format!(r#"{{"symbol":"AAPL","contracts":[{CONTRACT}]}}"#);
        let parsed: OptionQuoteParams = serde_json::from_str(&json).unwrap();
        match parsed {
            OptionQuoteParams::Contracts { symbol, contracts } => {
                assert_eq!(symbol, "AAPL");
                assert_eq!(contracts.len(), 1);
                assert_eq!(contracts[0].strike_price, 310.0);
            }
            OptionQuoteParams::InstrumentUrls { .. } => panic!("expected attribute mode"),
        }
    }

    #[test]
    fn option_quotes_accepts_and_canonicalizes_instrument_url_mode() {
        let parsed: OptionQuoteParams = serde_json::from_str(
            r#"{"instrument_urls":["https://api.robinhood.com/options/instruments/not-a-uuid","https://api.robinhood.com/options/instruments/already-slash/"]}"#,
        )
        .unwrap();
        match parsed {
            OptionQuoteParams::InstrumentUrls { instrument_urls } => assert_eq!(
                instrument_urls,
                [
                    "https://api.robinhood.com/options/instruments/not-a-uuid/",
                    "https://api.robinhood.com/options/instruments/already-slash/",
                ]
            ),
            OptionQuoteParams::Contracts { .. } => panic!("expected instrument URL mode"),
        }
    }

    #[test]
    fn option_quotes_rejects_every_invalid_mode_combination() {
        let contracts = format!(r#"[{CONTRACT}]"#);
        let cases = [
            ("missing both modes", "{}".to_string()),
            ("symbol without contracts", r#"{"symbol":"AAPL"}"#.to_string()),
            ("contracts without symbol", format!(r#"{{"contracts":{contracts}}}"#)),
            (
                "symbol mixed with URLs",
                r#"{"symbol":"AAPL","instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}"#.to_string(),
            ),
            (
                "contracts mixed with URLs",
                format!(r#"{{"contracts":{contracts},"instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}}"#),
            ),
            (
                "both complete modes",
                format!(r#"{{"symbol":"AAPL","contracts":{contracts},"instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}}"#),
            ),
            ("empty contracts", r#"{"symbol":"AAPL","contracts":[]}"#.to_string()),
            ("empty URLs", r#"{"instrument_urls":[]}"#.to_string()),
        ];

        for (name, json) in cases {
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("{name} must fail"));
            let message = error.to_string();
            assert!(
                message.contains("mode")
                    || message.contains("together")
                    || message.contains("at least one"),
                "{name} must return an actionable error, got: {message}"
            );
        }
    }

    #[test]
    fn option_quotes_rejects_invalid_instrument_url_shapes() {
        let cases = [
            "options/instruments/id/",
            "http://api.robinhood.com/options/instruments/id/",
            "https://example.com/options/instruments/id/",
            "https://api.robinhood.com.evil.test/options/instruments/id/",
            "https://user@api.robinhood.com/options/instruments/id/",
            "https://api.robinhood.com:8443/options/instruments/id/",
            "https://api.robinhood.com:443/options/instruments/id/",
            "https://api.robinhood.com/options/instruments/id/?query=yes",
            "https://api.robinhood.com/options/instruments/id/#fragment",
            "https://api.robinhood.com/options/instruments/",
            "https://api.robinhood.com/options/instruments/id/extra",
            "https://api.robinhood.com/options/instruments/id//",
            "https://api.robinhood.com/instruments/id/",
            "https://api.robinhood.com/options/instruments/./",
            "https://api.robinhood.com/options/instruments/%2e%2e/",
        ];

        for value in cases {
            let json = serde_json::json!({"instrument_urls": [value]}).to_string();
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("invalid URL must fail: {value}"));
            assert!(
                error.to_string().contains("instrument_urls[0]"),
                "URL error must identify the invalid element for {value}: {error}"
            );
        }
    }

    #[test]
    fn option_quotes_rejects_encoded_path_syntax_in_nonempty_instrument_ids() {
        let cases = [
            (
                "lowercase encoded slash",
                "https://api.robinhood.com/options/instruments/id%2f../",
            ),
            (
                "uppercase encoded slash",
                "https://api.robinhood.com/options/instruments/id%2Fchild/",
            ),
            (
                "encoded traversal after a nonempty prefix",
                "https://api.robinhood.com/options/instruments/held%2E%2e/",
            ),
        ];

        for (name, value) in cases {
            let json = serde_json::json!({"instrument_urls": [value]}).to_string();
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("{name} must fail: {value}"));
            let message = error.to_string();
            assert!(
                message.contains("percent-encoded path separators or traversal"),
                "{name} must identify the forbidden encoded path syntax, got: {message}"
            );
        }
    }

    #[test]
    fn find_options_advertises_contract_search_schema() {
        let tool = RhoodTools::option_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "find_options")
            .expect("find_options must be advertised");

        assert!(
            tool.annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true))
        );
        let schema = serde_json::Value::Object((*tool.input_schema).clone());
        assert_eq!(
            schema["required"],
            serde_json::json!(["symbol", "expiration_date", "option_type"])
        );
        assert_eq!(
            schema["properties"]["symbol"]["pattern"],
            serde_json::json!("^[A-Za-z0-9.]{1,12}$")
        );
        assert_eq!(
            schema["properties"]["expiration_date"]["pattern"],
            serde_json::json!("^\\d{4}-\\d{2}-\\d{2}$")
        );
        let option_type_ref = schema["properties"]["option_type"]["$ref"]
            .as_str()
            .expect("option_type must reference its enum schema");
        let option_type_name = option_type_ref
            .strip_prefix("#/$defs/")
            .expect("option_type schema reference must use $defs");
        let option_type_values: Vec<_> = schema["$defs"][option_type_name]["oneOf"]
            .as_array()
            .expect("option_type must be a string enum")
            .iter()
            .map(|variant| {
                variant["const"]
                    .as_str()
                    .expect("option_type enum variants must be strings")
            })
            .collect();
        assert_eq!(option_type_values, ["call", "put"]);
        assert_eq!(
            schema["properties"]["strike_price"]["type"],
            serde_json::json!(["number", "null"])
        );
    }

    #[test]
    fn aggregate_option_positions_advertises_account_bound_pagination_and_read_only() {
        let tool = RhoodTools::option_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "get_aggregate_option_positions")
            .expect("aggregate option positions must be advertised");
        assert!(
            tool.annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true))
        );

        let mut schema = serde_json::Value::Object((*tool.input_schema).clone());
        close_object_schemas(&mut schema);
        assert_eq!(schema["required"], serde_json::json!([]));
        assert!(
            schema["properties"]["account_number"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("get_accounts"))
        );
        assert_eq!(schema["properties"]["cursor"]["maxLength"], 128);
        assert_eq!(schema["properties"]["limit"]["minimum"], 1);
        assert_eq!(schema["properties"]["limit"]["maximum"], 100);
    }

    #[test]
    fn option_quotes_advertises_two_exclusive_read_only_modes() {
        let tool = RhoodTools::option_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "get_option_quotes")
            .expect("get_option_quotes must be advertised");

        assert!(
            tool.annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true)),
            "get_option_quotes must retain its read-only annotation"
        );
        let mut schema = serde_json::Value::Object((*tool.input_schema).clone());
        close_object_schemas(&mut schema);
        assert_eq!(schema["type"], serde_json::json!("object"));
        assert_eq!(schema["additionalProperties"], serde_json::json!(false));
        assert_eq!(schema["properties"]["contracts"]["minItems"], 1);
        assert_eq!(schema["properties"]["instrument_urls"]["minItems"], 1);
        assert_eq!(
            schema["properties"]["instrument_urls"]["items"]["pattern"],
            serde_json::json!("^https://api\\.robinhood\\.com/options/instruments/[^/?#]+/?$")
        );

        let branches = schema["oneOf"]
            .as_array()
            .expect("quote modes must be represented by oneOf");
        assert_eq!(branches.len(), 2);
        let required_sets: Vec<_> = branches
            .iter()
            .map(|branch| branch["required"].clone())
            .collect();
        assert!(required_sets.contains(&serde_json::json!(["symbol", "contracts"])));
        assert!(required_sets.contains(&serde_json::json!(["instrument_urls"])));
        assert!(
            branches
                .iter()
                .all(|branch| branch["additionalProperties"] == serde_json::json!(false)),
            "each mode must reject fields from the other mode"
        );
    }
}
