//! Small shared helpers.

/// Temporary compatibility helper that borrows the UUID from a validated
/// Instrument Reference.
pub fn instrument_id_from_url(url: &str) -> Option<&str> {
    crate::client::instrument_identity::canonical_instrument_id(url).ok()?;
    if uuid::Uuid::parse_str(url).is_ok() {
        return Some(url);
    }
    let trimmed = url.trim_end_matches('/');
    let id = trimmed.rsplit('/').next()?;
    if id.is_empty() { None } else { Some(id) }
}

#[cfg(test)]
mod tests {
    use super::instrument_id_from_url;

    #[test]
    fn full_url_with_trailing_slash_returns_uuid() {
        let url = "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/";
        assert_eq!(
            instrument_id_from_url(url),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
    }

    #[test]
    fn full_url_without_trailing_slash_returns_uuid() {
        let url = "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        assert_eq!(
            instrument_id_from_url(url),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
    }

    #[test]
    fn empty_string_returns_none() {
        assert_eq!(instrument_id_from_url(""), None);
    }

    #[test]
    fn bare_uuid_no_slashes_returns_that_uuid() {
        let uuid = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        assert_eq!(instrument_id_from_url(uuid), Some(uuid));
    }

    #[test]
    fn rejects_noncanonical_instrument_paths() {
        assert_eq!(
            instrument_id_from_url("https://api.robinhood.com/instruments/"),
            None
        );
        assert_eq!(
            instrument_id_from_url(
                "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/history/"
            ),
            None
        );
    }
}
