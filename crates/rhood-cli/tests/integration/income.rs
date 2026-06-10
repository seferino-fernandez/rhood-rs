use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn dividends_returns_array() {
    let output = cli().args(["account", "dividends"]).assert().success();
    assert!(parse_json(&output).is_array());
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
