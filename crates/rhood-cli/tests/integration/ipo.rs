use crate::common::{STOCK_INSTRUMENT_ID, cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn ipo_access_list_returns_object() {
    let output = cli().args(["ipo", "list"]).assert().success();
    assert!(parse_json(&output).is_object());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn ipo_access_cards_returns_array() {
    let output = cli()
        .args(["ipo", "cards", STOCK_INSTRUMENT_ID])
        .assert()
        .success();
    assert!(parse_json(&output).is_array());
}
