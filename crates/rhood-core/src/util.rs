//! Small shared helpers.

/// Extracts the trailing instrument UUID from a Robinhood instrument URL such
/// as `https://api.robinhood.com/instruments/<uuid>/`. Returns `None` if the
/// URL is empty or has no path segment.
pub fn instrument_id_from_url(url: &str) -> Option<&str> {
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
}
