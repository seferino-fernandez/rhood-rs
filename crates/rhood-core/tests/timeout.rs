//! Regression: an upstream HTTP call that hangs longer than the configured
//! request timeout must error out with a timeout, not block indefinitely.

use rhood_core::{RhoodConfig, RobinhoodClient};
use secrecy::SecretString;
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_timeout_aborts_slow_upstream() {
    let server = MockServer::start().await;
    // Keep every attempt pending well beyond the 1-second per-request timeout.
    Mock::given(method("GET"))
        .and(path("/api/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{}")
                .set_delay(Duration::from_secs(10)),
        )
        .expect(3)
        .mount(&server)
        .await;

    let mut config = RhoodConfig::default();
    config.api.base_url = server.uri();
    config.http.request_timeout_secs = 1;
    config.http.connect_timeout_secs = 1;
    let client = RobinhoodClient::with_config(config).expect("client construction");
    client
        .inject_test_auth(
            SecretString::from("test-access"),
            "Bearer".to_string(),
            SecretString::from("test-refresh"),
        )
        .await;

    let url = format!("{}/api/slow", server.uri());
    let start = Instant::now();
    let result = client.get::<serde_json::Value>(&url).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "expected timeout error, got {result:?}");
    assert!(
        elapsed < Duration::from_millis(5_500),
        "three bounded attempts should finish well before the 10s upstream delay; took {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(2_700),
        "client should allow all three 1s timeout attempts; took {elapsed:?}"
    );
    server.verify().await;
}
