//! Regression: two clones of `RobinhoodClient` sharing auth state must be
//! able to issue HTTP calls in parallel, not serialized behind a mutex.

use rhood_core::{RhoodConfig, RobinhoodClient};
use secrecy::SecretString;
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Two cloned clients sharing auth state issue two GETs against a mock server
/// that delays 500ms per response. Serialized behavior would take >=1000ms;
/// parallel behavior takes ~500ms. Assert total elapsed is well under 1000ms.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_gets_do_not_serialize() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/delay"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{}")
                .set_delay(Duration::from_millis(500)),
        )
        .mount(&server)
        .await;

    let mut config = RhoodConfig::default();
    config.api.base_url = server.uri();
    let client = RobinhoodClient::with_config(config).expect("client construction");

    // Inject an authenticated state directly so the test avoids the full
    // login flow. `inject_test_auth` is a crate-test-only helper gated by
    // the `test-helpers` feature.
    client
        .inject_test_auth(
            SecretString::from("test-access"),
            "Bearer".to_string(),
            SecretString::from("test-refresh"),
        )
        .await;

    let url = format!("{}/api/delay", server.uri());
    let client_a = client.clone();
    let client_b = client.clone();
    let url_a = url.clone();
    let url_b = url.clone();

    let start = Instant::now();
    let (result_a, result_b) = tokio::join!(
        async move { client_a.get::<serde_json::Value>(&url_a).await },
        async move { client_b.get::<serde_json::Value>(&url_b).await },
    );
    let elapsed = start.elapsed();

    result_a.expect("request a");
    result_b.expect("request b");
    assert!(
        elapsed < Duration::from_millis(900),
        "two 500ms calls should run in parallel, total ~500ms; got {elapsed:?}"
    );
}
