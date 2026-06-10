//! Dividend and interest payment model types.

use serde::{Deserialize, Serialize};

/// A single dividend payment record from Robinhood.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dividend {
    /// Unique identifier for this dividend record.
    pub id: Option<String>,
    /// API URL for this dividend resource.
    pub url: Option<String>,
    /// API URL for the account that received the dividend.
    pub account: Option<String>,
    /// API URL for the instrument that issued the dividend.
    pub instrument: Option<String>,
    /// Resolved ticker symbol for the instrument (e.g., "AAPL"). Populated
    /// by `enrich_dividend_symbols`; not present in the raw API response.
    pub symbol: Option<String>,
    /// Dividend amount in dollars.
    pub amount: Option<String>,
    /// Dividend rate per share.
    pub rate: Option<String>,
    /// Number of shares held at the record date.
    pub position: Option<String>,
    /// Federal tax withholding amount.
    pub withholding: Option<String>,
    /// Date the dividend was recorded.
    pub record_date: Option<String>,
    /// Date the dividend is payable.
    pub payable_date: Option<String>,
    /// Timestamp when the dividend was actually paid.
    pub paid_at: Option<String>,
    /// Dividend state: "paid", "reinvested", "voided", or "pending".
    pub state: Option<String>,
    /// Non-resident alien withholding amount.
    pub nra_withholding: Option<String>,
    /// Whether dividend reinvestment (DRIP) was enabled.
    pub drip_enabled: Option<bool>,
}

/// A monetary amount with currency metadata.
///
/// Used by interest/sweep payments and potentially other endpoints that
/// return structured `{amount, currency_code, currency_id}` objects.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MoneyAmount {
    /// Numeric amount as a decimal string (e.g., `"2.99"`).
    pub amount: Option<String>,
    /// ISO 4217 currency code (e.g., `"USD"`).
    pub currency_code: Option<String>,
    /// Robinhood-internal currency identifier.
    pub currency_id: Option<String>,
}

/// An interest or sweep payment record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestPayment {
    /// Unique identifier for this payment record.
    pub id: Option<String>,
    /// Payment amount with currency metadata.
    pub amount: Option<MoneyAmount>,
    /// Payment direction (e.g., `"credit"`).
    pub direction: Option<String>,
    /// Account number that received the payment.
    pub account_number: Option<String>,
    /// Date the payment was issued.
    pub pay_date: Option<String>,
    /// Start of the pay period.
    pub pay_period_start: Option<String>,
    /// End of the pay period.
    pub pay_period_end: Option<String>,
    /// Payout type (e.g., `"eom_payment"`, `"end_of_month_payment"`).
    pub payout_type: Option<String>,
    /// Reason for the payment (e.g., `"interest_payment"`).
    pub reason: Option<String>,
}

impl InterestPayment {
    /// Returns the payment ID as a string for display.
    pub fn display_id(&self) -> String {
        self.id.clone().unwrap_or_default()
    }

    /// Returns the payment amount as a string for display.
    pub fn display_amount(&self) -> String {
        self.amount
            .as_ref()
            .and_then(|money| money.amount.clone())
            .unwrap_or_default()
    }

    /// Returns the payout type as a string for display.
    pub fn display_payout_type(&self) -> String {
        self.payout_type.clone().unwrap_or_default()
    }

    /// Returns the pay date as a string for display.
    pub fn display_pay_date(&self) -> String {
        self.pay_date.clone().unwrap_or_default()
    }
}
