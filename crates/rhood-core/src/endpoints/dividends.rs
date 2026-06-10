use crate::Result;
use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::dividend::{Dividend, InterestPayment};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Sums the `amount` of dividends in `paid`/`reinvested` states using decimal
/// arithmetic, returning the total as a string.
///
/// Decimal summation preserves the source amounts' scale, so currency values
/// keep their trailing zeros (e.g. `"0.07" + "0.12" -> "0.19"`, and a lone
/// `"5.00" -> "5.00"`). Trailing zeros are deliberately *not* stripped.
pub(crate) fn sum_dividend_amounts(dividends: &[crate::models::dividend::Dividend]) -> String {
    let total: Decimal = dividends
        .iter()
        .filter(|d| matches!(d.state.as_deref(), Some("paid" | "reinvested")))
        .filter_map(|d| d.amount.as_deref())
        .filter_map(|a| Decimal::from_str(a).ok())
        .sum();
    total.to_string()
}

impl RobinhoodClient {
    /// Fetches all dividend payments, optionally filtered by date.
    ///
    /// When `since` is provided, only dividends updated on or after that date
    /// (ISO 8601 format, e.g. "2025-01-01") are returned.
    ///
    /// Instrument URLs are resolved to ticker symbols on a best-effort basis
    /// via [`enrich_dividend_symbols`]; any failure is silently ignored so
    /// that the caller always receives the raw dividends even when the
    /// symbol-resolution request fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    ///
    /// [`enrich_dividend_symbols`]: RobinhoodClient::enrich_dividend_symbols
    pub async fn get_dividends(&self, since: Option<&str>) -> Result<Vec<Dividend>> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(date) = since {
            params.push(("updated_at[gte]", date));
        }
        let mut dividends: Vec<Dividend> = self
            .get_paginated(&self.api_url(paths::DIVIDENDS), &params)
            .await?;
        let _ = self.enrich_dividend_symbols(&mut dividends).await;
        Ok(dividends)
    }

    /// Backfills `symbol` on each dividend by resolving its instrument URL to a
    /// ticker via a single batched `/instruments/?ids=` request. Best-effort:
    /// dividends whose URL can't be parsed or resolved are left with
    /// `symbol = None`.
    pub async fn enrich_dividend_symbols(&self, dividends: &mut [Dividend]) -> Result<()> {
        let uuids: Vec<String> = dividends
            .iter()
            .filter(|d| d.symbol.is_none())
            .filter_map(|d| d.instrument.as_deref())
            .filter_map(|url| crate::util::instrument_id_from_url(url))
            .map(|id| id.to_string())
            .collect();
        if uuids.is_empty() {
            return Ok(());
        }
        let map = self.resolve_symbols(&uuids).await?;
        for d in dividends.iter_mut() {
            if d.symbol.is_none()
                && let Some(url) = d.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(url)
                && let Some(sym) = map.get(id)
            {
                d.symbol = Some(sym.clone());
            }
        }
        Ok(())
    }

    /// Computes the total dividend income received (paid + reinvested) as a
    /// decimal string that preserves the source amounts' precision. Excludes
    /// voided/pending dividends.
    ///
    /// # Errors
    ///
    /// Returns an error if the dividend fetch fails.
    pub async fn get_total_dividends(&self) -> Result<String> {
        let dividends = self.get_dividends(None).await?;
        Ok(sum_dividend_amounts(&dividends))
    }

    /// Fetches all interest/sweep payments.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_interest_payments(&self) -> Result<Vec<InterestPayment>> {
        self.get_paginated(&self.api_url(paths::SWEEPS), &[]).await
    }
}

#[cfg(test)]
mod tests {
    use crate::models::dividend::{Dividend, InterestPayment};

    #[test]
    fn sum_dividend_amounts_avoids_float_error() {
        use crate::models::dividend::Dividend;
        let mk = |state: &str, amount: &str| Dividend {
            amount: Some(amount.into()),
            state: Some(state.into()),
            ..Default::default()
        };
        let divs = vec![
            mk("paid", "0.07"),
            mk("reinvested", "0.12"),
            mk("pending", "5.00"), // excluded
            mk("voided", "9.00"),  // excluded
        ];
        assert_eq!(super::sum_dividend_amounts(&divs), "0.19");
    }

    #[test]
    fn sum_dividend_amounts_preserves_trailing_zeros() {
        use crate::models::dividend::Dividend;
        let mk = |state: &str, amount: &str| Dividend {
            amount: Some(amount.into()),
            state: Some(state.into()),
            ..Default::default()
        };
        // A whole-dollar total keeps its currency precision rather than
        // collapsing to "5".
        let divs = vec![mk("paid", "2.50"), mk("reinvested", "2.50")];
        assert_eq!(super::sum_dividend_amounts(&divs), "5.00");
        // Empty / all-excluded sums to a plain zero.
        assert_eq!(super::sum_dividend_amounts(&[]), "0");
    }

    #[test]
    fn dividend_deserializes_full() {
        let json = r#"{
            "id": "div-001",
            "url": "https://api.robinhood.com/dividends/div-001/",
            "account": "https://api.robinhood.com/accounts/ABC123/",
            "instrument": "https://api.robinhood.com/instruments/inst-001/",
            "amount": "1.25",
            "rate": "0.25",
            "position": "5.0000",
            "withholding": "0.00",
            "record_date": "2026-03-15",
            "payable_date": "2026-03-20",
            "paid_at": "2026-03-20T10:00:00Z",
            "state": "paid",
            "nra_withholding": "0.00",
            "drip_enabled": true
        }"#;
        let div: Dividend = serde_json::from_str(json).unwrap();
        assert_eq!(div.id.as_deref(), Some("div-001"));
        assert_eq!(div.amount.as_deref(), Some("1.25"));
        assert_eq!(div.state.as_deref(), Some("paid"));
        assert_eq!(div.drip_enabled, Some(true));
    }

    #[test]
    fn dividend_handles_missing_fields() {
        let json = r#"{"id": "div-002", "state": "pending"}"#;
        let div: Dividend = serde_json::from_str(json).unwrap();
        assert_eq!(div.id.as_deref(), Some("div-002"));
        assert!(div.amount.is_none());
        assert!(div.paid_at.is_none());
        // symbol is not in raw API response — must default to None
        assert!(div.symbol.is_none());
    }

    #[test]
    fn dividend_symbol_field_deserializes() {
        // When the JSON happens to include "symbol" (e.g. after enrichment
        // round-trip through serde), it should populate the field.
        let json = r#"{"id": "div-003", "symbol": "AAPL", "state": "paid"}"#;
        let div: Dividend = serde_json::from_str(json).unwrap();
        assert_eq!(div.symbol.as_deref(), Some("AAPL"));
        assert_eq!(div.id.as_deref(), Some("div-003"));
    }

    #[test]
    fn enrich_dividend_symbols_applies_map() {
        // Unit test for the assignment loop logic, mirroring the positions test.
        let uuid = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let url = format!("https://api.robinhood.com/instruments/{uuid}/");
        let mut div = Dividend {
            id: Some("div-004".to_string()),
            instrument: Some(url.clone()),
            symbol: None,
            ..Default::default()
        };

        // Simulate what enrich_dividend_symbols does after calling resolve_symbols
        let mut map = std::collections::HashMap::new();
        map.insert(uuid.to_string(), "TSLA".to_string());

        let dividends: &mut [Dividend] = std::slice::from_mut(&mut div);
        for d in dividends.iter_mut() {
            if d.symbol.is_none()
                && let Some(instrument_url) = d.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(instrument_url)
                && let Some(sym) = map.get(id)
            {
                d.symbol = Some(sym.clone());
            }
        }

        assert_eq!(div.symbol.as_deref(), Some("TSLA"));
    }

    #[test]
    fn enrich_dividend_symbols_skips_already_set() {
        let uuid = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let url = format!("https://api.robinhood.com/instruments/{uuid}/");
        let mut div = Dividend {
            instrument: Some(url),
            symbol: Some("EXISTING".to_string()),
            ..Default::default()
        };

        let mut map = std::collections::HashMap::new();
        map.insert(uuid.to_string(), "REPLACED".to_string());

        // Only apply if symbol is None (mirrors enrich logic)
        let dividends: &mut [Dividend] = std::slice::from_mut(&mut div);
        for d in dividends.iter_mut() {
            if d.symbol.is_none()
                && let Some(instrument_url) = d.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(instrument_url)
                && let Some(sym) = map.get(id)
            {
                d.symbol = Some(sym.clone());
            }
        }

        // Should not be replaced because symbol was already set
        assert_eq!(div.symbol.as_deref(), Some("EXISTING"));
    }

    #[test]
    fn interest_payment_deserializes_real_api_shape() {
        let json = r#"{
            "amount": {
                "amount": "2.99",
                "currency_code": "USD",
                "currency_id": "1072fc76-1862-41ab-82c2-485837590762"
            },
            "direction": "credit",
            "id": "9c6fe185-e563-4d33-95b0-6c8fe558bcf1",
            "account_number": "767920911",
            "pay_date": "2026-03-31T21:00:00Z",
            "pay_period_start": "2026-03-31T21:00:00Z",
            "pay_period_end": "2026-03-31T21:00:00Z",
            "payout_type": "eom_payment",
            "reason": "interest_payment"
        }"#;
        let payment: InterestPayment = serde_json::from_str(json).unwrap();
        assert_eq!(payment.display_id(), "9c6fe185-e563-4d33-95b0-6c8fe558bcf1");
        assert_eq!(payment.display_amount(), "2.99");
        assert_eq!(payment.display_payout_type(), "eom_payment");
        assert_eq!(payment.display_pay_date(), "2026-03-31T21:00:00Z");
        assert_eq!(payment.direction.as_deref(), Some("credit"));
        assert_eq!(payment.account_number.as_deref(), Some("767920911"));
        assert_eq!(payment.reason.as_deref(), Some("interest_payment"));
        let amount = payment.amount.unwrap();
        assert_eq!(amount.currency_code.as_deref(), Some("USD"));
        assert_eq!(
            amount.currency_id.as_deref(),
            Some("1072fc76-1862-41ab-82c2-485837590762")
        );
    }

    #[test]
    fn interest_payment_deserializes_missing_fields() {
        let json = r#"{}"#;
        let payment: InterestPayment = serde_json::from_str(json).unwrap();
        assert_eq!(payment.display_id(), "");
        assert_eq!(payment.display_amount(), "");
        assert!(payment.direction.is_none());
        assert!(payment.pay_date.is_none());
    }
}
