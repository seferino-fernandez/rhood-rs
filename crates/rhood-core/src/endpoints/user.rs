use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::account::AccountProfile;
use crate::models::user::{DayTradeCheck, UserProfile};
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

/// The raw recent-day-trades endpoint payload.
#[derive(serde::Deserialize)]
struct RecentDayTradesResponse {
    #[serde(default)]
    equity_day_trades: Vec<serde_json::Value>,
    #[serde(default)]
    option_day_trades: Vec<serde_json::Value>,
}

/// Returns whether margin balances indicate a pattern-day-trader flag.
pub(crate) fn is_flagged_pdt(margin: &crate::models::account::MarginBalances) -> bool {
    margin.is_pdt_forever.unwrap_or(false) || margin.marked_pattern_day_trader_date.is_some()
}

impl RobinhoodClient {
    /// Fetches the authenticated user's profile.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_user_profile(&self) -> Result<UserProfile> {
        self.get(&self.api_url(paths::USER)).await
    }

    /// Fetches recent day trades for the user's account.
    ///
    /// Discovers the account number from the account endpoint, then fetches
    /// day trade data. The PDT flag is derived from the account's margin
    /// balances.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if no account is found.
    pub async fn get_day_trades(&self) -> Result<DayTradeCheck> {
        let resp: ResultsResponse<AccountProfile> = self
            .get_with_params(
                &self.api_url(paths::ACCOUNTS),
                &[("default_to_all_accounts", "true")],
            )
            .await?;
        // Pull the account number and PDT flag before the next await so we
        // don't hold a borrow of `resp` across it.
        let (account_number, flagged) = {
            let account = resp.results.first().ok_or(RhoodError::NotAuthenticated)?;
            let number = account
                .account_number
                .clone()
                .ok_or(RhoodError::NotAuthenticated)?;
            let flagged = account
                .margin_balances
                .as_ref()
                .map(is_flagged_pdt)
                .unwrap_or(false);
            (number, flagged)
        };
        let url = format!(
            "{}{account_number}/recent_day_trades/",
            self.api_url(paths::DAY_TRADES)
        );
        let recent: RecentDayTradesResponse = self.get(&url).await?;
        let day_trade_count =
            (recent.equity_day_trades.len() + recent.option_day_trades.len()) as i64;
        Ok(DayTradeCheck {
            equity_day_trades: recent.equity_day_trades,
            option_day_trades: recent.option_day_trades,
            day_trade_count,
            flagged_as_pattern_day_trader: flagged,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::models::user::{DayTradeCheck, UserProfile};

    #[test]
    fn user_profile_deserializes() {
        let json = r#"{
            "id": "user-001",
            "username": "alice@example.com",
            "first_name": "Alice",
            "last_name": "Smith",
            "email": "alice@example.com",
            "created_at": "2024-01-01T00:00:00Z"
        }"#;
        let user: UserProfile = serde_json::from_str(json).unwrap();
        assert_eq!(user.username.as_deref(), Some("alice@example.com"));
        assert_eq!(user.first_name.as_deref(), Some("Alice"));
    }

    #[test]
    fn user_profile_handles_missing_fields() {
        let json = r#"{"id": "user-002"}"#;
        let user: UserProfile = serde_json::from_str(json).unwrap();
        assert_eq!(user.id.as_deref(), Some("user-002"));
        assert!(user.email.is_none());
    }

    #[test]
    fn day_trade_check_new_shape_deserializes() {
        let json = r#"{
            "equity_day_trades": [],
            "option_day_trades": [],
            "day_trade_count": 0,
            "flagged_as_pattern_day_trader": false
        }"#;
        let check: DayTradeCheck = serde_json::from_str(json).unwrap();
        assert_eq!(check.day_trade_count, 0);
        assert!(!check.flagged_as_pattern_day_trader);
        assert!(check.equity_day_trades.is_empty());
        assert!(check.option_day_trades.is_empty());
    }

    #[test]
    fn recent_day_trades_parses_real_payload() {
        let json =
            r#"{"account_number":"767920911","equity_day_trades":[],"option_day_trades":[]}"#;
        let parsed: super::RecentDayTradesResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.equity_day_trades.is_empty());
        assert!(parsed.option_day_trades.is_empty());
    }

    #[test]
    fn pdt_flag_false_when_not_marked() {
        use crate::models::account::MarginBalances;
        let m = MarginBalances {
            is_pdt_forever: Some(false),
            marked_pattern_day_trader_date: None,
            ..Default::default()
        };
        assert!(!super::is_flagged_pdt(&m));
    }

    #[test]
    fn pdt_flag_true_when_marked() {
        use crate::models::account::MarginBalances;
        let m = MarginBalances {
            is_pdt_forever: Some(false),
            marked_pattern_day_trader_date: Some("2026-01-01".into()),
            ..Default::default()
        };
        assert!(super::is_flagged_pdt(&m));
        let forever = MarginBalances {
            is_pdt_forever: Some(true),
            ..Default::default()
        };
        assert!(super::is_flagged_pdt(&forever));
    }
}
