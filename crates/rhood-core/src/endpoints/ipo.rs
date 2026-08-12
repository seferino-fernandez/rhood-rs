use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::ipo::{IpoAccessCard, IpoAccessCardsResponse, IpoAccessList};
use crate::{Result, RhoodError};

/// Maximum instrument identifiers accepted by one IPO Access card request.
pub const MAX_IPO_ACCESS_CARD_IDS: usize = 50;

impl RobinhoodClient {
    /// Fetches the IPO Access list view model for the authenticated user.
    ///
    /// The response contains an `empty_state` when no offerings are available.
    /// Offering-dependent sections and presentation fields are preserved raw.
    ///
    /// # Errors
    ///
    /// Returns an error on authentication, HTTP, or deserialization failures.
    pub async fn get_ipo_access_list(&self) -> Result<IpoAccessList> {
        self.get(&self.bonfire_url(paths::IPO_ACCESS_LIST)).await
    }

    /// Fetches IPO Access cards for one or more Robinhood instrument IDs.
    ///
    /// An empty input returns an empty vector without making a request. A
    /// single request accepts at most [`MAX_IPO_ACCESS_CARD_IDS`] identifiers.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] when too many identifiers are
    /// supplied or an identifier is not a UUID. Also returns an error on
    /// authentication, HTTP, or deserialization failures.
    pub async fn get_ipo_access_cards(
        &self,
        instrument_ids: &[&str],
    ) -> Result<Vec<IpoAccessCard>> {
        if instrument_ids.is_empty() {
            return Ok(Vec::new());
        }
        if instrument_ids.len() > MAX_IPO_ACCESS_CARD_IDS {
            return Err(RhoodError::InvalidParameter(format!(
                "IPO Access cards accept at most {MAX_IPO_ACCESS_CARD_IDS} instrument IDs per request"
            )));
        }
        for (index, instrument_id) in instrument_ids.iter().enumerate() {
            if uuid::Uuid::parse_str(instrument_id).is_err() {
                return Err(RhoodError::InvalidParameter(format!(
                    "IPO Access instrument ID at position {} must be a UUID",
                    index.saturating_add(1)
                )));
            }
        }

        let joined_ids = instrument_ids.join(",");
        let response: IpoAccessCardsResponse = self
            .get_with_params(
                &self.bonfire_url(paths::IPO_ACCESS_CARDS),
                &[("ids", joined_ids.as_str())],
            )
            .await?;
        Ok(response
            .results
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::MAX_IPO_ACCESS_CARD_IDS;
    use crate::RhoodError;
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use secrecy::SecretString;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIRST_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
    const SECOND_ID: &str = "1b6c0f0f-9ef9-4d6a-ae1c-7d6f7d5df1a2";

    async fn client_for_server(server: &MockServer) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_string_lossy()
            .into_owned();
        config.api.bonfire_url = server.uri();
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .inject_test_auth(
                SecretString::from("access-token"),
                "Bearer".to_string(),
                SecretString::from("refresh-token"),
            )
            .await;
        (dir, client)
    }

    #[tokio::test]
    async fn list_uses_exact_bonfire_route_and_preserves_empty_state() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/lists/ipo_access/view_model/"))
            .and(header("authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "empty_state": {"title": "No new IPOs available"},
                "learn_tab": {"sections": []},
                "future_component": true
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        let list = client.get_ipo_access_list().await.unwrap();

        assert_eq!(
            list.empty_state.as_ref().unwrap()["title"],
            "No new IPOs available"
        );
        assert_eq!(list.additional_fields["future_component"], true);
        server.verify().await;
    }

    #[tokio::test]
    async fn cards_send_joined_ids_and_preserve_captured_card_shape() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/lists/ipo_access/cards/"))
            .and(query_param("ids", format!("{FIRST_ID},{SECOND_ID}")))
            .and(header("authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "instrument_id": FIRST_ID,
                    "name": "Apple",
                    "title": "AAPL",
                    "subtitle": "",
                    "accent_color": {"light": "fg", "dark": "fg"},
                    "action": {"action_type": "deeplink", "action_data": {}},
                    "future_badge": {"text": "New"}
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        let cards = client
            .get_ipo_access_cards(&[FIRST_ID, SECOND_ID])
            .await
            .unwrap();

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].instrument_id.as_deref(), Some(FIRST_ID));
        assert_eq!(cards[0].title.as_deref(), Some("AAPL"));
        assert_eq!(cards[0].additional_fields["future_badge"]["text"], "New");
        server.verify().await;
    }

    #[tokio::test]
    async fn cards_tolerate_missing_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/lists/ipo_access/cards/"))
            .and(query_param("ids", FIRST_ID))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        assert!(
            client
                .get_ipo_access_cards(&[FIRST_ID])
                .await
                .unwrap()
                .is_empty()
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn cards_tolerate_null_results_and_rows() {
        for body in [
            serde_json::json!({"results": null}),
            serde_json::json!({"results": [null]}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/lists/ipo_access/cards/"))
                .and(query_param("ids", FIRST_ID))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
            let (_dir, client) = client_for_server(&server).await;

            assert!(
                client
                    .get_ipo_access_cards(&[FIRST_ID])
                    .await
                    .unwrap()
                    .is_empty()
            );
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn cards_reject_invalid_and_oversized_inputs_before_http() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server).await;

        assert!(client.get_ipo_access_cards(&[]).await.unwrap().is_empty());
        let invalid = client
            .get_ipo_access_cards(&["not-a-uuid"])
            .await
            .unwrap_err();
        assert!(matches!(invalid, RhoodError::InvalidParameter(_)));
        let identifiers = vec![FIRST_ID; MAX_IPO_ACCESS_CARD_IDS.saturating_add(1)];
        let oversized = client.get_ipo_access_cards(&identifiers).await.unwrap_err();
        assert!(matches!(oversized, RhoodError::InvalidParameter(_)));
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
