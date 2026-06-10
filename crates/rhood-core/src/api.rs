//! Robinhood REST API path constants.
//!
//! These constants are the URL path segments appended to base URLs
//! via [`RobinhoodClient::api_url`](crate::client::RobinhoodClient::api_url),
//! [`RobinhoodClient::phoenix_url`](crate::client::RobinhoodClient::phoenix_url), and
//! [`RobinhoodClient::bonfire_url`](crate::client::RobinhoodClient::bonfire_url).

/// Robinhood API path constants organized by domain.
pub mod paths {
    // Auth / internal

    /// OAuth2 token endpoint.
    pub const TOKEN: &str = "/oauth2/token/";
    /// Pathfinder user machine creation endpoint for device verification.
    pub const PATHFINDER_USER_MACHINE: &str = "/pathfinder/user_machine/";
    /// Pathfinder inquiry polling endpoint for device verification.
    pub const PATHFINDER_INQUIRIES: &str = "/pathfinder/inquiries/";
    /// Push notification status polling endpoint.
    pub const PUSH_STATUS: &str = "/push/";
    /// SMS/email challenge response endpoint.
    pub const CHALLENGE: &str = "/challenge/";

    // Account

    /// Unified account summary suffix (appended to `/accounts/{account_number}/`).
    pub const ACCOUNT_SUMMARY_SUFFIX: &str = "/unified/";
    /// Account listing endpoint.
    pub const ACCOUNTS: &str = "/accounts/";
    /// Stock positions endpoint.
    pub const POSITIONS: &str = "/positions/";
    /// Portfolio summary endpoint.
    pub const PORTFOLIOS: &str = "/portfolios/";

    // Stocks

    /// Real-time stock quote endpoint.
    pub const QUOTES: &str = "/quotes/";
    /// Stock fundamentals endpoint.
    pub const FUNDAMENTALS: &str = "/fundamentals/";
    /// Historical stock price data endpoint.
    pub const HISTORICALS: &str = "/quotes/historicals/";
    /// Instrument lookup endpoint.
    pub const INSTRUMENTS: &str = "/instruments/";

    // Options

    /// Options instrument search endpoint.
    pub const OPTION_INSTRUMENTS: &str = "/options/instruments/";
    /// Options positions endpoint.
    pub const OPTION_POSITIONS: &str = "/options/positions/";
    /// Options chain endpoint.
    pub const OPTION_CHAINS: &str = "/options/chains/";
    /// Options market data (quotes, Greeks, volume) endpoint.
    pub const OPTION_MARKET_DATA: &str = "/marketdata/options/";

    // Orders

    /// Stock order endpoint (list, place, cancel).
    pub const STOCK_ORDERS: &str = "/orders/";
    /// Option order endpoint (list, place, cancel).
    pub const OPTION_ORDERS: &str = "/options/orders/";

    // Markets

    /// Market listing endpoint.
    pub const MARKETS: &str = "/markets/";
    /// Robinhood curated daily movers list ID.
    pub const DAILY_MOVERS_LIST_ID: &str = "eddbebe5-34cc-4df1-953c-d3e3cb55bc19";

    // Futures

    /// Futures contract lookup endpoint.
    pub const FUTURES_CONTRACTS: &str = "/arsenal/v1/futures/contracts/";
    /// Futures real-time quote endpoint.
    pub const FUTURES_QUOTES: &str = "/marketdata/futures/quotes/v1/";
    /// Futures account listing endpoint.
    pub const FUTURES_ACCOUNTS: &str = "/ceres/v1/accounts/";

    // Indexes

    /// Index instrument lookup endpoint.
    pub const INDEXES: &str = "/indexes/";
    /// Index real-time market data endpoint.
    pub const INDEX_MARKET_DATA: &str = "/marketdata/indexes/values/v1/";

    // Dividends & Interest

    /// Dividend history endpoint.
    pub const DIVIDENDS: &str = "/dividends/";
    /// Interest/sweep payments endpoint.
    pub const SWEEPS: &str = "/accounts/sweeps/";

    // Transfers

    /// Unified transfers endpoint (ACH, wire, debit card).
    pub const UNIFIED_TRANSFERS: &str = "/paymenthub/unified_transfers/";

    // Recurring Investments

    /// Recurring investment schedules endpoint.
    pub const RECURRING_SCHEDULES: &str = "/recurring_schedules/";

    // Research & Discovery

    /// Earnings data endpoint.
    pub const EARNINGS: &str = "/marketdata/earnings/";
    /// Analyst ratings endpoint (requires instrument ID suffix).
    pub const RATINGS: &str = "/midlands/ratings/";
    /// Market news endpoint.
    pub const NEWS: &str = "/midlands/news/";
    /// Tag-based instrument discovery endpoint.
    pub const TAGS: &str = "/midlands/tags/tag/";

    // Watchlists

    /// Watchlist listing endpoint (midlands v2 lists).
    pub const WATCHLISTS: &str = "/midlands/lists/";
    /// Watchlist items endpoint (discovery API, read-only).
    pub const WATCHLIST_ITEMS: &str = "/discovery/lists/items/";
    /// Watchlist items write endpoint (midlands bulk-edit; create items).
    pub const WATCHLIST_ITEMS_WRITE: &str = "/midlands/lists/items/";

    // User & Account Info

    /// User profile endpoint.
    pub const USER: &str = "/user/";
    /// Recent day trades endpoint (requires account ID suffix).
    pub const DAY_TRADES: &str = "/accounts/";
    /// Account documents endpoint.
    pub const DOCUMENTS: &str = "/documents/";
}

#[cfg(test)]
mod tests {
    use super::paths;

    #[test]
    fn all_paths_start_with_slash() {
        let all = [
            paths::TOKEN,
            paths::PATHFINDER_USER_MACHINE,
            paths::PATHFINDER_INQUIRIES,
            paths::PUSH_STATUS,
            paths::CHALLENGE,
            paths::ACCOUNT_SUMMARY_SUFFIX,
            paths::ACCOUNTS,
            paths::POSITIONS,
            paths::PORTFOLIOS,
            paths::QUOTES,
            paths::FUNDAMENTALS,
            paths::HISTORICALS,
            paths::INSTRUMENTS,
            paths::OPTION_INSTRUMENTS,
            paths::OPTION_POSITIONS,
            paths::OPTION_CHAINS,
            paths::OPTION_MARKET_DATA,
            paths::STOCK_ORDERS,
            paths::OPTION_ORDERS,
            paths::MARKETS,
            paths::FUTURES_CONTRACTS,
            paths::FUTURES_QUOTES,
            paths::FUTURES_ACCOUNTS,
            paths::INDEXES,
            paths::INDEX_MARKET_DATA,
            paths::DIVIDENDS,
            paths::SWEEPS,
            paths::UNIFIED_TRANSFERS,
            paths::RECURRING_SCHEDULES,
            paths::EARNINGS,
            paths::RATINGS,
            paths::NEWS,
            paths::TAGS,
            paths::WATCHLISTS,
            paths::WATCHLIST_ITEMS,
            paths::USER,
            paths::DAY_TRADES,
            paths::DOCUMENTS,
        ];
        for path in all {
            assert!(path.starts_with('/'), "{path} does not start with '/'");
        }
    }
}
