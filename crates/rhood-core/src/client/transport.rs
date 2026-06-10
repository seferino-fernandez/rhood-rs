//! Authenticated HTTP verb helpers for [`RobinhoodClient`].
//!
//! Wraps reqwest with the auth header, futures contract header, pagination
//! follow-through, and uniform 4xx/5xx/429 handling so endpoint methods stay
//! free of transport boilerplate.

use super::{
    DEFAULT_RETRY_AFTER_SECS, FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE,
    RobinhoodClient,
};
use crate::pagination::{CursorPaginatedResponse, PaginatedResponse};
use crate::{Result, RhoodError};
use serde::Serialize;
use serde::de::DeserializeOwned;

impl RobinhoodClient {
    /// Sends an authenticated GET request and deserializes the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends an authenticated GET request with query parameters and deserializes
    /// the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn get_with_params<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .query(params)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends an authenticated GET request and follows pagination links to collect
    /// all results into a single `Vec`.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn get_paginated<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<T>> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .query(params)
            .send()
            .await?;
        let mut page: PaginatedResponse<T> = handle_response(res).await?;
        let mut all_results = page.results;
        while let Some(next_url) = page.next {
            let res = self
                .http
                .get(&next_url)
                .header("Authorization", &auth)
                .send()
                .await?;
            page = handle_response(res).await?;
            all_results.extend(page.results);
        }
        Ok(all_results)
    }

    /// Sends an authenticated GET request with the futures contract header.
    ///
    /// All Robinhood futures endpoints require `Rh-Contract-Protected: true`.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub(crate) async fn get_futures<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends an authenticated GET request with query params and the futures
    /// contract header.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub(crate) async fn get_futures_with_params<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
            .query(params)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends authenticated GET requests with the futures contract header,
    /// following cursor-based pagination to collect all results.
    ///
    /// Unlike [`get_paginated`](Self::get_paginated), this re-requests the same
    /// base URL with `cursor=<token>` appended to the original params.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub(crate) async fn get_futures_cursor_paginated<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<T>> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .get(url)
            .header("Authorization", &auth)
            .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
            .query(params)
            .send()
            .await?;
        let mut page: CursorPaginatedResponse<T> = handle_response(res).await?;
        let mut all_results = page.results;
        while let Some(cursor) = page.next {
            let mut next_params: Vec<(&str, &str)> = params.to_vec();
            let cursor_owned = cursor;
            next_params.push(("cursor", &cursor_owned));
            let res = self
                .http
                .get(url)
                .header("Authorization", &auth)
                .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
                .query(&next_params)
                .send()
                .await?;
            page = handle_response(res).await?;
            all_results.extend(page.results);
        }
        Ok(all_results)
    }

    /// Sends an authenticated POST request with a form-encoded body and
    /// deserializes the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn post_form<T: DeserializeOwned, P: Serialize + ?Sized>(
        &self,
        url: &str,
        payload: &P,
    ) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .post(url)
            .header("Authorization", &auth)
            .form(payload)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends an authenticated POST request with a JSON body and deserializes
    /// the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn post_json<T: DeserializeOwned, P: Serialize + ?Sized>(
        &self,
        url: &str,
        payload: &P,
    ) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .post(url)
            .header("Authorization", &auth)
            .json(payload)
            .send()
            .await?;
        handle_response(res).await
    }

    /// Sends an authenticated POST request with an empty body.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn post_empty(&self, url: &str) -> Result<()> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .post(url)
            .header("Authorization", &auth)
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        tracing::debug!(
            status = status.as_u16(),
            url = %url,
            body = %body,
            "API response"
        );
        if status.is_success() {
            Ok(())
        } else {
            Err(RhoodError::Api {
                status: status.as_u16(),
                message: body,
            })
        }
    }

    /// Sends an authenticated DELETE request.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn delete(&self, url: &str) -> Result<()> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .delete(url)
            .header("Authorization", &auth)
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        tracing::debug!(
            status = status.as_u16(),
            url = %url,
            body = %body,
            "API response"
        );
        if status.is_success() {
            Ok(())
        } else {
            Err(RhoodError::Api {
                status: status.as_u16(),
                message: body,
            })
        }
    }

    /// Sends an authenticated PATCH request with a JSON body and deserializes
    /// the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn patch_json<T: DeserializeOwned, P: Serialize + ?Sized>(
        &self,
        url: &str,
        payload: &P,
    ) -> Result<T> {
        let auth = self.require_auth().await?;
        let res = self
            .http
            .patch(url)
            .header("Authorization", &auth)
            .json(payload)
            .send()
            .await?;
        handle_response(res).await
    }
}

async fn handle_response<T: DeserializeOwned>(res: reqwest::Response) -> Result<T> {
    let status = res.status();
    if status.as_u16() == 429 {
        let retry_after = res
            .headers()
            .get("retry-after")
            .and_then(|header| header.to_str().ok())
            .and_then(|text| text.parse().ok())
            .unwrap_or(DEFAULT_RETRY_AFTER_SECS);
        return Err(RhoodError::RateLimited {
            retry_after_secs: retry_after,
        });
    }
    let url = res.url().clone();
    let body = res.text().await.unwrap_or_default();
    tracing::debug!(
        status = status.as_u16(),
        url = %url,
        body = %body,
        "API response"
    );
    if !status.is_success() {
        return Err(RhoodError::Api {
            status: status.as_u16(),
            message: body,
        });
    }
    serde_json::from_str::<T>(&body).map_err(|e| RhoodError::Api {
        status: status.as_u16(),
        message: format!("Failed to parse response from {url}: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_config_with_tempdir;
    use super::*;
    use crate::auth::AuthState;
    use secrecy::SecretString;
    use serde::Deserialize;
    use wiremock::matchers::{
        body_string_contains, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct TestBody {
        value: String,
    }

    async fn authenticated_client(base_url: &str) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = base_url.to_string();
        let client = RobinhoodClient::with_config(config).unwrap();
        *client.auth_state.write().await = AuthState::Authenticated {
            access_token: SecretString::from("access-token"),
            token_type: "Bearer".to_string(),
            refresh_token: SecretString::from("refresh-token"),
        };
        (dir, client)
    }

    #[test]
    fn futures_header_constant_is_correct() {
        assert_eq!(FUTURES_CONTRACT_HEADER, "Rh-Contract-Protected");
        assert_eq!(FUTURES_CONTRACT_HEADER_VALUE, "true");
    }

    #[tokio::test]
    async fn get_sends_auth_header_and_deserializes_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/plain"))
            .and(header("Authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "ok"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let body: TestBody = client
            .get(&format!("{}/plain", server.uri()))
            .await
            .unwrap();

        assert_eq!(body, TestBody { value: "ok".into() });
    }

    #[tokio::test]
    async fn get_with_params_sends_query_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("symbol", "HOOD"))
            .and(query_param("active", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "found"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let body: TestBody = client
            .get_with_params(
                &format!("{}/search", server.uri()),
                &[("symbol", "HOOD"), ("active", "true")],
            )
            .await
            .unwrap();

        assert_eq!(body.value, "found");
    }

    #[tokio::test]
    async fn get_paginated_follows_next_links() {
        let server = MockServer::start().await;
        let next_url = format!("{}/page-2", server.uri());
        Mock::given(method("GET"))
            .and(path("/page-1"))
            .and(query_param("nonzero", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [1, 2],
                "next": next_url,
                "previous": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [3],
                "next": null,
                "previous": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let values: Vec<i32> = client
            .get_paginated(&format!("{}/page-1", server.uri()), &[("nonzero", "true")])
            .await
            .unwrap();

        assert_eq!(values, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn futures_gets_include_contract_header_and_cursor_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/futures"))
            .and(header(
                FUTURES_CONTRACT_HEADER,
                FUTURES_CONTRACT_HEADER_VALUE,
            ))
            .and(query_param("symbol", "ES"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [10],
                "next": "cursor-2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/futures"))
            .and(header(
                FUTURES_CONTRACT_HEADER,
                FUTURES_CONTRACT_HEADER_VALUE,
            ))
            .and(query_param("symbol", "ES"))
            .and(query_param("cursor", "cursor-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [20],
                "next": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let values: Vec<i32> = client
            .get_futures_cursor_paginated(&format!("{}/futures", server.uri()), &[("symbol", "ES")])
            .await
            .unwrap();

        assert_eq!(values, vec![10, 20]);
    }

    #[tokio::test]
    async fn get_futures_with_params_includes_header_and_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/futures/quote"))
            .and(header(
                FUTURES_CONTRACT_HEADER,
                FUTURES_CONTRACT_HEADER_VALUE,
            ))
            .and(query_param("symbol", "ES"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "quote"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let body: TestBody = client
            .get_futures_with_params(
                &format!("{}/futures/quote", server.uri()),
                &[("symbol", "ES")],
            )
            .await
            .unwrap();

        assert_eq!(body.value, "quote");
    }

    #[tokio::test]
    async fn post_helpers_send_expected_body_shapes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/form"))
            .and(body_string_contains("name=rhood"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "form-ok"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/json"))
            .and(body_string_contains(r#""name":"rhood""#))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "json-ok"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/empty"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let form: TestBody = client
            .post_form(&format!("{}/form", server.uri()), &[("name", "rhood")])
            .await
            .unwrap();
        let json: TestBody = client
            .post_json(
                &format!("{}/json", server.uri()),
                &serde_json::json!({ "name": "rhood" }),
            )
            .await
            .unwrap();
        client
            .post_empty(&format!("{}/empty", server.uri()))
            .await
            .unwrap();

        assert_eq!(form.value, "form-ok");
        assert_eq!(json.value, "json-ok");
    }

    #[tokio::test]
    async fn delete_and_patch_json_handle_successful_responses() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/resource"))
            .respond_with(ResponseTemplate::new(200).set_body_string("deleted"))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/resource"))
            .and(body_string_contains(r#""enabled":true"#))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "patched"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        client
            .delete(&format!("{}/resource", server.uri()))
            .await
            .unwrap();
        let body: TestBody = client
            .patch_json(
                &format!("{}/resource", server.uri()),
                &serde_json::json!({ "enabled": true }),
            )
            .await
            .unwrap();

        assert_eq!(body.value, "patched");
    }

    #[tokio::test]
    async fn handle_response_maps_rate_limit_with_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "17"))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let err = client
            .get::<serde_json::Value>(&format!("{}/limited", server.uri()))
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            RhoodError::RateLimited {
                retry_after_secs: 17
            }
        ));
    }

    #[tokio::test]
    async fn handle_response_uses_default_rate_limit_when_header_is_invalid() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "soon"))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let err = client
            .get::<serde_json::Value>(&format!("{}/limited", server.uri()))
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            RhoodError::RateLimited {
                retry_after_secs: DEFAULT_RETRY_AFTER_SECS
            }
        ));
    }

    #[tokio::test]
    async fn handle_response_maps_api_and_parse_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api-error"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad-json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let api_err = client
            .get::<serde_json::Value>(&format!("{}/api-error", server.uri()))
            .await
            .unwrap_err();
        let parse_err = client
            .get::<serde_json::Value>(&format!("{}/bad-json", server.uri()))
            .await
            .unwrap_err();

        assert!(matches!(
            api_err,
            RhoodError::Api {
                status: 503,
                message
            } if message == "unavailable"
        ));
        assert!(matches!(
            parse_err,
            RhoodError::Api {
                status: 200,
                message
            } if message.contains("Failed to parse response")
        ));
    }
}
