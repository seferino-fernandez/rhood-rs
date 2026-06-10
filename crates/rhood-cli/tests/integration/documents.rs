use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn documents_returns_array() {
    let output = cli().args(["account", "documents"]).assert().success();
    assert!(parse_json(&output).is_array());
}
