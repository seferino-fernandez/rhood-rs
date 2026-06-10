use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn positions_returns_array() {
    let output = cli().args(["account", "positions"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn portfolio_returns_object() {
    let output = cli().args(["account", "portfolio"]).assert().success();
    assert!(parse_json(&output).is_object());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn profile_returns_object() {
    let output = cli().args(["account", "profile"]).assert().success();
    assert!(parse_json(&output).is_object());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn buying_power_returns_object() {
    let output = cli().args(["account", "buying-power"]).assert().success();
    assert!(parse_json(&output).is_object());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn all_positions_returns_array() {
    let output = cli().args(["account", "all-positions"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn day_trades_returns_object() {
    let output = cli().args(["account", "day-trades"]).assert().success();
    let body = parse_json(&output);
    assert!(
        body.is_object() || body.is_array(),
        "day-trades should be object or array"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn user_profile_returns_object() {
    let output = cli().args(["account", "user-profile"]).assert().success();
    assert!(parse_json(&output).is_object());
}
