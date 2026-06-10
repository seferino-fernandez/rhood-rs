/// Expected format for date arguments.
pub const DATE_FORMAT_PATTERN: &str = "YYYY-MM-DD";
/// Decimal precision for strike price formatting (e.g., "150.0000").
pub const STRIKE_PRICE_DECIMALS: usize = 4;

/// clap `value_parser` for YYYY-MM-DD date arguments. Rejects malformed dates
/// at parse time (exit code 2) so no malformed value reaches the API.
pub fn parse_date(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && value
            .char_indices()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return Err(format!("must be in {DATE_FORMAT_PATTERN} format"));
    }
    let month: u8 = value[5..7]
        .parse()
        .map_err(|_| "invalid month".to_string())?;
    let day: u8 = value[8..10]
        .parse()
        .map_err(|_| "invalid day".to_string())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(format!(
            "must be a real calendar date in {DATE_FORMAT_PATTERN} format"
        ));
    }
    Ok(value.to_string())
}

/// clap `value_parser` for UUID arguments (8-4-4-4-12 hex). Rejects malformed
/// IDs at parse time (exit code 2) so no malformed value reaches the API.
pub fn parse_uuid(value: &str) -> Result<String, String> {
    let groups: Vec<&str> = value.split('-').collect();
    let lengths_ok = groups.len() == 5
        && [8usize, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(want, g)| g.len() == *want);
    let hex_ok = groups
        .iter()
        .all(|g| g.chars().all(|c| c.is_ascii_hexdigit()));
    if lengths_ok && hex_ok {
        Ok(value.to_string())
    } else {
        Err("must be a valid UUID (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_accepts_valid() {
        assert_eq!(parse_date("2026-06-01").unwrap(), "2026-06-01");
    }

    #[test]
    fn parse_date_rejects_garbage() {
        assert!(parse_date("not-a-date").is_err());
        assert!(parse_date("2026/06/01").is_err());
        assert!(parse_date("2026-13-40").is_err());
        assert!(parse_date("206-6-1").is_err());
    }

    #[test]
    fn parse_uuid_accepts_valid() {
        let id = "6a1b9bec-419a-4bab-97bc-c1f562be70c4";
        assert_eq!(parse_uuid(id).unwrap(), id);
    }

    #[test]
    fn parse_uuid_rejects_garbage() {
        assert!(parse_uuid("not-a-uuid").is_err());
        assert!(parse_uuid("6a1b9bec419a4bab97bcc1f562be70c4").is_err());
    }
}
