use crate::common::{cli, parse_json};

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
fn option_orders_returns_array() {
    let output = cli().args(["option", "orders"]).assert().success();
    assert!(parse_json(&output).is_array());
}

// `option quote` takes symbol + expiration + strike + type; requires checking
// the option chain first. Skipped until we add a fixture contract.
