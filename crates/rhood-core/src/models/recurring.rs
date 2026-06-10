//! Recurring investment model types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// How often a recurring investment should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecurringFrequency {
    /// Every week.
    Weekly,
    /// Every two weeks.
    Biweekly,
    /// Every month.
    Monthly,
}

impl fmt::Display for RecurringFrequency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Weekly => formatter.write_str("weekly"),
            Self::Biweekly => formatter.write_str("biweekly"),
            Self::Monthly => formatter.write_str("monthly"),
        }
    }
}

/// Where the funds for a recurring investment come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecurringSource {
    /// Use available buying power in the brokerage account.
    #[default]
    BuyingPower,
    /// Pull funds from the linked ACH bank account.
    Ach,
}

impl fmt::Display for RecurringSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuyingPower => formatter.write_str("buying_power"),
            Self::Ach => formatter.write_str("ach"),
        }
    }
}

/// Lifecycle state of a recurring investment schedule.
///
/// `Deleted` is an internal sentinel set by `cancel_recurring_investment`;
/// it is hidden from the CLI (`#[clap(skip)]`) — callers cancel via the
/// dedicated `recurring cancel` subcommand rather than `--state deleted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecurringState {
    /// Schedule is running.
    Active,
    /// Schedule is paused (no investments execute, but the schedule persists).
    Paused,
    /// Schedule has been cancelled. Internal use only.
    #[cfg_attr(feature = "clap", clap(skip))]
    Deleted,
}

impl fmt::Display for RecurringState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => formatter.write_str("active"),
            Self::Paused => formatter.write_str("paused"),
            Self::Deleted => formatter.write_str("deleted"),
        }
    }
}

/// A money amount with currency code, as used in Robinhood recurring investment payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoneyAmount {
    /// Dollar amount as a string (e.g., "10.00").
    pub amount: String,
    /// Currency code (e.g., "USD").
    pub currency_code: String,
}

/// An investment asset reference within a recurring schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestmentAsset {
    /// Unique asset identifier.
    pub asset_id: Option<String>,
    /// Ticker symbol (e.g., "TSLA").
    pub asset_symbol: Option<String>,
    /// Asset type (e.g., "equity", "crypto").
    pub asset_type: Option<String>,
}

/// A recurring investment schedule from Robinhood.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringInvestment {
    /// Unique schedule identifier.
    pub id: Option<String>,
    /// Account number the schedule belongs to.
    pub account_number: Option<String>,
    /// Investment amount per recurrence.
    pub amount: Option<MoneyAmount>,
    /// Recurrence frequency (e.g., "weekly", "biweekly", "monthly").
    pub frequency: Option<String>,
    /// Date when the schedule starts or next runs.
    pub start_date: Option<String>,
    /// Schedule state (e.g., "active", "paused", "deleted").
    pub state: Option<String>,
    /// The asset being invested in.
    pub investment_asset: Option<InvestmentAsset>,
    /// Timestamp when the schedule was created.
    pub created_at: Option<String>,
    /// Timestamp when the schedule was last updated.
    pub updated_at: Option<String>,
}

/// Request to create a new recurring investment schedule.
#[derive(Debug, Clone)]
pub struct CreateRecurringRequest {
    /// Ticker symbol (e.g., "TSLA").
    pub symbol: String,
    /// Dollar amount per recurrence.
    pub amount: f64,
    /// Recurrence frequency.
    pub frequency: RecurringFrequency,
    /// Start date in YYYY-MM-DD format.
    pub start_date: String,
    /// Source of funds.
    pub source_of_funds: RecurringSource,
}

/// Payload sent to the Robinhood API to create a recurring investment.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreateRecurringPayload {
    pub account_number: String,
    pub amount: MoneyAmount,
    pub frequency: String,
    pub start_date: String,
    pub investment_asset: CreateRecurringAssetPayload,
    pub source_of_funds: String,
    pub ref_id: String,
    pub is_backup_ach_enabled: bool,
}

/// Asset reference in the create payload.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreateRecurringAssetPayload {
    pub asset_id: String,
    pub asset_symbol: String,
    pub asset_type: String,
}

/// Request to update an existing recurring investment schedule.
#[derive(Debug, Clone)]
pub struct UpdateRecurringRequest {
    /// New dollar amount (optional).
    pub amount: Option<f64>,
    /// New frequency (optional).
    pub frequency: Option<RecurringFrequency>,
    /// New state (optional; `Deleted` is internal — use the cancel endpoint instead).
    pub state: Option<RecurringState>,
    /// New start date (optional).
    pub start_date: Option<String>,
}

/// Payload sent to the Robinhood API to update a recurring investment.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct UpdateRecurringPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<MoneyAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
}

/// Response from the next-investment-date lookup endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextInvestmentDate {
    /// Recurrence frequency echoed back by the API.
    pub frequency: Option<String>,
    /// The next scheduled investment date in YYYY-MM-DD format.
    pub next_investment_date: Option<String>,
    /// The start date echoed back by the API.
    pub start_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_frequency_serializes_to_wire_form() {
        assert_eq!(
            serde_json::to_string(&RecurringFrequency::Weekly).unwrap(),
            "\"weekly\""
        );
        assert_eq!(
            serde_json::to_string(&RecurringFrequency::Biweekly).unwrap(),
            "\"biweekly\""
        );
        assert_eq!(
            serde_json::to_string(&RecurringFrequency::Monthly).unwrap(),
            "\"monthly\""
        );
    }

    #[test]
    fn recurring_frequency_roundtrips_all_variants() {
        for variant in [
            RecurringFrequency::Weekly,
            RecurringFrequency::Biweekly,
            RecurringFrequency::Monthly,
        ] {
            let wire = serde_json::to_string(&variant).unwrap();
            let back: RecurringFrequency = serde_json::from_str(&wire).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn recurring_frequency_display_matches_wire() {
        assert_eq!(RecurringFrequency::Weekly.to_string(), "weekly");
        assert_eq!(RecurringFrequency::Biweekly.to_string(), "biweekly");
        assert_eq!(RecurringFrequency::Monthly.to_string(), "monthly");
    }

    #[test]
    fn recurring_source_serializes_to_wire_form() {
        assert_eq!(
            serde_json::to_string(&RecurringSource::BuyingPower).unwrap(),
            "\"buying_power\""
        );
        assert_eq!(
            serde_json::to_string(&RecurringSource::Ach).unwrap(),
            "\"ach\""
        );
    }

    #[test]
    fn recurring_source_default_is_buying_power() {
        assert_eq!(RecurringSource::default(), RecurringSource::BuyingPower);
    }

    #[test]
    fn recurring_source_display_matches_wire() {
        assert_eq!(RecurringSource::BuyingPower.to_string(), "buying_power");
        assert_eq!(RecurringSource::Ach.to_string(), "ach");
    }

    #[test]
    fn recurring_state_serializes_to_wire_form() {
        assert_eq!(
            serde_json::to_string(&RecurringState::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&RecurringState::Paused).unwrap(),
            "\"paused\""
        );
        assert_eq!(
            serde_json::to_string(&RecurringState::Deleted).unwrap(),
            "\"deleted\""
        );
    }

    #[test]
    fn recurring_state_roundtrips_all_variants() {
        for variant in [
            RecurringState::Active,
            RecurringState::Paused,
            RecurringState::Deleted,
        ] {
            let wire = serde_json::to_string(&variant).unwrap();
            let back: RecurringState = serde_json::from_str(&wire).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn recurring_state_display_matches_wire() {
        assert_eq!(RecurringState::Active.to_string(), "active");
        assert_eq!(RecurringState::Paused.to_string(), "paused");
        assert_eq!(RecurringState::Deleted.to_string(), "deleted");
    }

    #[test]
    fn next_investment_date_parses_real_payload() {
        use crate::models::recurring::NextInvestmentDate;
        let json = r#"{"frequency":"weekly","next_investment_date":"2026-06-01","start_date":"2026-06-01"}"#;
        let parsed: NextInvestmentDate = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.next_investment_date.as_deref(), Some("2026-06-01"));
        assert_eq!(parsed.frequency.as_deref(), Some("weekly"));
        assert_eq!(parsed.start_date.as_deref(), Some("2026-06-01"));
    }
}
