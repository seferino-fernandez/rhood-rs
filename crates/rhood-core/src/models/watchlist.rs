//! Watchlist model types.

use serde::{Deserialize, Serialize};

/// A user watchlist from the Robinhood midlands lists API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watchlist {
    /// Unique list identifier.
    pub id: Option<String>,
    /// Display name of the watchlist.
    pub display_name: Option<String>,
    /// Display description of the watchlist.
    pub display_description: Option<String>,
    /// Owner type (e.g., `"custom"`).
    pub owner_type: Option<String>,
    /// Owner user ID.
    pub owner: Option<String>,
    /// Emoji icon for the watchlist.
    pub icon_emoji: Option<String>,
    /// Number of items in the watchlist.
    pub item_count: Option<i64>,
    /// Whether the user follows this watchlist.
    pub followed: Option<bool>,
    /// Whether the list is expanded by default in the UI.
    pub default_expanded: Option<bool>,
    /// Read permission level (e.g., `"private"`).
    pub read_permission: Option<String>,
    /// Sort direction for child items (e.g., `"ascending"`).
    pub child_sort_direction: Option<String>,
    /// Sort order for child items (e.g., `"custom"`).
    pub child_sort_order: Option<String>,
    /// Object types allowed in this watchlist.
    pub allowed_object_types: Option<Vec<String>>,
    /// Child info with item type and inline children.
    pub child_info: Option<WatchlistChildInfo>,
    /// IDs of parent lists.
    pub parent_lists: Option<Vec<String>>,
    /// IDs of related lists.
    pub related_lists: Option<Vec<String>>,
    /// Hero images for the list (schema varies).
    pub hero_images: Option<serde_json::Value>,
    /// Timestamp when the watchlist was created.
    pub created_at: Option<String>,
    /// Timestamp when the watchlist was last updated.
    pub updated_at: Option<String>,
}

/// Child info metadata for a watchlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistChildInfo {
    /// Type of children (e.g., `"item"`).
    pub child_type: Option<String>,
    /// Inline children (may be empty; full items come from the items endpoint).
    pub children: Option<Vec<WatchlistItem>>,
}

/// An enriched watchlist item from the discovery API.
///
/// Contains both the list membership metadata and live market data
/// for the instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistItem {
    /// Unique item identifier.
    pub id: Option<String>,
    /// Parent watchlist ID.
    pub list_id: Option<String>,
    /// Instrument UUID (object_id in the API).
    pub object_id: Option<String>,
    /// Object type (e.g., "instrument", "currency_pair").
    pub object_type: Option<String>,
    /// Owner type (e.g., "custom").
    pub owner_type: Option<String>,
    /// Sort weight within the list.
    pub weight: Option<String>,
    /// Ticker symbol.
    pub symbol: Option<String>,
    /// Company display name.
    pub name: Option<String>,
    /// Current price.
    pub price: Option<f64>,
    /// Current bid price.
    pub bid_price: Option<f64>,
    /// Current ask price.
    pub ask_price: Option<f64>,
    /// Previous session close price.
    pub previous_close: Option<f64>,
    /// Dollar change from previous close.
    pub one_day_dollar_change: Option<f64>,
    /// Percent change from previous close.
    pub one_day_percent_change: Option<f64>,
    /// Rolling dollar change.
    pub one_day_rolling_dollar_change: Option<f64>,
    /// Rolling percent change.
    pub one_day_rolling_percent_change: Option<f64>,
    /// Session high price.
    pub high: Option<f64>,
    /// Session low price.
    pub low: Option<f64>,
    /// Trading volume.
    pub volume: Option<f64>,
    /// Average trading volume.
    pub average_volume: Option<f64>,
    /// Market capitalization.
    pub market_cap: Option<f64>,
    /// 52-week high.
    pub high_52_weeks: Option<f64>,
    /// 52-week low.
    pub low_52_weeks: Option<f64>,
    /// Price-to-earnings ratio.
    pub pe_ratio: Option<f64>,
    /// Opening price.
    pub open_price: Option<f64>,
    /// Opening price direction.
    pub open_price_direction: Option<String>,
    /// Number of open positions the user holds.
    pub open_positions: Option<i64>,
    /// Whether the user currently holds this instrument.
    pub holdings: Option<bool>,
    /// Total return percentage for user's position.
    pub total_return_percentage: Option<f64>,
    /// Instrument state (e.g., "active").
    pub state: Option<String>,
    /// IPO access status.
    pub ipo_access_status: Option<String>,
    /// US tradability status.
    pub us_tradability: Option<String>,
    /// UK tradability status.
    pub uk_tradability: Option<String>,
    /// Timestamp when the item was added to the list.
    pub created_at: Option<String>,
    /// Timestamp when the item was last updated.
    pub updated_at: Option<String>,
}
