//! Bounded, cursor-based presentation of collections returned by MCP tools.
//!
//! Robinhood's collection clients currently collect every upstream page. This
//! module deliberately keeps the upstream continuation URL private and applies
//! a versioned MCP-local cursor to the collected results before serialization.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;

const CURSOR_VERSION: &str = "v1:";
const DEFAULT_PAGE_SIZE: usize = 50;
pub(super) const MAX_PAGE_SIZE: usize = 100;
const MAX_CURSOR_LENGTH: usize = 128;

/// A validated request for one bounded collection page.
pub(super) struct CollectionPager {
    scope: &'static str,
    offset: usize,
    limit: usize,
}

#[derive(Debug, Serialize)]
struct CollectionPage<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

impl CollectionPager {
    /// Validates paging inputs before a tool authenticates or contacts Robinhood.
    pub(super) fn new(
        scope: &'static str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Self, String> {
        let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(format!("limit must be between 1 and {MAX_PAGE_SIZE} items"));
        }

        let offset = cursor.map_or(Ok(0), |cursor| decode_cursor(cursor, scope))?;
        Ok(Self {
            scope,
            offset,
            limit,
        })
    }

    /// Serializes one page while preserving the collection's upstream ordering.
    pub(super) fn serialize<T: Serialize>(self, items: Vec<T>) -> Result<String, String> {
        let total = items.len();
        if self.offset > total {
            return Err(
                "cursor is no longer valid for this collection; restart without a cursor"
                    .to_string(),
            );
        }

        let end = self.offset.saturating_add(self.limit).min(total);
        let page_items = items
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect();
        let next_cursor = (end < total).then(|| encode_cursor(self.scope, end));
        let page = CollectionPage {
            items: page_items,
            next_cursor,
        };
        serde_json::to_string_pretty(&page).map_err(|error| error.to_string())
    }
}

fn encode_cursor(scope: &str, offset: usize) -> String {
    URL_SAFE_NO_PAD.encode(format!("{CURSOR_VERSION}{scope}:{offset}"))
}

fn decode_cursor(cursor: &str, expected_scope: &str) -> Result<usize, String> {
    let invalid =
        || "invalid cursor; pass next_cursor from the previous response unchanged".to_string();
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_LENGTH {
        return Err(invalid());
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|error| format!("{}: {error}", invalid()))?;
    let decoded =
        std::str::from_utf8(&decoded).map_err(|error| format!("{}: {error}", invalid()))?;
    let payload = decoded.strip_prefix(CURSOR_VERSION).ok_or_else(invalid)?;
    let (scope, offset) = payload.rsplit_once(':').ok_or_else(invalid)?;
    if scope != expected_scope {
        return Err(
            "cursor does not belong to this collection or filter; restart without a cursor"
                .to_string(),
        );
    }
    if offset.is_empty() || !offset.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    offset
        .parse::<usize>()
        .map_err(|error| format!("{}: {error}", invalid()))
}

#[cfg(test)]
mod tests {
    use super::{CollectionPager, MAX_PAGE_SIZE};
    use serde_json::Value;

    fn page(cursor: Option<&str>, limit: Option<usize>, items: Vec<u16>) -> Value {
        let pager = CollectionPager::new("numbers", cursor, limit).unwrap();
        let serialized = pager.serialize(items).unwrap();
        serde_json::from_str(&serialized).unwrap()
    }

    #[test]
    fn defaults_to_fifty_items_and_emits_a_continuation() {
        let result = page(None, None, (0..75).collect());
        assert_eq!(result["items"].as_array().unwrap().len(), 50);
        assert!(result["next_cursor"].as_str().is_some());
    }

    #[test]
    fn cursor_continues_from_the_absolute_offset() {
        let first = page(None, Some(2), vec![10, 20, 30, 40, 50]);
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = page(Some(cursor), Some(3), vec![10, 20, 30, 40, 50]);
        assert_eq!(second["items"], serde_json::json!([30, 40, 50]));
        assert_eq!(second["next_cursor"], Value::Null);
    }

    #[test]
    fn limit_may_change_between_pages_without_skipping_items() {
        let first = page(None, Some(1), vec![10, 20, 30]);
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = page(Some(cursor), Some(2), vec![10, 20, 30]);
        assert_eq!(second["items"], serde_json::json!([20, 30]));
    }

    #[test]
    fn validates_limits_before_paging() {
        assert!(CollectionPager::new("numbers", None, Some(0)).is_err());
        assert!(CollectionPager::new("numbers", None, Some(MAX_PAGE_SIZE + 1)).is_err());
        let pager = CollectionPager::new("numbers", None, Some(MAX_PAGE_SIZE)).unwrap();
        assert_eq!(pager.limit, MAX_PAGE_SIZE);
    }

    #[test]
    fn rejects_malformed_and_stale_cursors() {
        assert!(CollectionPager::new("numbers", Some("not-a-cursor"), None).is_err());
        assert!(CollectionPager::new("numbers", Some(&"x".repeat(129)), None).is_err());

        let first = page(None, Some(2), vec![10, 20, 30]);
        let cursor = first["next_cursor"].as_str().unwrap();
        assert!(CollectionPager::new("other-numbers", Some(cursor), None).is_err());

        let pager = CollectionPager::new("numbers", Some(cursor), None).unwrap();
        let error = pager.serialize(vec![10]).unwrap_err();
        assert!(error.contains("restart without a cursor"));
    }

    #[test]
    fn empty_collection_is_a_terminal_page() {
        let result = page(None, None, Vec::new());
        assert_eq!(result["items"], serde_json::json!([]));
        assert_eq!(result["next_cursor"], Value::Null);
    }
}
