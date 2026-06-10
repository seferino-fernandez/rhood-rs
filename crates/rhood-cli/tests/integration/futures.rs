use crate::common::{FUTURES_SYMBOL, cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_contract_echoes_symbol() {
    let output = cli()
        .args(["futures", "contract", FUTURES_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    // The displaySymbol typically looks like "/ESM26" - verify the payload
    // at least contains the bare symbol somewhere.
    let text = body.to_string();
    assert!(
        text.contains(FUTURES_SYMBOL),
        "response should reference {FUTURES_SYMBOL}"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_quote_echoes_symbol() {
    let output = cli()
        .args(["futures", "quote", FUTURES_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    let text = body.to_string();
    assert!(
        text.contains(FUTURES_SYMBOL),
        "response should reference {FUTURES_SYMBOL}"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn futures_orders_returns_array() {
    let output = cli().args(["futures", "orders"]).assert().success();
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
