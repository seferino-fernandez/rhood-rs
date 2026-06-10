use crate::Result;
use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::transfer::Transfer;

impl RobinhoodClient {
    /// Fetches all unified transfers (ACH, wire, debit card).
    ///
    /// Uses the Bonfire API to retrieve a consolidated view of all transfer
    /// types in a single request.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_transfers(&self) -> Result<Vec<Transfer>> {
        self.get_paginated(&self.bonfire_url(paths::UNIFIED_TRANSFERS), &[])
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::models::transfer::Transfer;

    #[test]
    fn transfer_deserializes_full() {
        let json = r#"{
            "id": "xfer-001",
            "transfer_type": "originated_ach",
            "amount": "500.00",
            "currency": "USD",
            "direction": "pull",
            "state": "completed",
            "created_at": "2026-03-01T10:00:00Z",
            "updated_at": "2026-03-02T10:00:00Z",
            "net_amount": "500.00",
            "service_fee": "0.00",
            "originating_account_id": "bank-001",
            "originating_account_type": "ach",
            "receiving_account_id": "rh-001",
            "receiving_account_type": "brokerage",
            "is_visible_in_history": true,
            "details": {"ach_relationship_id": "rel-001"}
        }"#;
        let xfer: Transfer = serde_json::from_str(json).unwrap();
        assert_eq!(xfer.id.as_deref(), Some("xfer-001"));
        assert_eq!(xfer.transfer_type.as_deref(), Some("originated_ach"));
        assert_eq!(xfer.direction.as_deref(), Some("pull"));
        assert!(xfer.details.is_some());
    }

    #[test]
    fn transfer_handles_missing_fields() {
        let json = r#"{"id": "xfer-002", "state": "pending"}"#;
        let xfer: Transfer = serde_json::from_str(json).unwrap();
        assert_eq!(xfer.id.as_deref(), Some("xfer-002"));
        assert!(xfer.details.is_none());
        assert!(xfer.transfer_type.is_none());
    }
}
