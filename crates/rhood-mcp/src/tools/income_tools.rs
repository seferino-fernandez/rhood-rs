use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use crate::tools::enrichment::{apply_resolved_symbols, resolve_urls_to_symbols};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;
use rhood_core::models::transfer::Transfer;

/// Lean projection of a unified transfer. Drops the untyped, variant-shaped
/// `details` blob (which carries upstream marketing cruft like
/// `gold_deposit_boost`) and keeps only the stable, decision-relevant fields.
#[derive(serde::Serialize)]
struct TransferView {
    id: Option<String>,
    transfer_type: Option<String>,
    amount: Option<String>,
    currency: Option<String>,
    direction: Option<String>,
    state: Option<String>,
    created_at: Option<String>,
    net_amount: Option<String>,
    service_fee: Option<String>,
}

impl From<&Transfer> for TransferView {
    fn from(transfer: &Transfer) -> Self {
        Self {
            id: transfer.id.clone(),
            transfer_type: transfer.transfer_type.clone(),
            amount: transfer.amount.clone(),
            currency: transfer.currency.clone(),
            direction: transfer.direction.clone(),
            state: transfer.state.clone(),
            created_at: transfer.created_at.clone(),
            net_amount: transfer.net_amount.clone(),
            service_fee: transfer.service_fee.clone(),
        }
    }
}

#[tool_router(router = income_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_dividends",
        description = "Get dividend payment history, optionally filtered by date",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_dividends(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<DividendParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let dividends = client
            .get_dividends(params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let urls: Vec<Option<String>> = dividends
            .iter()
            .map(|dividend| dividend.instrument.clone())
            .collect();
        let symbols = resolve_urls_to_symbols(&client, &urls).await;
        let enriched = apply_resolved_symbols(dividends, &symbols);
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_total_dividends",
        description = "Get total dividend income received (paid + reinvested)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_total_dividends(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let total = client
            .get_total_dividends()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let result = serde_json::json!({ "total_dividends": total });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_interest_payments",
        description = "Get interest and sweep payment history",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_interest_payments(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let payments = client
            .get_interest_payments()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&payments).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_transfers",
        description = "Get all unified transfers (ACH, wire, debit card) in a single view",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_transfers(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let transfers = client
            .get_transfers()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let views: Vec<TransferView> = transfers.iter().map(TransferView::from).collect();
        serde_json::to_string_pretty(&views).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::TransferView;
    use rhood_core::models::transfer::Transfer;

    #[test]
    fn transfer_view_drops_details_blob() {
        let transfer: Transfer = serde_json::from_str(
            r#"{"id":"t1","transfer_type":"originated_ach","amount":"100.00",
                "currency":"USD","direction":"pull","state":"completed",
                "details":{"gold_deposit_boost":{"badge":"Gold","learn_more":"http://x"}}}"#,
        )
        .unwrap();
        let view = TransferView::from(&transfer);
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("gold_deposit_boost"),
            "marketing cruft leaked: {json}"
        );
        assert!(!json.contains("details"), "details blob leaked: {json}");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["id"], "t1");
        assert_eq!(value["amount"], "100.00");
    }
}
