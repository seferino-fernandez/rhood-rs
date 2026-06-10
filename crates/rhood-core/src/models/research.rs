//! Research and discovery data types.
//!
//! Models for earnings reports, analyst ratings, news articles,
//! stock splits, and tag-based discovery.

use serde::{Deserialize, Serialize};

/// Earnings report data for a company.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Earnings {
    /// Ticker symbol.
    pub symbol: Option<String>,
    /// Instrument URL.
    pub instrument: Option<String>,
    /// Fiscal year.
    pub year: Option<i32>,
    /// Fiscal quarter (1-4).
    pub quarter: Option<i32>,
    /// Earnings per share data.
    pub eps: Option<EarningsEps>,
    /// Earnings report scheduling.
    pub report: Option<EarningsReport>,
}

/// Earnings per share estimate and actual values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsEps {
    /// Consensus EPS estimate.
    pub estimate: Option<String>,
    /// Actual reported EPS.
    pub actual: Option<String>,
}

/// Earnings report date and timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsReport {
    /// Report date (YYYY-MM-DD).
    pub date: Option<String>,
    /// Report timing relative to market hours ("am" or "pm").
    pub timing: Option<String>,
    /// Whether the report date has been verified by the company.
    pub verified: Option<bool>,
}

/// Analyst rating data for an instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rating {
    /// Instrument ID.
    pub instrument_id: Option<String>,
    /// Aggregated rating summary.
    pub summary: Option<RatingSummary>,
    /// Individual analyst ratings.
    pub ratings: Option<Vec<RatingEntry>>,
}

/// Aggregated analyst rating counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingSummary {
    /// Number of buy ratings.
    pub num_buy_ratings: Option<i32>,
    /// Number of hold ratings.
    pub num_hold_ratings: Option<i32>,
    /// Number of sell ratings.
    pub num_sell_ratings: Option<i32>,
}

impl RatingSummary {
    /// Total number of analyst ratings.
    pub fn total(&self) -> i32 {
        self.num_buy_ratings.unwrap_or(0)
            + self.num_hold_ratings.unwrap_or(0)
            + self.num_sell_ratings.unwrap_or(0)
    }

    /// Percentage of buy ratings (0.0–100.0). Returns 0.0 if no ratings exist.
    pub fn buy_pct(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        f64::from(self.num_buy_ratings.unwrap_or(0)) / f64::from(total) * 100.0
    }
}

/// Individual analyst rating entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingEntry {
    /// Analyst or firm name.
    pub published_by: Option<String>,
    /// Rating type (e.g., "buy", "hold", "sell").
    #[serde(rename = "type")]
    pub rating_type: Option<String>,
    /// Rating text.
    pub text: Option<String>,
    /// Publication date.
    pub published_at: Option<String>,
}

/// A news article related to a stock or the market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsArticle {
    /// Unique article identifier.
    pub uuid: Option<String>,
    /// Article headline.
    pub title: Option<String>,
    /// News source name.
    pub source: Option<String>,
    /// Article URL.
    pub url: Option<String>,
    /// Brief article summary.
    pub summary: Option<String>,
    /// ISO 8601 publication timestamp.
    pub published_at: Option<String>,
    /// Instrument URLs related to this article.
    pub related_instruments: Option<Vec<String>>,
    /// Preview image URL.
    pub preview_image_url: Option<String>,
}

/// Stock split event details.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StockSplit {
    /// API URL for this split resource.
    pub url: Option<String>,
    /// Instrument URL.
    pub instrument: Option<String>,
    /// Date the split was executed (YYYY-MM-DD).
    pub execution_date: Option<String>,
    /// Split multiplier (shares received per original share).
    pub multiplier: Option<String>,
    /// Split divisor.
    pub divisor: Option<String>,
}

/// Result from a tag-based instrument discovery query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagResult {
    /// Human-readable tag name.
    pub name: Option<String>,
    /// URL-friendly tag slug.
    pub slug: Option<String>,
    /// Instrument URLs matching this tag.
    pub instruments: Option<Vec<String>>,
}
