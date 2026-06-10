//! Account-related model types for the Robinhood API.
//!
//! Contains structs representing user account profiles, portfolio summaries,
//! unified account summary data, and individual stock positions.

use serde::{Deserialize, Serialize};

use super::dividend::MoneyAmount;

/// Represents a Robinhood brokerage account profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountProfile {
    /// Unique account number assigned by Robinhood.
    pub account_number: Option<String>,
    /// Total buying power available in the account.
    pub buying_power: Option<String>,
    /// Cash balance in the account.
    pub cash: Option<String>,
    /// Cash amount currently reserved for pending orders.
    pub cash_held_for_orders: Option<String>,
    /// API URL for this account resource.
    pub url: Option<String>,
    /// API URL for the associated portfolio resource.
    pub portfolio: Option<String>,
    /// API URL for the positions endpoint.
    pub positions: Option<String>,
    /// Account type (e.g., "cash", "margin").
    #[serde(rename = "type")]
    pub account_type: Option<String>,
    /// Timestamp when the account was created.
    pub created_at: Option<String>,
    /// Timestamp when the account was last updated.
    pub updated_at: Option<String>,
    /// Brokerage account classification (e.g., "individual").
    pub brokerage_account_type: Option<String>,
    /// Whether the account is deactivated.
    pub deactivated: Option<bool>,
    /// Whether deposits are halted.
    pub deposit_halted: Option<bool>,
    /// Whether withdrawals are halted.
    pub withdrawal_halted: Option<bool>,
    /// Whether only position-closing trades are allowed.
    pub only_position_closing_trades: Option<bool>,
    /// Overnight buying power.
    pub onbp: Option<String>,
    /// Cash available for withdrawal.
    pub cash_available_for_withdrawal: Option<String>,
    /// Cash available for withdrawal without margin.
    pub cash_available_for_withdrawal_without_margin: Option<String>,
    /// Amount eligible for deposit cancellation.
    pub amount_eligible_for_deposit_cancellation: Option<String>,
    /// Uncleared deposit total.
    pub uncleared_deposits: Option<String>,
    /// Special memorandum account balance.
    pub sma: Option<String>,
    /// SMA amount reserved for pending orders.
    pub sma_held_for_orders: Option<String>,
    /// Unsettled funds balance.
    pub unsettled_funds: Option<String>,
    /// Unsettled debit balance.
    pub unsettled_debit: Option<String>,
    /// Buying power available for crypto trades.
    pub crypto_buying_power: Option<String>,
    /// Maximum early-access ACH deposit amount.
    pub max_ach_early_access_amount: Option<String>,
    /// Cash balances (null for margin accounts).
    pub cash_balances: Option<serde_json::Value>,
    /// Margin balance details.
    pub margin_balances: Option<MarginBalances>,
    /// Whether cash sweep is enabled.
    pub sweep_enabled: Option<bool>,
    /// Whether the account is enrolled in cash sweep.
    pub sweep_enrolled: Option<bool>,
    /// Instant deposit eligibility details.
    pub instant_eligibility: Option<InstantEligibility>,
    /// Options trading level (e.g., "option_level_2").
    pub option_level: Option<String>,
    /// Whether this is a Pinnacle account.
    pub is_pinnacle_account: Option<bool>,
    /// Numeric account number from RHS.
    pub rhs_account_number: Option<serde_json::Value>,
    /// Account state (e.g., "active").
    pub state: Option<String>,
    /// Active subscription ID.
    pub active_subscription_id: Option<String>,
    /// Whether the account is locked.
    pub locked: Option<bool>,
    /// Whether the account is permanently deactivated.
    pub permanently_deactivated: Option<bool>,
    /// Whether IPO access is restricted.
    pub ipo_access_restricted: Option<bool>,
    /// Reason IPO access is restricted.
    pub ipo_access_restricted_reason: Option<String>,
    /// Whether ACH debit reception is locked.
    pub received_ach_debit_locked: Option<bool>,
    /// Whether dividend reinvestment is enabled.
    pub drip_enabled: Option<bool>,
    /// Whether fractional share trading is available.
    pub eligible_for_fractionals: Option<bool>,
    /// Whether DRIP is available.
    pub eligible_for_drip: Option<bool>,
    /// Whether cash management is available.
    pub eligible_for_cash_management: Option<bool>,
    /// Whether cash management is enabled.
    pub cash_management_enabled: Option<bool>,
    /// Whether option trading on expiration day is enabled.
    pub option_trading_on_expiration_enabled: Option<bool>,
    /// Cash reserved for options collateral.
    pub cash_held_for_options_collateral: Option<String>,
    /// Whether fractional positions are closing-only.
    pub fractional_position_closing_only: Option<bool>,
    /// User ID associated with the account.
    pub user_id: Option<String>,
    /// Equity trading lock status.
    pub equity_trading_lock: Option<String>,
    /// Option trading lock status.
    pub option_trading_lock: Option<String>,
    /// Whether alternative data trading is disabled.
    pub disable_adt: Option<bool>,
    /// Account management type (e.g., "self_directed").
    pub management_type: Option<String>,
    /// Dynamic instant deposit limit.
    pub dynamic_instant_limit: Option<String>,
    /// Affiliate designation (e.g., "rhf").
    pub affiliate: Option<String>,
    /// Whether second trade suitability questionnaire is completed.
    pub second_trade_suitability_completed: Option<bool>,
    /// Whether the account has a linked futures account.
    pub has_futures_account: Option<bool>,
    /// Whether this is the default account.
    pub is_default: Option<bool>,
    /// Portfolio cash balance.
    pub portfolio_cash: Option<String>,
    /// API URL for the associated user resource.
    pub user: Option<String>,
    /// API URL for checking cash downgrade eligibility.
    pub can_downgrade_to_cash: Option<String>,
    /// Customer account review valid-until date.
    pub car_valid_until: Option<String>,
    /// Account nickname.
    pub nickname: Option<String>,
    /// Reference ID.
    pub ref_id: Option<String>,
    /// Whether this is the original account.
    pub is_original: Option<bool>,
    /// User's real instant deposit limit.
    pub user_real_instant_limit: Option<String>,
    /// User's dynamic instant deposit limit.
    pub user_dynamic_instant_limit: Option<String>,
}

/// Margin balance details for a Robinhood account.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarginBalances {
    /// Special memorandum account balance.
    pub sma: Option<String>,
    /// Day trade buying power reserved for pending orders.
    pub day_trade_buying_power_held_for_orders: Option<String>,
    /// Day trade buying power at the start of the day.
    pub start_of_day_dtbp: Option<String>,
    /// Overnight buying power reserved for pending orders.
    pub overnight_buying_power_held_for_orders: Option<String>,
    /// Whether margin leverage is enabled.
    pub leverage_enabled: Option<bool>,
    /// Unsettled funds balance.
    pub unsettled_funds: Option<String>,
    /// Unsettled debit balance.
    pub unsettled_debit: Option<String>,
    /// Cash reserved for crypto orders.
    pub cash_held_for_crypto_orders: Option<String>,
    /// Cash reserved for pending dividends.
    pub cash_held_for_dividends: Option<String>,
    /// Cash reserved for account restrictions.
    pub cash_held_for_restrictions: Option<String>,
    /// Cash reserved for options collateral.
    pub cash_held_for_options_collateral: Option<String>,
    /// Cash reserved for pending orders.
    pub cash_held_for_orders: Option<String>,
    /// Amount eligible for instant deposit.
    pub eligible_deposit_as_instant: Option<String>,
    /// Instant deposit amount used.
    pub instant_used: Option<String>,
    /// Outstanding margin interest.
    pub outstanding_interest: Option<String>,
    /// Pending debit card debits.
    pub pending_debit_card_debits: Option<String>,
    /// Settled amount currently borrowed.
    pub settled_amount_borrowed: Option<String>,
    /// Short cash balance.
    pub short_cash: Option<String>,
    /// Short cash reserved.
    pub short_cash_held: Option<String>,
    /// Short unsettled debit.
    pub short_unsettled_debit: Option<String>,
    /// Short unsettled credit.
    pub short_unsettled_credit: Option<String>,
    /// Uncleared deposit total.
    pub uncleared_deposits: Option<String>,
    /// Cash balance.
    pub cash: Option<String>,
    /// Cash held for Nummus restrictions.
    pub cash_held_for_nummus_restrictions: Option<String>,
    /// Cash available for withdrawal.
    pub cash_available_for_withdrawal: Option<String>,
    /// Unallocated margin cash.
    pub unallocated_margin_cash: Option<String>,
    /// Margin borrowing limit.
    pub margin_limit: Option<String>,
    /// Buying power available for crypto.
    pub crypto_buying_power: Option<String>,
    /// Day trade buying power.
    pub day_trade_buying_power: Option<String>,
    /// Whether day trade protection is enabled.
    pub day_trades_protection: Option<bool>,
    /// Overnight buying power at the start of the day.
    pub start_of_day_overnight_buying_power: Option<String>,
    /// Overnight buying power.
    pub overnight_buying_power: Option<String>,
    /// Overnight margin ratio.
    pub overnight_ratio: Option<String>,
    /// Day trade margin ratio.
    pub day_trade_ratio: Option<String>,
    /// Date the account was flagged as a pattern day trader.
    pub marked_pattern_day_trader_date: Option<String>,
    /// Expiry date for PDT classification.
    pub pattern_day_trader_expiry_date: Option<String>,
    /// Timestamp when the margin balances were created.
    pub created_at: Option<String>,
    /// Timestamp when the margin balances were last updated.
    pub updated_at: Option<String>,
    /// Portfolio cash balance.
    pub portfolio_cash: Option<String>,
    /// Gold equity requirement.
    pub gold_equity_requirement: Option<String>,
    /// Uncleared Nummus deposits.
    pub uncleared_nummus_deposits: Option<String>,
    /// Cash pending from options events.
    pub cash_pending_from_options_events: Option<String>,
    /// Pending deposit amount.
    pub pending_deposit: Option<String>,
    /// Funding hold balance.
    pub funding_hold_balance: Option<String>,
    /// Net moving cash.
    pub net_moving_cash: Option<String>,
    /// Margin withdrawal limit.
    pub margin_withdrawal_limit: Option<String>,
    /// Instant deposit amount allocated.
    pub instant_allocated: Option<String>,
    /// Whether this is the primary account.
    pub is_primary_account: Option<bool>,
    /// Whether the account is permanently flagged as PDT.
    pub is_pdt_forever: Option<bool>,
}

/// Instant deposit eligibility details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantEligibility {
    /// Eligibility state (e.g., "ok").
    pub state: Option<String>,
    /// Reason for current eligibility state.
    pub reason: Option<String>,
    /// Date when eligibility will be reinstated.
    pub reinstatement_date: Option<String>,
    /// Reversal details.
    pub reversal: Option<String>,
    /// Additional deposit needed for eligibility.
    pub additional_deposit_needed: Option<String>,
    /// Compliance email for major oak.
    pub compliance_user_major_oak_email: Option<String>,
    /// Timestamp when the eligibility record was created.
    pub created_at: Option<String>,
    /// Who created the record.
    pub created_by: Option<String>,
    /// Timestamp when the eligibility was last updated.
    pub updated_at: Option<String>,
}

/// Represents a portfolio summary for a Robinhood account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioProfile {
    /// API URL for this portfolio resource.
    pub url: Option<String>,
    /// API URL for the associated account.
    pub account: Option<String>,
    /// Date when the portfolio started.
    pub start_date: Option<String>,
    /// Total equity value of the portfolio.
    pub equity: Option<String>,
    /// Portfolio equity value during extended trading hours.
    pub extended_hours_equity: Option<String>,
    /// Total market value of all positions.
    pub market_value: Option<String>,
    /// Market value during extended trading hours.
    pub extended_hours_market_value: Option<String>,
    /// Portfolio equity during extended trading hours.
    pub extended_hours_portfolio_equity: Option<String>,
    /// Equity value at the last core-hours close.
    pub last_core_equity: Option<String>,
    /// Market value at the last core-hours close.
    pub last_core_market_value: Option<String>,
    /// Portfolio equity at the last core-hours close.
    pub last_core_portfolio_equity: Option<String>,
    /// Excess margin available for trading.
    pub excess_margin: Option<String>,
    /// Excess maintenance margin.
    pub excess_maintenance: Option<String>,
    /// Excess margin including uncleared deposits.
    pub excess_margin_with_uncleared_deposits: Option<String>,
    /// Excess maintenance margin including uncleared deposits.
    pub excess_maintenance_with_uncleared_deposits: Option<String>,
    /// Equity at the previous session close.
    pub equity_previous_close: Option<String>,
    /// Portfolio equity at the previous session close.
    pub portfolio_equity_previous_close: Option<String>,
    /// Adjusted equity at the previous session close.
    pub adjusted_equity_previous_close: Option<String>,
    /// Adjusted portfolio equity at the previous session close.
    pub adjusted_portfolio_equity_previous_close: Option<String>,
    /// Amount available for withdrawal.
    pub withdrawable_amount: Option<String>,
    /// Deposits not yet available for withdrawal.
    pub unwithdrawable_deposits: Option<String>,
    /// Grants not yet available for withdrawal.
    pub unwithdrawable_grants: Option<String>,
    /// Whether this is the primary account.
    pub is_primary_account: Option<bool>,
    /// Non-USD currency equity.
    pub non_usd_currency_equity: Option<String>,
}

/// Unified account summary from the Robinhood bonfire API.
///
/// Provides an at-a-glance view of buying power, equity, market value,
/// and cash across all asset classes (equities, crypto, options).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountSummary {
    /// Account number.
    pub account_number: Option<String>,
    /// RHS account number.
    pub rhs_account_number: Option<String>,
    /// Brokerage account type (e.g., "individual").
    pub brokerage_account_type: Option<String>,
    /// Account management type (e.g., "self_directed").
    pub management_type: Option<String>,
    /// Account nickname.
    pub nickname: Option<String>,
    /// Whether this is the original account.
    pub is_original: Option<bool>,
    /// Whether the account has a linked futures account.
    pub has_futures_account: Option<bool>,
    /// Timestamp when the account was created.
    pub created_at: Option<String>,
    /// Total buying power across all asset classes.
    pub account_buying_power: Option<MoneyAmount>,
    /// Cash available from instant deposits.
    pub cash_available_from_instant_deposits: Option<MoneyAmount>,
    /// Cash reserved for pending currency orders.
    pub cash_held_for_currency_orders: Option<MoneyAmount>,
    /// Cash reserved for pending dividends.
    pub cash_held_for_dividends: Option<MoneyAmount>,
    /// Cash reserved for pending equity orders.
    pub cash_held_for_equity_orders: Option<MoneyAmount>,
    /// Cash reserved for options collateral.
    pub cash_held_for_options_collateral: Option<MoneyAmount>,
    /// Cash reserved for all pending orders.
    pub cash_held_for_orders: Option<MoneyAmount>,
    /// Cash reserved for account restrictions.
    pub cash_held_for_restrictions: Option<MoneyAmount>,
    /// Crypto holdings summary.
    pub crypto: Option<serde_json::Value>,
    /// Buying power available for crypto trades.
    pub crypto_buying_power: Option<MoneyAmount>,
    /// Equities holdings summary.
    pub equities: Option<serde_json::Value>,
    /// Portfolio equity during extended trading hours.
    pub extended_hours_portfolio_equity: Option<MoneyAmount>,
    /// Instant deposit amount allocated.
    pub instant_allocated: Option<MoneyAmount>,
    /// Amount borrowed on margin.
    pub levered_amount: Option<MoneyAmount>,
    /// Whether the account is near a margin call.
    pub near_margin_call: Option<bool>,
    /// Buying power available for options trades.
    pub options_buying_power: Option<MoneyAmount>,
    /// Total portfolio equity at regular-hours close.
    pub portfolio_equity: Option<MoneyAmount>,
    /// Portfolio equity at the previous session close.
    pub portfolio_previous_close: Option<MoneyAmount>,
    /// Previous close value.
    pub previous_close: Option<MoneyAmount>,
    /// Portfolio equity during regular hours.
    pub regular_hours_portfolio_equity: Option<MoneyAmount>,
    /// Total equity across all holdings and cash.
    pub total_equity: Option<MoneyAmount>,
    /// Total extended-hours equity.
    pub total_extended_hours_equity: Option<MoneyAmount>,
    /// Total extended-hours market value.
    pub total_extended_hours_market_value: Option<MoneyAmount>,
    /// Total market value of all positions.
    pub total_market_value: Option<MoneyAmount>,
    /// Total regular-hours equity.
    pub total_regular_hours_equity: Option<MoneyAmount>,
    /// Total regular-hours market value.
    pub total_regular_hours_market_value: Option<MoneyAmount>,
    /// Total absolute market value.
    pub total_absolute_market_value: Option<MoneyAmount>,
    /// Cash not currently invested.
    pub uninvested_cash: Option<MoneyAmount>,
    /// Cash available for withdrawal.
    pub withdrawable_cash: Option<MoneyAmount>,
    /// Margin health status.
    pub margin_health: Option<MarginHealth>,
    /// Currency used for buying power display.
    pub buying_power_display_currency: Option<String>,
}

/// Margin health status from the unified account summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginHealth {
    /// Health state (e.g., "healthy").
    pub margin_health_state: Option<String>,
    /// Margin buffer ratio (e.g., "1.0000").
    pub margin_buffer: Option<String>,
    /// Margin buffer as a monetary amount.
    pub margin_buffer_amount: Option<MoneyAmount>,
}

/// Represents a stock position held in a Robinhood account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// API URL for the account that holds this position.
    pub account: Option<String>,
    /// API URL for the instrument this position represents.
    pub instrument: Option<String>,
    /// Ticker symbol, backfilled by resolving the instrument URL.
    pub symbol: Option<String>,
    /// Average cost basis per share.
    pub average_buy_price: Option<String>,
    /// Total number of shares held.
    pub quantity: Option<String>,
    /// Number of shares reserved for pending buy orders.
    pub shares_held_for_buys: Option<String>,
    /// Number of shares reserved for pending sell orders.
    pub shares_held_for_sells: Option<String>,
    /// Timestamp when the position was created.
    pub created_at: Option<String>,
    /// Timestamp when the position was last updated.
    pub updated_at: Option<String>,
}
