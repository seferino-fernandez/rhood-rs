use crate::common::{WATCHLIST_NAME, cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn watchlist_list_returns_array() {
    let output = cli().args(["watchlist", "list"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn watchlist_show_default_list() {
    // Requires the default watchlist to exist. Users can delete default
    // watchlists, in which case this test will fail with a 404 - that's a
    // legitimate signal the fixture needs updating.
    let output = cli()
        .args(["watchlist", "show", WATCHLIST_NAME])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(body.is_array() || body.is_object());
}
