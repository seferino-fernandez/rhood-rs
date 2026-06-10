use crate::common::{RECURRING_START_DATE, cli, parse_json};

#[test]
#[ignore = "requires live Robinhood credentials"]
fn recurring_list_returns_array() {
    let output = cli().args(["recurring", "list"]).assert().success();
    assert!(parse_json(&output).is_array());
}

#[test]
#[ignore = "requires live Robinhood credentials"]
fn recurring_next_date_returns_object() {
    // `next-date` requires BOTH --frequency and --start-date flags.
    let output = cli()
        .args([
            "recurring",
            "next-date",
            "--frequency",
            "weekly",
            "--start-date",
            RECURRING_START_DATE,
        ])
        .assert()
        .success();
    let body = parse_json(&output);
    assert!(
        body.is_object() || body.is_string(),
        "next-date should be an object or ISO date string"
    );
}
