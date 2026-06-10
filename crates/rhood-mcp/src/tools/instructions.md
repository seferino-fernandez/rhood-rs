An MCP server for the Robinhood brokerage API. Tools are grouped by domain.

**Stocks** — `get_stock_quotes`, `get_stock_history`, `get_fundamentals`, `get_latest_prices` for market data; `get_open_orders` and `get_all_stock_orders` for stock order history.

**Options** — `get_option_chain` and `get_option_quotes` (live bid/ask, Greeks, volume) for chain data; `get_option_positions`, `get_option_orders`, `get_open_option_orders` for positions and order history.

**Futures** — `get_futures_contract` to look up contracts, `get_futures_quotes` for real-time quotes, `get_all_futures_orders` for order history, `get_futures_account` to discover the futures account ID.

**Indices** — `get_index_quotes` (SPX, NDX, VIX, RUT, XSP), `get_index_option_chain` for chain metadata, `find_index_options` to search contracts.

**Account** — `get_positions`, `get_all_positions`, `get_portfolio`, `get_account_profile`, `get_account_summary` for state; `get_documents` for statements, tax forms, and trade confirmations.

**Income** — `get_dividends` and `get_total_dividends` for dividend history; `get_interest_payments` for sweep/interest; `get_transfers` for unified ACH / wire / debit-card movement.

**Research** — `get_earnings`, `get_ratings`, `get_news`, `get_splits`, `get_tags`.

**Recurring investments** — `get_recurring_investments`, `create_recurring_investment`, `update_recurring_investment`, `cancel_recurring_investment`, `get_next_investment_date`.

**Watchlists** — `get_watchlists`, `get_watchlist_items`, `add_to_watchlist`, `remove_from_watchlist`.

**User** — `get_user_profile`, `get_day_trades` (recent day trades and pattern-day-trader status).

**Market** — `get_markets`, `get_market_hours`, `get_market_today_hours`, `get_daily_movers` (top 20 daily movers).

**Orders (two-step)** — Stage with `place_stock_order` or `place_option_order`, then submit with `confirm_order`. Cancel an open stock order with `cancel_order`, an open option order with `cancel_option_order`. In read-only mode, all order tools are disabled.
