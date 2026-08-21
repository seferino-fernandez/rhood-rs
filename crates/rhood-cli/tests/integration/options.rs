use crate::common::{STOCK_SYMBOL, cli, discover_option_fixture, parse_json};

#[test]
fn option_cancel_order_rejects_bad_uuid() {
    cli()
        .args(["option", "cancel-order", "not-a-uuid"])
        .assert()
        .failure()
        .code(2);
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn option_positions_returns_array() {
    let output = cli().args(["option", "positions"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn option_aggregate_positions_returns_array() {
    let output = cli()
        .args(["option", "aggregate-positions"])
        .assert()
        .success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn option_orders_returns_array() {
    let output = cli().args(["option", "orders"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn open_option_orders_returns_array() {
    let output = cli()
        .args(["option", "orders", "--open"])
        .assert()
        .success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn option_quote_uses_a_discovered_active_contract() {
    let fixture = discover_option_fixture(STOCK_SYMBOL, false);
    let output = cli()
        .args([
            "option",
            "quote",
            STOCK_SYMBOL,
            "--strike",
            &fixture.strike,
            "--expiry",
            &fixture.expiration,
            "--type",
            "call",
            "--detail",
        ])
        .assert()
        .success();
    let body = parse_json(&output);
    let quotes = body.as_array().expect("option quotes should be an array");
    assert_eq!(quotes.len(), 1);
    assert!(
        quotes[0]["instrument"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://api.robinhood.com/options/instruments/"))
    );
    assert!(
        quotes[0]["instrument_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}
