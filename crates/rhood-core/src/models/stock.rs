//! Stock-related model types for the Robinhood API.
//!
//! Contains structs for stock quotes, fundamentals, historical candlestick
//! data, instruments, and associated configuration enums.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Represents a real-time stock quote from the Robinhood API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQuote {
    /// Current ask price for the stock.
    pub ask_price: Option<String>,
    /// Number of shares available at the ask price.
    pub ask_size: Option<i64>,
    /// Timestamp of the ask price from the venue.
    pub venue_ask_time: Option<String>,
    /// Current bid price for the stock.
    pub bid_price: Option<String>,
    /// Number of shares available at the bid price.
    pub bid_size: Option<i64>,
    /// Timestamp of the bid price from the venue.
    pub venue_bid_time: Option<String>,
    /// Price of the most recent trade.
    pub last_trade_price: Option<String>,
    /// Timestamp of the last trade from the venue.
    pub venue_last_trade_time: Option<String>,
    /// Price of the most recent extended-hours trade.
    pub last_extended_hours_trade_price: Option<String>,
    /// Price of the most recent non-regular-hours trade.
    pub last_non_reg_trade_price: Option<String>,
    /// Timestamp of the last non-regular-hours trade from the venue.
    pub venue_last_non_reg_trade_time: Option<String>,
    /// Closing price from the previous trading session.
    pub previous_close: Option<String>,
    /// Adjusted previous close price accounting for splits and dividends.
    pub adjusted_previous_close: Option<String>,
    /// Date of the previous close.
    pub previous_close_date: Option<String>,
    /// Ticker symbol for the stock.
    pub symbol: Option<String>,
    /// Indicates whether trading is currently halted.
    pub trading_halted: Option<bool>,
    /// Indicates whether the stock has been traded today.
    pub has_traded: Option<bool>,
    /// Source of the last trade price (e.g., "nls").
    pub last_trade_price_source: Option<String>,
    /// Source of the last non-regular-hours trade price.
    pub last_non_reg_trade_price_source: Option<String>,
    /// Timestamp when the quote was last updated.
    pub updated_at: Option<String>,
    /// URL of the associated instrument resource.
    pub instrument: Option<String>,
    /// UUID of the associated instrument.
    pub instrument_id: Option<String>,
    /// Instrument state (e.g., "active").
    pub state: Option<String>,
}

/// Represents fundamental financial data for a stock instrument.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Fundamentals {
    /// Opening price for the current trading day.
    pub open: Option<String>,
    /// Highest price reached during the current trading day.
    pub high: Option<String>,
    /// Lowest price reached during the current trading day.
    pub low: Option<String>,
    /// Trading volume for the current day.
    pub volume: Option<String>,
    /// Overnight trading volume.
    pub overnight_volume: Option<String>,
    /// Trading session bounds (e.g., "regular").
    pub bounds: Option<String>,
    /// Market date for this data (YYYY-MM-DD).
    pub market_date: Option<String>,
    /// Average trading volume over the last two weeks.
    pub average_volume_2_weeks: Option<String>,
    /// Overall average trading volume.
    pub average_volume: Option<String>,
    /// Average trading volume over the last 30 days.
    pub average_volume_30_days: Option<String>,
    /// Highest price reached in the last 52 weeks.
    pub high_52_weeks: Option<String>,
    /// Date of the 52-week high (YYYY-MM-DD).
    pub high_52_weeks_date: Option<String>,
    /// Annual dividend yield as a percentage.
    pub dividend_yield: Option<String>,
    /// Number of publicly tradable shares (float).
    pub float: Option<String>,
    /// Lowest price reached in the last 52 weeks.
    pub low_52_weeks: Option<String>,
    /// Date of the 52-week low (YYYY-MM-DD).
    pub low_52_weeks_date: Option<String>,
    /// Total market capitalization.
    pub market_cap: Option<String>,
    /// Price-to-book ratio.
    pub pb_ratio: Option<String>,
    /// Price-to-earnings ratio.
    pub pe_ratio: Option<String>,
    /// Total number of outstanding shares.
    pub shares_outstanding: Option<String>,
    /// Textual description of the company.
    pub description: Option<String>,
    /// URL of the associated instrument resource.
    pub instrument: Option<String>,
    /// Name of the company's chief executive officer.
    pub ceo: Option<String>,
    /// City where the company is headquartered.
    pub headquarters_city: Option<String>,
    /// State where the company is headquartered.
    pub headquarters_state: Option<String>,
    /// Market sector the company belongs to.
    pub sector: Option<String>,
    /// Industry classification of the company.
    pub industry: Option<String>,
    /// Total number of employees.
    pub num_employees: Option<i64>,
    /// Year the company was founded.
    pub year_founded: Option<i64>,
    /// Next dividend payable date (YYYY-MM-DD).
    pub payable_date: Option<String>,
    /// Ex-dividend date (YYYY-MM-DD).
    pub ex_dividend_date: Option<String>,
    /// Financial status indicator code.
    pub financial_status_indicator: Option<String>,
    /// Human-readable financial status description.
    pub financial_status_description: Option<String>,
    /// Ticker symbol for the stock.
    pub symbol: Option<String>,
}

/// Represents a single candlestick data point in a historical price series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    /// Timestamp marking the start of the candle period.
    pub begins_at: Option<String>,
    /// Opening price for the candle period.
    pub open_price: Option<String>,
    /// Closing price for the candle period.
    pub close_price: Option<String>,
    /// Highest price reached during the candle period.
    pub high_price: Option<String>,
    /// Lowest price reached during the candle period.
    pub low_price: Option<String>,
    /// Number of shares traded during the candle period.
    pub volume: Option<i64>,
    /// Trading session indicator (e.g., regular, pre-market, after-hours).
    pub session: Option<String>,
    /// Indicates whether the data point was interpolated due to missing data.
    pub interpolated: Option<bool>,
    /// Ticker symbol for the stock.
    pub symbol: Option<String>,
}

/// Per-account-type tradability entry from the instrument API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTypeTradability {
    /// Account type (e.g., "individual").
    pub account_type: Option<String>,
    /// Tradability status for this account type.
    pub account_type_tradability: Option<String>,
}

/// Represents a tradable instrument (stock) on Robinhood.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    /// Unique identifier for the instrument.
    pub id: Option<String>,
    /// API URL for this instrument resource.
    pub url: Option<String>,
    /// API URL for the quote resource.
    pub quote: Option<String>,
    /// API URL for the fundamentals resource.
    pub fundamentals: Option<String>,
    /// API URL for the splits resource.
    pub splits: Option<String>,
    /// Ticker symbol for the instrument.
    pub symbol: Option<String>,
    /// Simplified display name of the instrument.
    pub simple_name: Option<String>,
    /// Full legal name of the instrument.
    pub name: Option<String>,
    /// Indicates whether the instrument is currently tradeable.
    pub tradeable: Option<bool>,
    /// Tradability status string (e.g., "tradable", "untradable").
    pub tradability: Option<String>,
    /// URL of the market where the instrument is listed.
    pub market: Option<String>,
    /// Country code where the instrument is domiciled.
    pub country: Option<String>,
    /// Type of instrument (e.g., "stock", "etp", "adr").
    #[serde(rename = "type")]
    pub instrument_type: Option<String>,
    /// Identifier for the associated options chain, if tradable.
    pub tradable_chain_id: Option<String>,
    /// Fractional share tradability status.
    pub fractional_tradability: Option<String>,
    /// Current state of the instrument (e.g., "active").
    pub state: Option<String>,
    /// Bloomberg unique identifier.
    pub bloomberg_unique: Option<String>,
    /// Initial margin ratio for the instrument.
    pub margin_initial_ratio: Option<String>,
    /// Maintenance margin ratio.
    pub maintenance_ratio: Option<String>,
    /// Day trade margin ratio.
    pub day_trade_ratio: Option<String>,
    /// Date the instrument was first listed (YYYY-MM-DD).
    pub list_date: Option<String>,
    /// Minimum tick size for the instrument.
    pub min_tick_size: Option<String>,
    /// Robinhood-specific tradability status.
    pub rhs_tradability: Option<String>,
    /// Affiliate tradability status.
    pub affiliate_tradability: Option<String>,
    /// Short selling tradability status.
    pub short_selling_tradability: Option<String>,
    /// Default collar fraction for orders.
    pub default_collar_fraction: Option<String>,
    /// IPO access status.
    pub ipo_access_status: Option<String>,
    /// IPO access close-of-business deadline.
    pub ipo_access_cob_deadline: Option<String>,
    /// URL for the IPO S-1 filing.
    pub ipo_s1_url: Option<String>,
    /// URL for the IPO roadshow.
    pub ipo_roadshow_url: Option<String>,
    /// Whether the instrument is a SPAC.
    pub is_spac: Option<bool>,
    /// Whether the instrument is a test instrument.
    pub is_test: Option<bool>,
    /// Whether the IPO access supports DSP.
    pub ipo_access_supports_dsp: Option<bool>,
    /// IPO access start date.
    pub ipoa_start_date: Option<String>,
    /// Whether fractional trading is available in extended hours.
    pub extended_hours_fractional_tradability: Option<bool>,
    /// Reason for any internal trading halt.
    pub internal_halt_reason: Option<String>,
    /// Details of any internal trading halt.
    pub internal_halt_details: Option<String>,
    /// Sessions affected by an internal halt.
    pub internal_halt_sessions: Option<String>,
    /// Start time of an internal halt.
    pub internal_halt_start_time: Option<String>,
    /// End time of an internal halt.
    pub internal_halt_end_time: Option<String>,
    /// Source of an internal halt.
    pub internal_halt_source: Option<String>,
    /// All-day tradability status.
    pub all_day_tradability: Option<String>,
    /// Decimal precision for notional estimated quantities.
    pub notional_estimated_quantity_decimals: Option<i64>,
    /// Tax classification of the security.
    pub tax_security_type: Option<String>,
    /// Reserved buying power percentage for queued orders.
    pub reserved_buying_power_percent_queued: Option<String>,
    /// Reserved buying power percentage for immediate orders.
    pub reserved_buying_power_percent_immediate: Option<String>,
    /// OTC market tier.
    pub otc_market_tier: Option<String>,
    /// Whether a customer account review is required.
    pub car_required: Option<bool>,
    /// High-risk maintenance margin ratio.
    pub high_risk_maintenance_ratio: Option<String>,
    /// Low-risk maintenance margin ratio.
    pub low_risk_maintenance_ratio: Option<String>,
    /// Default preset percentage limit for orders.
    pub default_preset_percent_limit: Option<String>,
    /// Affiliate designation.
    pub affiliate: Option<String>,
    /// Per-account-type tradability entries.
    pub account_type_tradabilities: Option<Vec<AccountTypeTradability>>,
    /// Issuer type (e.g., "third_party").
    pub issuer_type: Option<String>,
}

/// Represents the time interval between candlestick data points in a historical query.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum HistoricalInterval {
    /// Five-minute interval between data points.
    #[cfg_attr(feature = "clap", value(name = "5minute"))]
    #[serde(rename = "5minute")]
    FiveMinute,
    /// Ten-minute interval between data points.
    #[cfg_attr(feature = "clap", value(name = "10minute"))]
    #[serde(rename = "10minute")]
    TenMinute,
    /// One-hour interval between data points.
    Hour,
    /// One-day interval between data points.
    Day,
    /// One-week interval between data points.
    Week,
}

impl HistoricalInterval {
    /// Returns the API query-string representation of this interval.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FiveMinute => "5minute",
            Self::TenMinute => "10minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
        }
    }
}

impl fmt::Display for HistoricalInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Represents the total time span of a historical data query.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum HistoricalSpan {
    /// Spans the current trading day.
    Day,
    /// Spans the past week.
    Week,
    /// Spans the past month.
    Month,
    /// Spans the past three months.
    #[cfg_attr(feature = "clap", value(name = "3month"))]
    #[serde(rename = "3month")]
    ThreeMonth,
    /// Spans the past year.
    Year,
    /// Spans the past five years.
    #[cfg_attr(feature = "clap", value(name = "5year"))]
    #[serde(rename = "5year")]
    FiveYear,
}

impl HistoricalSpan {
    /// Returns the API query-string representation of this span.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::ThreeMonth => "3month",
            Self::Year => "year",
            Self::FiveYear => "5year",
        }
    }
}

impl fmt::Display for HistoricalSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Represents which trading session bounds to include in historical data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum HistoricalBounds {
    /// Includes only regular market hours (9:30 AM -- 4:00 PM ET).
    Regular,
    /// Includes extended hours (pre-market and after-hours) in addition to regular hours.
    Extended,
    /// Includes all trading session data.
    Trading,
}

impl HistoricalBounds {
    /// Returns the API query-string representation of these bounds.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Extended => "extended",
            Self::Trading => "trading",
        }
    }
}

impl fmt::Display for HistoricalBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Configuration options for a historical data query.
#[derive(Debug, Clone)]
pub struct HistoricalOpts {
    /// Time interval between candlestick data points.
    pub interval: HistoricalInterval,
    /// Total time span of the query.
    pub span: HistoricalSpan,
    /// Trading session bounds to include.
    pub bounds: HistoricalBounds,
}

/// Represents a historical data response containing candlestick data points.
#[derive(Deserialize)]
pub struct HistoricalDataPoints {
    /// Ticker symbol the data points belong to.
    pub symbol: Option<String>,
    /// Ordered list of candlestick data points.
    pub data_points: Vec<Candle>,
}

/// Represents an alternative historical data response using the `historicals` field name.
#[derive(serde::Deserialize)]
pub struct HistoricalsResult {
    /// Ticker symbol the historicals belong to.
    pub symbol: Option<String>,
    /// Ordered list of candlestick data points.
    pub historicals: Vec<Candle>,
}

/// Nested outer wrapper for the index-values endpoint response.
///
/// The endpoint returns a doubly-nested `{status, data}` envelope:
/// `{"status":"SUCCESS","data":{"status":"SUCCESS","data":{...IndexQuote fields...}}}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexQuoteWrapper {
    /// Outer-envelope status string returned by Robinhood.
    pub status: Option<String>,
    /// Inner wrapper carrying the actual quote payload.
    pub data: IndexQuoteInner,
}

/// Inner wrapper carrying the actual [`IndexQuote`] payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexQuoteInner {
    /// Inner-envelope status string returned by Robinhood.
    pub status: Option<String>,
    /// The unwrapped index-quote fields.
    pub data: IndexQuote,
}

/// Real-time index quote.
///
/// Mirrors the fields the Robinhood `/marketdata/indexes/values/v1/{id}/`
/// endpoint actually returns. The earlier struct declared session-extreme
/// fields (`open_value`, `high_value`, `low_value`, `previous_close_value`)
/// that do NOT exist upstream — they were a guess that produced silent-null
/// responses because serde found zero matching fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexQuote {
    /// Index symbol (e.g., "SPX"). May be absent; backfilled from the
    /// instrument lookup by [`crate::RobinhoodClient::get_index_quote`].
    pub symbol: Option<String>,
    /// Current index value (as a string decimal).
    pub value: Option<String>,
    /// Venue-reported timestamp for the most recent tick.
    pub venue_timestamp: Option<String>,
    /// Identifier of the underlying index instrument.
    pub instrument_id: Option<String>,
    /// State string returned by Robinhood (often empty).
    pub state: Option<String>,
    /// Updated-at timestamp set by Robinhood's ingestion layer.
    pub updated_at: Option<String>,
}

/// Represents an index instrument (e.g., SPX, NDX, VIX) on Robinhood.
///
/// Unlike regular equity [`Instrument`]s which have a singular
/// `tradable_chain_id`, indexes use `tradable_chain_ids` (an array of
/// chain IDs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexInstrument {
    /// Unique identifier for the index.
    pub id: Option<String>,
    /// Index symbol (e.g., "SPX", "NDX", "VIX").
    pub symbol: Option<String>,
    /// List of tradable option chain IDs associated with this index.
    pub tradable_chain_ids: Option<Vec<String>>,
}

#[cfg(test)]
mod historical_serde_tests {
    use super::{HistoricalBounds, HistoricalInterval, HistoricalSpan};

    #[test]
    fn interval_serde_matches_as_str() {
        for v in [
            HistoricalInterval::FiveMinute,
            HistoricalInterval::TenMinute,
            HistoricalInterval::Hour,
            HistoricalInterval::Day,
            HistoricalInterval::Week,
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, format!("\"{}\"", v.as_str()));
            let back: HistoricalInterval = serde_json::from_str(&json).unwrap();
            assert_eq!(back.as_str(), v.as_str());
        }
    }

    #[test]
    fn span_serde_matches_as_str() {
        for v in [
            HistoricalSpan::Day,
            HistoricalSpan::Week,
            HistoricalSpan::Month,
            HistoricalSpan::ThreeMonth,
            HistoricalSpan::Year,
            HistoricalSpan::FiveYear,
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, format!("\"{}\"", v.as_str()));
        }
    }

    #[test]
    fn bounds_serde_matches_as_str() {
        for v in [
            HistoricalBounds::Regular,
            HistoricalBounds::Extended,
            HistoricalBounds::Trading,
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, format!("\"{}\"", v.as_str()));
        }
    }
}
