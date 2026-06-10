use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn order_list_returns_array() {
    let output = cli().args(["order", "list"]).assert().success();
    assert!(parse_json(&output).is_array());
}
