An MCP server for the Robinhood brokerage API. Tools are grouped by domain.

**Stocks** - `get_latest_prices` is the compact choice when you only need one current trade price per symbol; use `get_stock_quotes` for full quote detail. `get_stock_history` and `get_fundamentals` provide other market data; `get_open_orders` and `get_all_stock_orders` provide stock order history.

**Options** - To value held contracts, page through `get_option_positions`, collect the `option` URLs from its `items`, then call `get_option_quotes` with `instrument_urls`. To discover an unheld equity contract, use `get_option_chain` -> `find_options` -> `get_option_quotes` with `symbol` and `contracts` / `place_option_order`. `get_option_quotes` returns live bid/ask, Greeks, and volume; `get_option_orders` and `get_open_option_orders` cover order history.

**Futures** - `get_futures_contract` to look up contracts, `get_futures_quotes` for real-time quotes, `get_all_futures_orders` for order history, `get_futures_account` to discover the futures account ID.

**Indices** - `get_index_quotes` (SPX, NDX, VIX, RUT, XSP), `get_index_option_chain` for chain metadata, `find_index_options` to search contracts.

**IPO Access** - `get_ipo_access_list` returns the authenticated user's raw offering list or current empty state. Use its instrument IDs with `get_ipo_access_cards` for raw offering cards. Summary, allocation, receipt, and order-entry view models are not exposed because their populated contracts are unverified.

**Account** - Call `get_accounts` to discover canonical account numbers. Account-sensitive tools auto-select only when exactly one account exists; otherwise pass `account_number`. Use `get_positions`, `get_all_positions`, `get_portfolio`, `get_account_profile`, and `get_account_summary` for selected-account state. Page through `get_documents` for aggregate statements, tax forms, and trade confirmations.

**Income** - `get_dividends` and `get_total_dividends` for dividend history; `get_interest_payments` for sweep/interest; page through `get_transfers` for unified ACH / wire / debit-card movement. Paginated tools return `items` and `next_cursor`; pass a non-null cursor back unchanged until it becomes null.

**Research** - `get_earnings`, `get_ratings`, `get_news`, `get_splits`, `get_tags`.

**Recurring investments** - `get_recurring_investments`, `create_recurring_investment`, `update_recurring_investment`, `cancel_recurring_investment`, `get_next_investment_date`.

**Watchlists** - `get_watchlists`, `get_watchlist_items`, `add_to_watchlist`, `remove_from_watchlist`.

**User** - `get_user_profile`, `get_day_trades` (recent day trades and pattern-day-trader status).

**Market** - `get_markets`, `get_market_hours`, `get_market_today_hours`, `get_daily_movers` (top 20 daily movers).

**Orders (human-confirmed)** - `place_stock_order` resolves the account and fixes the instrument, share quantity, quote collar, client reference, and form version before MCP elicitation. The accepted retry submits that exact sealed preparation once. It expires after 60 seconds. Dollar buys require at least $1 and good-for-day duration and are converted from the ask quote to a six-decimal share estimate. A timeout returns `submission_uncertain`; reconcile open and recent orders before retrying. `place_option_order` also seals the selected account. For options, always set both `side` and `position_effect`: buy + open is buy-to-open, sell + close is sell-to-close, sell + open is sell-to-open, and buy + close is buy-to-close. Cancel an open stock order with `cancel_order`, or an open option order with `cancel_option_order`. In read-only mode, all order tools are disabled.
