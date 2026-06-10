use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::watchlist::{Watchlist, WatchlistItem};
use crate::pagination::PaginatedResponse;
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Fetches all user watchlists.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_watchlists(&self) -> Result<Vec<Watchlist>> {
        let resp: PaginatedResponse<Watchlist> = self
            .get_with_params(
                &self.api_url(paths::WATCHLISTS),
                &[("owner_type", "custom")],
            )
            .await?;
        Ok(resp.results)
    }

    /// Fetches a single watchlist by display name or ID.
    ///
    /// Tries display name first (case-insensitive), then falls back to exact
    /// ID match. This allows users to look up watchlists with emoji names by ID.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if no watchlist matches.
    pub async fn get_watchlist(&self, name_or_id: &str) -> Result<Watchlist> {
        let lists = self.get_watchlists().await?;
        lists
            .iter()
            .find(|list| {
                list.display_name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(name_or_id))
            })
            .or_else(|| {
                lists
                    .iter()
                    .find(|list| list.id.as_deref() == Some(name_or_id))
            })
            .cloned()
            .ok_or_else(|| {
                RhoodError::InvalidParameter(format!("Watchlist not found: {name_or_id}"))
            })
    }

    /// Fetches the items in a watchlist by name or ID.
    ///
    /// Uses the `/discovery/lists/items/` endpoint which returns enriched
    /// items with live market data (price, change, volume, etc.).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the watchlist contains
    /// only option strategies (the discovery API does not support them).
    /// Returns an error if the watchlist is not found or the items cannot be fetched.
    pub async fn get_watchlist_items(&self, name_or_id: &str) -> Result<Vec<WatchlistItem>> {
        let list = self.get_watchlist(name_or_id).await?;

        let is_options_only = list.allowed_object_types.as_ref().is_some_and(|types| {
            types
                .iter()
                .all(|object_type| object_type == "option_strategy")
        });
        if is_options_only {
            let name = list.display_name.as_deref().unwrap_or("this watchlist");
            return Err(RhoodError::InvalidParameter(format!(
                "'{name}' is an options watchlist and cannot be listed via the discovery API"
            )));
        }

        let list_id = list
            .id
            .as_deref()
            .ok_or_else(|| RhoodError::InvalidParameter("Watchlist missing ID".into()))?;
        let resp: PaginatedResponse<WatchlistItem> = self
            .get_with_params(
                &self.api_url(paths::WATCHLIST_ITEMS),
                &[("list_id", list_id)],
            )
            .await?;
        Ok(resp.results)
    }

    /// Adds symbols to a watchlist.
    ///
    /// Resolves each symbol to its instrument ID, then adds them all in a
    /// single bulk write. Requires writable mode.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Returns [`RhoodError::InvalidSymbol`] if any symbol cannot be resolved.
    pub async fn add_to_watchlist(&self, name: &str, symbols: &[&str]) -> Result<()> {
        self.require_writable()?;
        let list = self.get_watchlist(name).await?;
        let list_id = list
            .id
            .clone()
            .ok_or_else(|| RhoodError::InvalidParameter("Watchlist missing ID".into()))?;

        let mut object_ids = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            let instrument = self
                .cached_instrument(symbol)
                .await?
                .ok_or_else(|| RhoodError::InvalidSymbol((*symbol).to_string()))?;
            let instrument_id = instrument
                .id
                .clone()
                .ok_or_else(|| RhoodError::InvalidSymbol((*symbol).to_string()))?;
            object_ids.push(instrument_id);
        }
        if object_ids.is_empty() {
            return Ok(());
        }
        self.bulk_watchlist_edit(&list_id, &object_ids, "create")
            .await?;
        Ok(())
    }

    /// Removes symbols from a watchlist.
    ///
    /// Fetches the enriched watchlist items, matches the requested symbols
    /// (case-insensitive), then removes the matches in a single bulk write.
    /// Requires writable mode.
    ///
    /// Returns the number of symbols that were actually found and removed.
    /// Symbols not present in the watchlist are silently skipped and do not
    /// count toward the return value.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Returns [`RhoodError::InvalidParameter`] if the watchlist is not found or
    /// is missing an ID.
    pub async fn remove_from_watchlist(&self, name: &str, symbols: &[&str]) -> Result<usize> {
        self.require_writable()?;
        let list = self.get_watchlist(name).await?;
        let list_id = list
            .id
            .clone()
            .ok_or_else(|| RhoodError::InvalidParameter("Watchlist missing ID".into()))?;
        let items = self.get_watchlist_items(name).await?;

        let object_ids: Vec<String> = symbols
            .iter()
            .filter_map(|symbol| {
                items
                    .iter()
                    .find(|item| {
                        item.symbol
                            .as_deref()
                            .is_some_and(|item_symbol| item_symbol.eq_ignore_ascii_case(symbol))
                    })
                    .and_then(|item| item.object_id.clone())
            })
            .collect();

        if object_ids.is_empty() {
            return Ok(0);
        }
        self.bulk_watchlist_edit(&list_id, &object_ids, "delete")
            .await?;
        Ok(object_ids.len())
    }

    /// Posts a bulk create/delete edit to the midlands lists write endpoint.
    ///
    /// The endpoint expects a body keyed by the list ID whose value is an array
    /// of `{object_type, object_id, operation}` ops. The per-list nested
    /// collection (`/midlands/lists/{id}/items/`) is not a usable write target:
    /// POST 404s and DELETE on the nested item path also 404s with an HTML body.
    async fn bulk_watchlist_edit(
        &self,
        list_id: &str,
        object_ids: &[String],
        operation: &str,
    ) -> Result<()> {
        let payload = bulk_watchlist_payload(list_id, object_ids, operation);
        let _: serde_json::Value = self
            .post_json(&self.api_url(paths::WATCHLIST_ITEMS_WRITE), &payload)
            .await?;
        Ok(())
    }
}

/// Builds the midlands bulk-edit request body: an object keyed by the list ID
/// whose value is an array of `{object_type, object_id, operation}` ops.
///
/// This exact keyed shape is required — a top-level array or an
/// `{list_id, items}` object are both rejected by the API.
fn bulk_watchlist_payload(
    list_id: &str,
    object_ids: &[String],
    operation: &str,
) -> serde_json::Value {
    let ops: Vec<serde_json::Value> = object_ids
        .iter()
        .map(|object_id| {
            serde_json::json!({
                "object_type": "instrument",
                "object_id": object_id,
                "operation": operation,
            })
        })
        .collect();
    serde_json::json!({ list_id: ops })
}

#[cfg(test)]
mod tests {
    use super::bulk_watchlist_payload;
    use crate::models::watchlist::{Watchlist, WatchlistItem};

    #[test]
    fn bulk_watchlist_payload_is_keyed_by_list_id() {
        // The midlands bulk-edit endpoint requires the body to be an object
        // keyed by the list ID, mapping to an array of ops. A top-level array
        // ("failed operations") or an {list_id, items} object ("Expected a
        // list of items but got type str") are both rejected live. Confirmed
        // working against the real API for both create and delete.
        let list_id = "2eda131c-04b4-4cbf-a0fa-4fcd48a84c5d";
        let object_ids = vec![
            "ad059c69-0c1c-4c6b-8322-f53f1bbd69d4".to_string(),
            "450dfc6d-5510-4d40-abfb-f633b7d9be3e".to_string(),
        ];
        let payload = bulk_watchlist_payload(list_id, &object_ids, "create");

        let ops = payload[list_id].as_array().expect("keyed array of ops");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0]["object_type"], "instrument");
        assert_eq!(ops[0]["object_id"], "ad059c69-0c1c-4c6b-8322-f53f1bbd69d4");
        assert_eq!(ops[0]["operation"], "create");
        assert_eq!(ops[1]["object_id"], "450dfc6d-5510-4d40-abfb-f633b7d9be3e");
        // No top-level `items` / `list_id` keys: the list_id IS the key.
        assert!(payload.get("items").is_none());
        assert!(payload.get("list_id").is_none());
    }

    #[test]
    fn bulk_watchlist_payload_supports_delete() {
        let payload = bulk_watchlist_payload("L1", &["I1".to_string()], "delete");
        assert_eq!(payload["L1"][0]["operation"], "delete");
    }

    #[test]
    fn watchlist_deserializes_real_api_shape() {
        let json = r#"{
            "child_sort_direction": "ascending",
            "child_sort_order": "custom",
            "created_at": "2023-06-08T18:09:06.615545+00:00",
            "display_description": null,
            "display_name": "My First List",
            "id": "2eda131c-04b4-4cbf-a0fa-4fcd48a84c5d",
            "owner_type": "custom",
            "parent_lists": [],
            "read_permission": "private",
            "updated_at": "2023-06-08T18:09:06.638995+00:00",
            "allowed_object_types": ["currency_pair", "futures", "index", "instrument"],
            "icon_emoji": "⚡",
            "owner": "141fb69c-72c4-49c5-994b-1251039c8648",
            "item_count": 16,
            "child_info": {
                "child_type": "item",
                "children": []
            },
            "followed": true,
            "default_expanded": true,
            "related_lists": [],
            "hero_images": null
        }"#;
        let list: Watchlist = serde_json::from_str(json).unwrap();
        assert_eq!(list.display_name.as_deref(), Some("My First List"));
        assert_eq!(
            list.id.as_deref(),
            Some("2eda131c-04b4-4cbf-a0fa-4fcd48a84c5d")
        );
        assert_eq!(list.owner_type.as_deref(), Some("custom"));
        assert_eq!(list.icon_emoji.as_deref(), Some("⚡"));
        assert_eq!(list.item_count, Some(16));
        assert_eq!(list.followed, Some(true));
        assert_eq!(list.allowed_object_types.as_ref().unwrap().len(), 4);
        let child_info = list.child_info.unwrap();
        assert_eq!(child_info.child_type.as_deref(), Some("item"));
        assert_eq!(child_info.children.unwrap().len(), 0);
    }

    #[test]
    fn watchlist_handles_missing_fields() {
        let json = r#"{"display_name": "Empty"}"#;
        let list: Watchlist = serde_json::from_str(json).unwrap();
        assert_eq!(list.display_name.as_deref(), Some("Empty"));
        assert!(list.child_info.is_none());
        assert!(list.id.is_none());
    }

    #[test]
    fn watchlist_item_deserializes_real_api_shape() {
        let json = r#"{
            "created_at": "2023-06-08T18:09:06.618468Z",
            "id": "57f6d7f4-0824-435b-9428-1f483bfc7c28",
            "list_id": "2eda131c-04b4-4cbf-a0fa-4fcd48a84c5d",
            "object_id": "e39ed23a-7bd1-4587-b060-71988d9ef483",
            "object_type": "instrument",
            "owner_type": "custom",
            "updated_at": "2023-06-08T18:09:06.618479Z",
            "weight": "1.00000",
            "market_cap": 1287984697449.2463,
            "high": 364.5,
            "low": 339.9101,
            "volume": 78838049.0,
            "average_volume": 67016803.259264,
            "high_52_weeks": 498.83,
            "low_52_weeks": 217.8,
            "pe_ratio": 322.345174,
            "name": "Tesla",
            "open_positions": 0,
            "symbol": "TSLA",
            "state": "active",
            "price": 341.87,
            "bid_price": 341.8,
            "ask_price": 341.9,
            "previous_close": 346.65,
            "one_day_dollar_change": -4.78,
            "one_day_percent_change": -1.3789124477138324,
            "holdings": false
        }"#;
        let item: WatchlistItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.symbol.as_deref(), Some("TSLA"));
        assert_eq!(item.name.as_deref(), Some("Tesla"));
        assert_eq!(item.object_type.as_deref(), Some("instrument"));
        assert_eq!(
            item.object_id.as_deref(),
            Some("e39ed23a-7bd1-4587-b060-71988d9ef483")
        );
        assert!((item.price.unwrap() - 341.87).abs() < 0.01);
        assert!((item.one_day_percent_change.unwrap() - (-1.3789124477138324)).abs() < 0.001);
        assert_eq!(item.holdings, Some(false));
        assert_eq!(item.open_positions, Some(0));
    }
}
