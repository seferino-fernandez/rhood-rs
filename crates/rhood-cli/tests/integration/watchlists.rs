use crate::common::{cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn watchlist_list_returns_array() {
    let output = cli().args(["watchlist", "list"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn watchlist_show_default_list() {
    let lists_output = cli().args(["watchlist", "list"]).assert().success();
    let lists = parse_json(&lists_output);
    let name = lists
        .as_array()
        .and_then(|lists| lists.first())
        .and_then(|list| {
            list["display_name"]
                .as_str()
                .or_else(|| list["id"].as_str())
        })
        .expect("live account should expose at least one readable watchlist");
    let output = cli().args(["watchlist", "show", name]).assert().success();
    let body = parse_json(&output);
    assert!(body.is_array() || body.is_object());
}
