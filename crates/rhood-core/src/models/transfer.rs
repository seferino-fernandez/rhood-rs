//! Unified transfer model types.

use serde::{Deserialize, Serialize};

/// A unified transfer record (ACH, wire, or debit card).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    /// Unique identifier for this transfer.
    pub id: Option<String>,
    /// Transfer type (e.g., "originated_ach", "debit_card_funding").
    pub transfer_type: Option<String>,
    /// Transfer amount.
    pub amount: Option<String>,
    /// Currency code (e.g., "USD").
    pub currency: Option<String>,
    /// Transfer direction: "pull" or "push".
    pub direction: Option<String>,
    /// Transfer state (e.g., "completed", "pending").
    pub state: Option<String>,
    /// Timestamp when the transfer was created.
    pub created_at: Option<String>,
    /// Timestamp when the transfer was last updated.
    pub updated_at: Option<String>,
    /// Net transfer amount after fees.
    pub net_amount: Option<String>,
    /// Service fee charged for the transfer.
    pub service_fee: Option<String>,
    /// Originating account ID.
    pub originating_account_id: Option<String>,
    /// Originating account type.
    pub originating_account_type: Option<String>,
    /// Receiving account ID.
    pub receiving_account_id: Option<String>,
    /// Receiving account type.
    pub receiving_account_type: Option<String>,
    /// Whether the transfer is visible in transaction history.
    pub is_visible_in_history: Option<bool>,
    /// Type-specific details (varies by transfer type).
    pub details: Option<serde_json::Value>,
}
