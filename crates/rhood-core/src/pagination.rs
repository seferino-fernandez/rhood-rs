//! Generic paginated response wrappers for the Robinhood API.
//!
//! Robinhood's API returns results in paginated envelopes. These structs
//! capture the common shapes so endpoint methods can deserialize them
//! generically.

use serde::Deserialize;

/// A paginated API response containing a page of results and optional
/// URLs for the next and previous pages.
#[derive(Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    /// The items in this page.
    pub results: Vec<T>,
    /// URL of the next page, or `None` if this is the last page.
    pub next: Option<String>,
    /// URL of the previous page, or `None` if this is the first page.
    pub previous: Option<String>,
}

/// A non-paginated API response containing a flat list of results.
#[derive(Debug, Deserialize)]
pub struct ResultsResponse<T> {
    /// The items returned by the API.
    pub results: Vec<T>,
}

/// A cursor-paginated API response.
///
/// Unlike [`PaginatedResponse`], the `next` field contains a cursor token
/// (not a full URL). The caller re-requests the same base URL with
/// `cursor=<token>` appended to the query parameters.
#[derive(Debug, Deserialize)]
pub struct CursorPaginatedResponse<T> {
    /// The items in this page.
    pub results: Vec<T>,
    /// Cursor token for the next page, or `None` if this is the last page.
    pub next: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::CursorPaginatedResponse;

    #[test]
    fn cursor_paginated_response_deserializes_with_next() {
        let json = r#"{"results": [1, 2, 3], "next": "cursor_abc123"}"#;
        let resp: CursorPaginatedResponse<i32> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results, vec![1, 2, 3]);
        assert_eq!(resp.next.as_deref(), Some("cursor_abc123"));
    }

    #[test]
    fn cursor_paginated_response_deserializes_without_next() {
        let json = r#"{"results": [4, 5]}"#;
        let resp: CursorPaginatedResponse<i32> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results, vec![4, 5]);
        assert!(resp.next.is_none());
    }

    #[test]
    fn cursor_paginated_response_deserializes_null_next() {
        let json = r#"{"results": [], "next": null}"#;
        let resp: CursorPaginatedResponse<i32> = serde_json::from_str(json).unwrap();
        assert!(resp.results.is_empty());
        assert!(resp.next.is_none());
    }
}
