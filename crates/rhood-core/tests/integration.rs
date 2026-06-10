use rhood_core::RobinhoodClient;
use rhood_core::config::RhoodConfig;

fn test_config(cache_path: &str) -> RhoodConfig {
    let mut cfg = RhoodConfig::default();
    cfg.auth.token_cache_path = cache_path.to_string();
    cfg
}

#[tokio::test]
async fn client_creation_succeeds() {
    let client = RobinhoodClient::with_config(test_config("/tmp/rhood-test-noexist.json")).unwrap();
    assert!(!client.is_authenticated().await);
}

#[tokio::test]
async fn unauthenticated_client_rejects_api_calls() {
    let client =
        RobinhoodClient::with_config(test_config("/tmp/rhood-test-noexist2.json")).unwrap();
    let result = client.get_quotes(&["AAPL"]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, rhood_core::RhoodError::NotAuthenticated));
}
