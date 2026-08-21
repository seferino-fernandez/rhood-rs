use crate::common::{cli, futures_symbol, one_year_ago, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_contract_echoes_symbol() {
    let symbol = futures_symbol();
    let output = cli()
        .args(["futures", "contract", &symbol])
        .assert()
        .success();
    let body = parse_json(&output);
    // The displaySymbol typically looks like "/ESM26" - verify the payload
    // at least contains the bare symbol somewhere.
    let text = body.to_string();
    assert!(text.contains(&symbol), "response should reference {symbol}");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_quote_echoes_symbol() {
    let symbol = futures_symbol();
    let output = cli().args(["futures", "quote", &symbol]).assert().success();
    let body = parse_json(&output);
    let text = body.to_string();
    assert!(text.contains(&symbol), "response should reference {symbol}");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_orders_returns_array() {
    let output = cli().args(["futures", "orders"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_orders_since_filters_without_an_upstream_query_error() {
    let since = one_year_ago();
    let output = cli()
        .args(["futures", "orders", "--since", &since])
        .assert()
        .success();
    let body = parse_json(&output);
    let orders = body.as_array().expect("futures orders should be an array");
    assert!(orders.iter().all(|order| {
        order["updated_at"]
            .as_str()
            .is_some_and(|updated_at| updated_at >= since.as_str())
    }));
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_positions_returns_array() {
    let output = cli().args(["futures", "positions"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_account_returns_identifier() {
    let output = cli().args(["futures", "account"]).assert().success();
    let body = parse_json(&output);
    assert!(
        body.is_object() || body.is_string(),
        "futures account should be object or id string"
    );
}
