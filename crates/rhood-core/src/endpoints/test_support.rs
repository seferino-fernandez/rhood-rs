//! Test-only module for cross-cutting endpoint tests.
//!
//! Tests that exercise behaviors which span multiple endpoint domains
//! (e.g. multi-domain method-signature compile-checks) live here rather
//! than being arbitrarily placed in a single domain's `mod tests` block.

#[cfg(test)]
mod tests {
    #[test]
    fn validate_token_is_public_method() {
        // Compile-time check that validate_token exists on RobinhoodClient
        async fn _assert(client: &crate::RobinhoodClient) {
            let _ = client.validate_token().await;
        }
    }

    #[test]
    fn endpoint_signatures_watchlist_user_documents() {
        async fn _assert(client: &crate::RobinhoodClient) {
            let _ = client.get_watchlists().await;
            let _ = client.get_watchlist("Default").await;
            let _ = client.get_watchlist_items("Default").await;
            let _ = client.get_documents(None).await;
            let _ = client.get_user_profile().await;
            let _ = client.get_day_trades().await;
            let _ = client.cancel_all_stock_orders().await;
            let _ = client.cancel_all_option_orders().await;
        }
    }
}
