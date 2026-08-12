use rhood_core::models::account::{AccountDescriptor, AccountProfile, AccountSummary};
use rhood_core::models::dividend::MoneyAmount;
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use serde::Serialize;

use super::handler::RhoodTools;
use super::pagination::CollectionPager;
use super::params::*;
use super::types::format_tool_error;

fn amount_of(m: &Option<MoneyAmount>) -> Option<String> {
    m.as_ref().and_then(|v| v.amount.clone())
}

#[derive(Serialize)]
struct AccountDiscoveryView {
    account_number: String,
    brokerage_account_type: Option<String>,
}

impl TryFrom<&AccountDescriptor> for AccountDiscoveryView {
    type Error = String;

    fn try_from(account: &AccountDescriptor) -> Result<Self, Self::Error> {
        let account_number = account
            .canonical_account_number()
            .ok_or_else(|| "Unified account is missing a canonical account number".to_string())?;
        Ok(Self {
            account_number: account_number.to_string(),
            brokerage_account_type: account.brokerage_account_type.clone(),
        })
    }
}

/// Decision-relevant projection of [`AccountSummary`] with `MoneyAmount`
/// fields flattened to their dollar string (drops repeated currency metadata).
#[derive(Serialize)]
pub(crate) struct AccountSummaryView {
    account_number: Option<String>,
    brokerage_account_type: Option<String>,
    account_buying_power: Option<String>,
    options_buying_power: Option<String>,
    crypto_buying_power: Option<String>,
    total_equity: Option<String>,
    total_market_value: Option<String>,
    portfolio_equity: Option<String>,
    uninvested_cash: Option<String>,
    withdrawable_cash: Option<String>,
    near_margin_call: Option<bool>,
}

impl From<&AccountSummary> for AccountSummaryView {
    fn from(s: &AccountSummary) -> Self {
        Self {
            account_number: s.account_number.clone(),
            brokerage_account_type: s.brokerage_account_type.clone(),
            account_buying_power: amount_of(&s.account_buying_power),
            options_buying_power: amount_of(&s.options_buying_power),
            crypto_buying_power: amount_of(&s.crypto_buying_power),
            total_equity: amount_of(&s.total_equity),
            total_market_value: amount_of(&s.total_market_value),
            portfolio_equity: amount_of(&s.portfolio_equity),
            uninvested_cash: amount_of(&s.uninvested_cash),
            withdrawable_cash: amount_of(&s.withdrawable_cash),
            near_margin_call: s.near_margin_call,
        }
    }
}

/// Decision-relevant projection of [`AccountProfile`].
#[derive(Serialize)]
pub(crate) struct AccountProfileView {
    account_number: Option<String>,
    account_type: Option<String>,
    brokerage_account_type: Option<String>,
    buying_power: Option<String>,
    cash: Option<String>,
    portfolio_cash: Option<String>,
    cash_available_for_withdrawal: Option<String>,
    crypto_buying_power: Option<String>,
    option_level: Option<String>,
    has_futures_account: Option<bool>,
    deactivated: Option<bool>,
}

impl From<&AccountProfile> for AccountProfileView {
    fn from(p: &AccountProfile) -> Self {
        Self {
            account_number: p.account_number.clone(),
            account_type: p.account_type.clone(),
            brokerage_account_type: p.brokerage_account_type.clone(),
            buying_power: p.buying_power.clone(),
            cash: p.cash.clone(),
            portfolio_cash: p.portfolio_cash.clone(),
            cash_available_for_withdrawal: p.cash_available_for_withdrawal.clone(),
            crypto_buying_power: p.crypto_buying_power.clone(),
            option_level: p.option_level.clone(),
            has_futures_account: p.has_futures_account,
            deactivated: p.deactivated,
        }
    }
}

#[tool_router(router = account_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_accounts",
        description = "Discover brokerage accounts and canonical account numbers for account-sensitive tools",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_accounts(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let accounts = client
            .get_accounts()
            .await
            .map_err(|error| format_tool_error(&error))?;
        let views: Vec<AccountDiscoveryView> = accounts
            .iter()
            .filter(|account| account.canonical_account_number().is_some())
            .map(AccountDiscoveryView::try_from)
            .collect::<Result<_, _>>()?;
        serde_json::to_string_pretty(&views).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_positions",
        description = "Get all current stock positions",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_positions(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let positions = client
            .get_positions(&account)
            .await
            .map_err(|error| format_tool_error(&error))?;
        serde_json::to_string_pretty(&positions).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_all_positions",
        description = "Get all stock positions including closed (zero quantity) ones",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_all_positions(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let positions = client
            .get_all_positions(&account)
            .await
            .map_err(|error| format_tool_error(&error))?;
        serde_json::to_string_pretty(&positions).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_portfolio",
        description = "Get the raw portfolio profile (equity, market value, extended-hours balances, margin)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_portfolio(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let portfolio = client
            .get_portfolio(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&portfolio).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_portfolio_performance",
        description = "Get the raw portfolio performance chart model for one account. Values are returns and rendering metadata, not historical equity candles; the current Bonfire endpoint has no interval, span, or bounds inputs.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_portfolio_performance(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let performance = client
            .get_portfolio_performance(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&performance).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_account_profile",
        description = "Get account profile (account number, type, buying power, cash)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_account_profile(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let profile = client
            .get_account_profile(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let view = AccountProfileView::from(&profile);
        serde_json::to_string_pretty(&view).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_account_summary",
        description = "Get unified account summary (buying power, total equity, market value, cash, margin health)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_account_summary(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let summary = client
            .get_account_summary(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let view = AccountSummaryView::from(&summary);
        serde_json::to_string_pretty(&view).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_documents",
        description = "Get one cursor-paginated page of account documents (statements, tax forms, trade confirmations)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_documents(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<DocumentsParams>,
    ) -> Result<String, String> {
        use rhood_core::models::document::DocumentType;

        let scope = match params.doc_type {
            None => "documents-all",
            Some(DocumentType::AccountStatement) => "documents-account-statement",
            Some(DocumentType::TradeConfirm) => "documents-trade-confirm",
            Some(DocumentType::Tax1099) => "documents-1099",
        };
        let pager = CollectionPager::new(scope, params.cursor.as_deref(), params.limit)?;
        let client = self.ensure_client(&peer).await?;
        let documents = client
            .get_documents(params.doc_type)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        pager.serialize(documents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::models::account::{AccountSummary, Position};
    use rhood_core::models::dividend::MoneyAmount;

    fn money(a: &str) -> MoneyAmount {
        MoneyAmount {
            amount: Some(a.into()),
            currency_code: Some("USD".into()),
            currency_id: Some("1072fc76-1862-41ab-82c2-485837590762".into()),
        }
    }

    #[test]
    fn summary_view_flattens_money_and_drops_currency_id() {
        let summary = AccountSummary {
            account_number: Some("ABC".into()),
            total_equity: Some(money("1234.56")),
            account_buying_power: Some(money("100.00")),
            ..Default::default()
        };
        let view = AccountSummaryView::from(&summary);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("currency_id"), "currency_id leaked: {json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["total_equity"], "1234.56");
        assert_eq!(v["account_buying_power"], "100.00");
        assert_eq!(v["account_number"], "ABC");
    }

    #[test]
    fn positions_serialize_core_owned_wire_symbols_once() {
        let position: Position = serde_json::from_str(
            r#"{"instrument":"https://api.robinhood.com/instruments/abc/","symbol":"RKLB"}"#,
        )
        .unwrap();

        let json = serde_json::to_string(&[position]).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "duplicate symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "RKLB");
    }
}
