//! Process-level MCP transport tests.
//!
//! The deterministic tests exercise the real stdio binary without contacting
//! Robinhood. Ignored tests cover authenticated production calls and must be
//! requested explicitly.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::let_underscore_must_use,
    clippy::panic,
    reason = "black-box tests use explicit fixture-key indexing, direct diagnostics, and best-effort child cleanup"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use serde_json::{Value, json};

const MODERN_PROTOCOL: &str = "2026-07-28";
const LEGACY_PROTOCOL: &str = "2025-11-25";
const STOCK_SYMBOL: &str = "AAPL";
const INDEX_SYMBOL: &str = "SPX";
const STOCK_INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";

struct StdioMcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

fn utc_today() -> Date {
    Timestamp::now().to_zoned(TimeZone::UTC).date()
}

impl StdioMcp {
    fn spawn(live: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rhood-mcp"));
        command
            .args(["--transport", "stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Command::env(&mut command, "RHOOD_READ_ONLY", "true");
        if !live {
            Command::env(
                &mut command,
                "RHOOD_CONFIG",
                std::env::temp_dir().join("rhood-mcp-protocol-test-no-config.toml"),
            );
            for key in ["RHOOD_USERNAME", "RHOOD_PASSWORD", "RHOOD_TOTP_SECRET"] {
                Command::env_remove(&mut command, key);
            }
        }
        let mut child = command.spawn().expect("spawn rhood-mcp stdio server");
        let input = child.stdin.take().expect("capture MCP stdin");
        let output = BufReader::new(child.stdout.take().expect("capture MCP stdout"));
        Self {
            child,
            input,
            output,
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("MCP request id remains representable");
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        serde_json::to_writer(&mut self.input, &request).expect("serialize MCP request");
        self.input.write_all(b"\n").expect("terminate MCP request");
        self.input.flush().expect("flush MCP request");

        loop {
            let mut line = String::new();
            let read = self.output.read_line(&mut line).expect("read MCP response");
            assert!(read > 0, "MCP server closed before responding to {method}");
            let response: Value = serde_json::from_str(&line).expect("MCP response is JSON");
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                return response;
            }
        }
    }

    fn notify_initialized(&mut self) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });
        serde_json::to_writer(&mut self.input, &notification)
            .expect("serialize initialized notification");
        self.input
            .write_all(b"\n")
            .expect("terminate initialized notification");
        self.input.flush().expect("flush initialized notification");
    }

    fn initialize(&mut self, version: &str) -> Value {
        self.request(
            "initialize",
            json!({
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "rhood-integration-test", "version": "1" },
            }),
        )
    }

    fn all_tools(&mut self) -> Vec<Value> {
        let mut cursor: Option<String> = None;
        let mut tools = Vec::new();
        for _ in 0..10 {
            let mut params = json!({});
            if let Some(current) = cursor.as_deref() {
                params["cursor"] = Value::String(current.to_owned());
            }
            let response = self.request("tools/list", params);
            assert!(
                response.get("error").is_none(),
                "tools/list returned an error"
            );
            let result = &response["result"];
            assert_eq!(result["resultType"], "complete");
            assert_eq!(result["ttlMs"], 300_000);
            assert_eq!(result["cacheScope"], "private");
            tools.extend(
                result["tools"]
                    .as_array()
                    .expect("tools/list result has an array")
                    .iter()
                    .cloned(),
            );
            cursor = result["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none(), "tools/list pagination did not terminate");
        tools
    }

    fn call_json(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert!(
            response.get("error").is_none(),
            "MCP request failed for tool {name}"
        );
        let result = &response["result"];
        assert_eq!(
            result["isError"], false,
            "MCP tool reported an error for {name}"
        );
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "rhood-mcp"
        );
        let text = result["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|content| content["text"].as_str())
            .unwrap_or_else(|| panic!("MCP tool {name} returned no text content"));
        serde_json::from_str(text)
            .unwrap_or_else(|_| panic!("MCP tool {name} returned non-JSON text"))
    }
}

impl Drop for StdioMcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_names(tools: &[Value]) -> BTreeSet<String> {
    tools
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn stdio_binary_negotiates_modern_protocol_and_pages_read_only_tools() {
    let mut client = StdioMcp::spawn(false);
    let initialized = client.initialize(MODERN_PROTOCOL);
    assert_eq!(initialized["result"]["protocolVersion"], MODERN_PROTOCOL);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "rhood-mcp");
    client.notify_initialized();

    let tools = client.all_tools();
    assert_eq!(tools.len(), 50);
    let names = tool_names(&tools);
    assert_eq!(names.len(), tools.len(), "tool names must be unique");
    assert!(tools.iter().all(|tool| {
        tool["annotations"]["readOnlyHint"] == Value::Bool(true)
            && tool["inputSchema"]["additionalProperties"] == Value::Bool(false)
    }));
    for write_tool in [
        "place_stock_order",
        "place_option_order",
        "cancel_order",
        "cancel_option_order",
        "create_recurring_investment",
        "update_recurring_investment",
        "cancel_recurring_investment",
        "add_to_watchlist",
        "remove_from_watchlist",
    ] {
        assert!(!names.contains(write_tool));
    }
}

#[test]
fn stdio_binary_rejects_legacy_protocol() {
    let mut client = StdioMcp::spawn(false);
    let response = client.initialize(LEGACY_PROTOCOL);
    assert!(response.get("result").is_none());
    assert_eq!(response["error"]["code"], -32_022);
}

fn first_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| map.values().find_map(|child| first_string(child, key))),
        Value::Array(values) => values.iter().find_map(|child| first_string(child, key)),
        _ => None,
    }
}

fn current_futures_symbol() -> String {
    let today = utc_today();
    let quarters = [(3, 'H'), (6, 'M'), (9, 'U'), (12, 'Z')];
    let (code, year) = quarters
        .iter()
        .find(|(month, _)| *month > today.month() || (*month == today.month() && today.day() <= 15))
        .map_or_else(
            || {
                (
                    'H',
                    today
                        .year()
                        .checked_add(1)
                        .expect("next futures contract year is representable"),
                )
            },
            |(_, code)| (*code, today.year()),
        );
    format!("ES{code}{:02}", year.rem_euclid(100))
}

fn call_live(
    client: &mut StdioMcp,
    called: &mut BTreeSet<String>,
    name: &str,
    arguments: Value,
) -> Value {
    let result = client.call_json(name, arguments);
    assert!(
        called.insert(name.to_owned()),
        "tool {name} was called twice"
    );
    eprintln!("live MCP ok: {name}");
    thread::sleep(Duration::from_millis(300));
    result
}

#[test]
#[ignore = "requires live Robinhood credentials and production API access"]
fn stdio_calls_every_live_read_only_tool() {
    let mut client = StdioMcp::spawn(true);
    let initialized = client.initialize(MODERN_PROTOCOL);
    assert_eq!(initialized["result"]["protocolVersion"], MODERN_PROTOCOL);
    client.notify_initialized();
    let advertised = tool_names(&client.all_tools());
    assert_eq!(advertised.len(), 50);
    let mut called = BTreeSet::new();

    let accounts = call_live(&mut client, &mut called, "get_accounts", json!({}));
    let account_number = first_string(&accounts, "account_number")
        .expect("live account discovery returns a canonical account number");

    let option_chain = call_live(
        &mut client,
        &mut called,
        "get_option_chain",
        json!({ "symbol": STOCK_SYMBOL }),
    );
    let today = utc_today().to_string();
    let option_expiration = option_chain["expiration_dates"]
        .as_array()
        .and_then(|dates| {
            dates
                .iter()
                .filter_map(Value::as_str)
                .find(|date| *date >= today.as_str())
        })
        .expect("equity option chain has a current expiration")
        .to_owned();
    let options = call_live(
        &mut client,
        &mut called,
        "find_options",
        json!({
            "symbol": STOCK_SYMBOL,
            "expiration_date": option_expiration,
            "option_type": "call",
        }),
    );
    let option = options
        .as_array()
        .and_then(|contracts| contracts.first())
        .expect("equity option discovery returns an active contract");
    let option_strike = option["strike_price"]
        .as_str()
        .expect("option contract has a strike")
        .parse::<f64>()
        .expect("option strike is numeric");

    let index_chain = call_live(
        &mut client,
        &mut called,
        "get_index_option_chain",
        json!({ "symbol": INDEX_SYMBOL }),
    );
    let index_expiration = index_chain["expiration_dates"]
        .as_array()
        .and_then(|dates| {
            dates
                .iter()
                .filter_map(Value::as_str)
                .find(|date| *date >= today.as_str())
        })
        .expect("index option chain has a current expiration")
        .to_owned();
    let _ = call_live(
        &mut client,
        &mut called,
        "find_index_options",
        json!({
            "symbol": INDEX_SYMBOL,
            "expiration_date": index_expiration,
            "option_type": "call",
        }),
    );

    let watchlists = call_live(&mut client, &mut called, "get_watchlists", json!({}));
    let watchlist_name = first_string(&watchlists, "display_name")
        .or_else(|| first_string(&watchlists, "id"))
        .expect("live account exposes a readable watchlist");

    let one_year_ago = utc_today()
        .checked_sub(365.days())
        .expect("one year ago is representable")
        .to_string();
    let future = current_futures_symbol();
    let account = || json!({ "account_number": account_number });
    let mut remaining = BTreeMap::new();
    remaining.insert("get_account_profile", account());
    remaining.insert("get_account_summary", account());
    remaining.insert(
        "get_aggregate_option_positions",
        json!({ "account_number": account_number, "limit": 5 }),
    );
    remaining.insert("get_all_futures_orders", json!({ "since": one_year_ago }));
    remaining.insert("get_all_positions", account());
    remaining.insert(
        "get_all_stock_orders",
        json!({ "account_number": account_number, "since": one_year_ago }),
    );
    remaining.insert("get_daily_movers", json!({}));
    remaining.insert("get_day_trades", account());
    remaining.insert("get_dividends", json!({ "since": one_year_ago }));
    remaining.insert("get_documents", json!({ "limit": 5 }));
    remaining.insert("get_earnings", json!({ "symbol": STOCK_SYMBOL }));
    remaining.insert("get_fundamentals", json!({ "symbols": [STOCK_SYMBOL] }));
    remaining.insert("get_futures_account", json!({}));
    remaining.insert("get_futures_contract", json!({ "symbol": future }));
    remaining.insert("get_futures_positions", json!({}));
    remaining.insert("get_futures_quotes", json!({ "symbols": [future] }));
    remaining.insert("get_index_quotes", json!({ "symbols": [INDEX_SYMBOL] }));
    remaining.insert("get_interest_payments", json!({}));
    remaining.insert(
        "get_ipo_access_cards",
        json!({ "instrument_ids": [STOCK_INSTRUMENT_ID] }),
    );
    remaining.insert("get_ipo_access_list", json!({}));
    remaining.insert("get_latest_prices", json!({ "symbols": [STOCK_SYMBOL] }));
    remaining.insert("get_market_hours", json!({ "mic": "XNYS", "date": today }));
    remaining.insert("get_market_today_hours", json!({ "mic": "XNYS" }));
    remaining.insert("get_markets", json!({}));
    remaining.insert("get_news", json!({ "symbol": STOCK_SYMBOL }));
    remaining.insert(
        "get_next_investment_date",
        json!({ "frequency": "weekly", "start_date": today }),
    );
    remaining.insert("get_open_option_orders", account());
    remaining.insert("get_open_orders", account());
    remaining.insert(
        "get_option_orders",
        json!({ "account_number": account_number, "since": one_year_ago }),
    );
    remaining.insert(
        "get_option_positions",
        json!({ "account_number": account_number, "limit": 5 }),
    );
    remaining.insert(
        "get_option_quotes",
        json!({
            "symbol": STOCK_SYMBOL,
            "contracts": [{
                "strike_price": option_strike,
                "expiration_date": option_expiration,
                "option_type": "call",
            }],
        }),
    );
    remaining.insert("get_portfolio", account());
    remaining.insert("get_portfolio_performance", account());
    remaining.insert("get_positions", account());
    remaining.insert("get_ratings", json!({ "symbol": STOCK_SYMBOL }));
    remaining.insert("get_recurring_investments", account());
    remaining.insert("get_splits", json!({ "symbol": STOCK_SYMBOL }));
    remaining.insert(
        "get_stock_history",
        json!({ "symbol": STOCK_SYMBOL, "span": "week", "interval": "day" }),
    );
    remaining.insert("get_stock_quotes", json!({ "symbols": [STOCK_SYMBOL] }));
    remaining.insert("get_tags", json!({ "tag": "100-most-popular" }));
    remaining.insert("get_total_dividends", json!({}));
    remaining.insert("get_transfers", json!({ "limit": 5 }));
    remaining.insert("get_user_profile", json!({}));
    remaining.insert("get_watchlist_items", json!({ "name": watchlist_name }));

    for (name, arguments) in remaining {
        let _ = call_live(&mut client, &mut called, name, arguments);
    }
    assert_eq!(
        called, advertised,
        "every advertised read tool must be called"
    );
}

struct HttpServer {
    child: Child,
    base_url: String,
}

impl HttpServer {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve HTTP test port");
        let port = listener.local_addr().expect("read reserved port").port();
        drop(listener);
        let mut command = Command::new(env!("CARGO_BIN_EXE_rhood-mcp"));
        command
            .args([
                "--transport",
                "http",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--auth-mode",
                "none",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Command::env(&mut command, "RHOOD_READ_ONLY", "true");
        let child = command.spawn().expect("spawn HTTP MCP server");
        Self {
            child,
            base_url: format!("http://127.0.0.1:{port}/mcp"),
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL,
        "io.modelcontextprotocol/clientInfo": {
            "name": "rhood-http-integration-test",
            "version": "1",
        },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn http_request(
    client: &reqwest::Client,
    base_url: &str,
    method: &str,
    name: Option<&str>,
    params: Value,
) -> Result<Value, reqwest::Error> {
    let mut body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    if method != "initialize" {
        body["params"]["_meta"] = modern_meta();
    }
    let mut request = client
        .post(base_url)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json");
    if method != "initialize" {
        request = request
            .header("mcp-protocol-version", MODERN_PROTOCOL)
            .header("mcp-method", method);
    }
    if let Some(name) = name {
        request = request.header("mcp-name", name);
    }
    let response = request.json(&body).send().await?.error_for_status()?;
    let text = response.text().await?;
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .unwrap_or(text.trim());
    Ok(serde_json::from_str(payload).expect("HTTP MCP response is JSON"))
}

#[tokio::test]
#[ignore = "requires live Robinhood credentials and production API access"]
async fn http_transport_initializes_and_calls_each_read_domain() {
    let server = HttpServer::spawn();
    let client = reqwest::Client::new();
    let initialize_params = json!({
        "protocolVersion": MODERN_PROTOCOL,
        "capabilities": {},
        "clientInfo": { "name": "rhood-http-integration-test", "version": "1" },
    });
    let mut initialized = None;
    for _ in 0..60 {
        match http_request(
            &client,
            &server.base_url,
            "initialize",
            None,
            initialize_params.clone(),
        )
        .await
        {
            Ok(response) => {
                initialized = Some(response);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let initialized = initialized.expect("HTTP MCP server becomes ready");
    assert_eq!(initialized["result"]["protocolVersion"], MODERN_PROTOCOL);

    let listed = http_request(&client, &server.base_url, "tools/list", None, json!({}))
        .await
        .expect("HTTP tools/list succeeds");
    assert_eq!(
        listed["result"]["tools"]
            .as_array()
            .expect("HTTP tools/list returns an array")
            .len(),
        20
    );

    let representatives = [
        ("get_accounts", json!({})),
        ("get_stock_quotes", json!({ "symbols": [STOCK_SYMBOL] })),
        ("get_option_chain", json!({ "symbol": STOCK_SYMBOL })),
        ("get_futures_account", json!({})),
        ("get_index_quotes", json!({ "symbols": [INDEX_SYMBOL] })),
        ("get_ipo_access_list", json!({})),
        ("get_interest_payments", json!({})),
        ("get_recurring_investments", json!({})),
        ("get_watchlists", json!({})),
        ("get_user_profile", json!({})),
        ("get_markets", json!({})),
        ("get_earnings", json!({ "symbol": STOCK_SYMBOL })),
        ("get_open_orders", json!({})),
        ("get_documents", json!({ "limit": 1 })),
    ];
    for (name, arguments) in representatives {
        let response = http_request(
            &client,
            &server.base_url,
            "tools/call",
            Some(name),
            json!({ "name": name, "arguments": arguments }),
        )
        .await
        .unwrap_or_else(|_| panic!("HTTP MCP request failed for {name}"));
        assert!(response.get("error").is_none(), "HTTP MCP error for {name}");
        assert_eq!(
            response["result"]["isError"], false,
            "tool error for {name}"
        );
        eprintln!("live HTTP MCP ok: {name}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
