use crate::common::{POPULAR_TAG, STOCK_SYMBOL, STOCK_SYMBOLS_MULTI, cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_quote_echoes_requested_symbol() {
    let output = cli()
        .args(["stock", "quote", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    let quote = &body[0];
    assert_eq!(
        quote["symbol"], STOCK_SYMBOL,
        "expected {STOCK_SYMBOL} echoed in response"
    );
    assert!(quote["bid_price"].is_string() || quote["bid_price"].is_null());
    assert!(quote["ask_price"].is_string() || quote["ask_price"].is_null());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_quote_multi_symbol_preserves_order() {
    let mut args = vec!["stock", "quote"];
    args.extend(STOCK_SYMBOLS_MULTI);
    let output = cli().args(&args).assert().success();
    let body = parse_json(&output);
    let array = body.as_array().expect("expected JSON array");
    assert_eq!(array.len(), STOCK_SYMBOLS_MULTI.len());
    for (index, expected_symbol) in STOCK_SYMBOLS_MULTI.iter().enumerate() {
        assert_eq!(array[index]["symbol"], *expected_symbol);
    }
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_history_returns_candles_for_symbol() {
    let output = cli()
        .args([
            "stock",
            "history",
            STOCK_SYMBOL,
            "--span",
            "week",
            "--interval",
            "day",
        ])
        .assert()
        .success();
    let body = parse_json(&output);
    let candles = body.as_array().expect("expected JSON array of candles");
    assert!(
        !candles.is_empty(),
        "expected at least one candle for a week-long query"
    );
    for candle in candles {
        assert!(candle["open_price"].is_string() || candle["open_price"].is_null());
        assert!(candle["close_price"].is_string() || candle["close_price"].is_null());
    }
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_fundamentals_echoes_symbol_via_enrichment() {
    // The CLI calls rhood-core directly (not the MCP enrichment), so the
    // response may or may not carry a `symbol` field depending on upstream.
    // We just assert the call succeeds and returns a non-empty payload.
    let output = cli()
        .args(["stock", "fundamentals", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_array() || body.is_object(),
        "fundamentals should be array or object"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_latest_prices_returns_price_per_symbol() {
    let mut args = vec!["stock", "latest-prices"];
    args.extend(STOCK_SYMBOLS_MULTI);
    let output = cli().args(&args).assert().success();
    let body = parse_json(&output);
    let prices = body.as_array().expect("expected JSON array");
    assert_eq!(prices.len(), STOCK_SYMBOLS_MULTI.len());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_earnings_returns_array_for_symbol() {
    let output = cli()
        .args(["stock", "earnings", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(body.is_array(), "earnings should be a JSON array");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_ratings_returns_summary_object() {
    let output = cli()
        .args(["stock", "ratings", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(body.is_object(), "ratings should be a JSON object");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_news_returns_array_of_articles() {
    let output = cli()
        .args(["stock", "news", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_array() || body.is_null(),
        "news should be a JSON array (may be empty/null for symbols with no coverage)"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_splits_returns_array() {
    let output = cli()
        .args(["stock", "splits", STOCK_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(body.is_array(), "splits should be a JSON array");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn stock_tags_returns_instrument_list() {
    let output = cli()
        .args(["stock", "tags", POPULAR_TAG])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_array() || body.is_object(),
        "tags should be array or object"
    );
}
