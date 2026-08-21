use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn order_list_returns_array() {
    let output = cli().args(["order", "list"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn open_order_filter_returns_only_cancellable_states() {
    let output = cli()
        .args(["order", "list", "--status", "open"])
        .assert()
        .success();
    let body = parse_json(&output);
    let orders = body.as_array().expect("open orders should be an array");
    assert!(orders.iter().all(|order| {
        matches!(
            order["state"].as_str(),
            Some("queued" | "confirmed" | "unconfirmed" | "partially_filled")
        )
    }));
}
