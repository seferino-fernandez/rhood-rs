use crate::common::{MARKET_DATE, MARKET_MIC, cli, parse_json};

#[test]
fn market_hours_rejects_bad_date() {
    cli()
        .args(["market", "hours", "XNYS", "not-a-date"])
        .assert()
        .failure()
        .code(2);
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn market_list_returns_array() {
    let output = cli().args(["market", "list"]).assert().success();
    let body = parse_json(&output);
    assert!(
        body.is_array(),
        "market list should be a JSON array of exchanges"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn market_hours_for_exchange_and_date_returns_object() {
    // `market hours` requires <MIC> <DATE>. We use a weekday well in the future
    // so the call always resolves against a future trading day.
    let output = cli()
        .args(["market", "hours", MARKET_MIC, MARKET_DATE])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_object() || body.is_array(),
        "market hours should be an object or array"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn market_today_returns_object() {
    let output = cli()
        .args(["market", "today", MARKET_MIC])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_object() || body.is_array(),
        "market today should be an object or array"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn market_movers_returns_array() {
    let output = cli().args(["market", "movers"]).assert().success();
    let body = parse_json(&output);
    assert!(
        body.is_array() || body.is_object(),
        "market movers should be an array or object"
    );
}
