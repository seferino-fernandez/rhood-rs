use crate::common::{INDEX_SYMBOL, INDEX_SYMBOLS_MULTI, cli, parse_json};
use assert_cmd::Command;

/// Confirms that `rhood index options --help` exposes `--option-type` (not
/// just `--type`), so help text is discoverable regardless of which alias
/// a user tries.
#[test]
fn index_options_help_mentions_option_type() {
    let output = Command::cargo_bin("rhood")
        .expect("rhood binary not found")
        .args(["index", "options", "--help"])
        .output()
        .expect("failed to run rhood");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--option-type"),
        "`rhood index options --help` should list --option-type; got:\n{help}"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn index_quote_echoes_symbol() {
    let output = cli()
        .args(["index", "quote", INDEX_SYMBOL])
        .assert()
        .success();
    let body = parse_json(&output);
    let text = body.to_string();
    assert!(
        text.contains(INDEX_SYMBOL),
        "response should reference {INDEX_SYMBOL}"
    );
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn index_quote_multi_symbol_returns_array() {
    let mut args = vec!["index", "quote"];
    args.extend(INDEX_SYMBOLS_MULTI);
    let output = cli().args(&args).assert().success();
    let body = parse_json(&output);
    assert!(
        body.is_array() || body.is_object(),
        "multi-symbol index quote should be array or object"
    );
}
