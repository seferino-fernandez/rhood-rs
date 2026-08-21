use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn dividends_returns_array() {
    let output = cli().args(["account", "dividends"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn dividend_total_returns_a_parseable_amount() {
    let output = cli()
        .args(["account", "dividends", "--total"])
        .assert()
        .success();
    let body = parse_json(&output);
    let amount = body
        .as_str()
        .or_else(|| body.get("total").and_then(serde_json::Value::as_str))
        .expect("dividend total should expose a decimal amount");
    amount
        .parse::<rust_decimal::Decimal>()
        .expect("dividend total should be a valid decimal");
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn interest_returns_array() {
    let output = cli().args(["account", "interest"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn transfers_returns_array() {
    let output = cli().args(["account", "transfers"]).assert().success();
    assert!(parse_json(&output).is_array());
}
