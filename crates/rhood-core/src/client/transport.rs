//! Authenticated HTTP verb helpers for [`RobinhoodClient`].
//!
//! Wraps reqwest with the auth header, futures contract header, pagination
//! follow-through, and uniform 4xx/5xx/429 handling so endpoint methods stay
//! free of transport boilerplate.

use super::read_policy::ReadAttemptError;
use super::{
    DEFAULT_RETRY_AFTER_SECS, FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE,
    RobinhoodClient,
};
use crate::models::order::{OptionOrder, OptionOrderPayload, StockOrder, StockOrderPayload};
use crate::models::recurring::{CreateRecurringPayload, RecurringInvestment};
use crate::pagination::{CursorPaginatedResponse, PaginatedResponse};
use crate::{Result, RhoodError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashSet;

/// Upper bound on pages followed in one paginated call.
///
/// Chosen well above any observed Robinhood collection so it never truncates a
/// real result, and low enough that a peer looping forever fails in seconds
/// rather than exhausting memory. It is a backstop behind [`PageTrail`], not
/// the primary defence.
const MAX_PAGES_FOLLOWED: usize = 1_000;

/// Records the pagination links already followed in one call.
///
/// A loop whose continuation is decided by response content carries its own
/// bound. A peer returning a `next` that points at a page already fetched must
/// produce an error rather than an unbounded accumulation.
struct PageTrail {
    seen: HashSet<String>,
}

impl PageTrail {
    fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }

    /// Records one link, failing if it repeats or if the page ceiling is hit.
    fn record(&mut self, next_url: &str) -> Result<()> {
        if self.seen.len() >= MAX_PAGES_FOLLOWED {
            return Err(RhoodError::InvalidMarketDataResponse {
                endpoint: "pagination".to_string(),
                message: "paginated response exceeded the page ceiling".into(),
            });
        }
        if !self.seen.insert(next_url.to_string()) {
            return Err(RhoodError::InvalidMarketDataResponse {
                endpoint: "pagination".to_string(),
                message: "paginated response repeated a page link".into(),
            });
        }
        Ok(())
    }
}

impl RobinhoodClient {
    /// Sends an authenticated GET request and deserializes the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let res = self.send_authenticated_get(self.http.get(url)).await?;
        handle_read_response(res)
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
        let res = self
            .send_authenticated_get(self.http.get(url).query(params))
            .await?;
        handle_read_response(res)
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
        let res = self
            .send_authenticated_get(self.http.get(url).query(params))
            .await?;
        let mut page: PaginatedResponse<T> = handle_read_response(res)?;
        let mut all_results = page.results;
        let mut visited = PageTrail::new();
        while let Some(next_url) = page.next {
            self.check_pagination_target(&next_url)?;
            visited.record(&next_url)?;
            let res = self
                .send_authenticated_get(self.http.get(&next_url))
                .await?;
            page = handle_read_response(res)?;
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
        let res = self
            .send_authenticated_get(
                self.http
                    .get(url)
                    .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE),
            )
            .await?;
        handle_read_response(res)
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
        let res = self
            .send_authenticated_get(
                self.http
                    .get(url)
                    .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
                    .query(params),
            )
            .await?;
        handle_read_response(res)
    }

    /// Sends authenticated GET requests with the futures contract header,
    /// following full `next` URLs to collect all results.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the client is not logged in,
    /// or a transport/API error on failure.
    pub(crate) async fn get_futures_paginated<T: DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<Vec<T>> {
        let res = self
            .send_authenticated_get(
                self.http
                    .get(url)
                    .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE),
            )
            .await?;
        let mut page: PaginatedResponse<T> = handle_read_response(res)?;
        let mut all_results = page.results;
        let mut visited = PageTrail::new();
        while let Some(next_url) = page.next {
            self.check_pagination_target(&next_url)?;
            visited.record(&next_url)?;
            let res = self
                .send_authenticated_get(
                    self.http
                        .get(&next_url)
                        .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE),
                )
                .await?;
            page = handle_read_response(res)?;
            all_results.extend(page.results);
        }
        Ok(all_results)
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
        let res = self
            .send_authenticated_get(
                self.http
                    .get(url)
                    .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
                    .query(params),
            )
            .await?;
        let mut page: CursorPaginatedResponse<T> = handle_read_response(res)?;
        let mut all_results = page.results;
        while let Some(cursor) = page.next {
            let mut next_params: Vec<(&str, &str)> = params.to_vec();
            let cursor_owned = cursor;
            next_params.push(("cursor", &cursor_owned));
            let res = self
                .send_authenticated_get(
                    self.http
                        .get(url)
                        .header(FUTURES_CONTRACT_HEADER, FUTURES_CONTRACT_HEADER_VALUE)
                        .query(&next_params),
                )
                .await?;
            page = handle_read_response(res)?;
            all_results.extend(page.results);
        }
        Ok(all_results)
    }

    /// Refuses a pagination link whose host is not one this client is
    /// configured to talk to.
    ///
    /// A URL taken from a response body is untrusted, and
    /// [`Self::send_authenticated_get`] attaches the live bearer token to
    /// whatever it is given. The test is deliberately against this client's own
    /// configured hosts rather than the vendor's topology - a same-origin check
    /// would guess at how Robinhood shards its API, and a hardcoded domain
    /// would break every wiremock test, where all three base URLs point at one
    /// local server.
    fn check_pagination_target(&self, next_url: &str) -> Result<()> {
        let reject = |message: &str| {
            Err(RhoodError::InvalidMarketDataResponse {
                endpoint: "pagination".to_string(),
                message: message.to_string(),
            })
        };
        let Ok(next) = reqwest::Url::parse(next_url) else {
            return reject("pagination link is not a valid absolute URL");
        };
        let Some(next_host) = next.host_str() else {
            return reject("pagination link has no host");
        };
        let configured = [
            &self.config.api.base_url,
            &self.config.api.phoenix_url,
            &self.config.api.bonfire_url,
        ];
        let permitted = configured.iter().any(|base| {
            reqwest::Url::parse(base).is_ok_and(|base| {
                base.host_str() == Some(next_host)
                    && base.port_or_known_default() == next.port_or_known_default()
            })
        });
        if permitted {
            Ok(())
        } else {
            // The rejected host is deliberately not echoed: it is attacker-
            // supplied text and this message reaches logs and adapters.
            reject("pagination link points outside the configured hosts")
        }
    }

    async fn send_authenticated_get(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<BufferedResponse> {
        let auth = self.require_auth().await?;
        let request = request.header("Authorization", auth).build()?;
        if request.try_clone().is_none() {
            return Err(RhoodError::InvalidParameter(
                "authenticated GET request could not be cloned for retry safety".to_string(),
            ));
        }

        let result = self
            .read_policy
            .execute(|| async {
                let attempt = request.try_clone().ok_or(ReadAttemptError::Other(None))?;
                match self.http.execute(attempt).await {
                    Ok(response) if response.status().as_u16() == 429 => {
                        let retry_after_secs = response
                            .headers()
                            .get("retry-after")
                            .and_then(|header| header.to_str().ok())
                            .and_then(|text| self.read_policy.retry_after_delay_secs(text));
                        Err(ReadAttemptError::RateLimited { retry_after_secs })
                    }
                    Ok(response) => {
                        let status = response.status();
                        let url = response.url().clone();
                        match response.text().await {
                            Ok(body) => Ok(BufferedResponse { status, url, body }),
                            Err(error) if error.is_connect() => {
                                Err(ReadAttemptError::Connection(Some(error)))
                            }
                            Err(error) if error.is_timeout() => {
                                Err(ReadAttemptError::Timeout(Some(error)))
                            }
                            Err(error) => Err(ReadAttemptError::Other(Some(error))),
                        }
                    }
                    Err(error) if error.is_connect() => {
                        Err(ReadAttemptError::Connection(Some(error)))
                    }
                    Err(error) if error.is_timeout() => Err(ReadAttemptError::Timeout(Some(error))),
                    Err(error) => Err(ReadAttemptError::Other(Some(error))),
                }
            })
            .await;

        match result {
            Ok(response) => Ok(response),
            Err(ReadAttemptError::RateLimited { retry_after_secs }) => {
                Err(RhoodError::RateLimited {
                    retry_after_secs: retry_after_secs.unwrap_or(DEFAULT_RETRY_AFTER_SECS),
                })
            }
            Err(
                ReadAttemptError::Connection(Some(error))
                | ReadAttemptError::Timeout(Some(error))
                | ReadAttemptError::Other(Some(error)),
            ) => Err(RhoodError::Http(error)),
            Err(
                ReadAttemptError::Connection(None)
                | ReadAttemptError::Timeout(None)
                | ReadAttemptError::Other(None),
            ) => Err(RhoodError::InvalidParameter(
                "authenticated GET request could not be cloned for retry safety".to_string(),
            )),
        }
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

    pub(crate) async fn submit_stock_order_form(
        &self,
        url: &str,
        payload: &StockOrderPayload,
    ) -> Result<StockOrder> {
        let auth = self.require_auth().await?;
        let client_ref = payload.ref_id.clone();
        let response = self
            .write_http
            .post(url)
            .header("Authorization", &auth)
            .form(payload)
            .send()
            .await
            .map_err(|error| RhoodError::SubmissionUncertain {
                client_ref: client_ref.clone(),
                message: error.to_string(),
            })?;

        match handle_response(response).await {
            Err(RhoodError::Api { status, message }) if (200..300).contains(&status) => {
                Err(RhoodError::SubmissionUncertain {
                    client_ref,
                    message,
                })
            }
            result => result,
        }
    }

    pub(crate) async fn submit_option_order_json(
        &self,
        url: &str,
        payload: &OptionOrderPayload,
    ) -> Result<OptionOrder> {
        let auth = self.require_auth().await?;
        let client_ref = payload.ref_id.clone();
        let response = self
            .write_http
            .post(url)
            .header("Authorization", &auth)
            .json(payload)
            .send()
            .await
            .map_err(|error| RhoodError::SubmissionUncertain {
                client_ref: client_ref.clone(),
                message: error.to_string(),
            })?;

        match handle_response(response).await {
            Err(RhoodError::Api { status, message }) if (200..300).contains(&status) => {
                Err(RhoodError::SubmissionUncertain {
                    client_ref,
                    message,
                })
            }
            result => result,
        }
    }

    pub(crate) async fn submit_recurring_investment_json(
        &self,
        url: &str,
        payload: &CreateRecurringPayload,
    ) -> Result<RecurringInvestment> {
        let auth = self.require_auth().await?;
        let client_ref = payload.ref_id.clone();
        let response = self
            .write_http
            .post(url)
            .header("Authorization", &auth)
            .json(payload)
            .send()
            .await
            .map_err(|error| RhoodError::SubmissionUncertain {
                client_ref: client_ref.clone(),
                message: error.to_string(),
            })?;

        match handle_response(response).await {
            Err(RhoodError::Api { status, message }) if (200..300).contains(&status) => {
                Err(RhoodError::SubmissionUncertain {
                    client_ref,
                    message,
                })
            }
            result => result,
        }
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
            body_len = body.len(),
            "API response"
        );
        if status.is_success() {
            Ok(())
        } else {
            Err(RhoodError::Api {
                status: status.as_u16(),
                message: redacted_response_body_message(&body),
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
            body_len = body.len(),
            "API response"
        );
        if status.is_success() {
            Ok(())
        } else {
            Err(RhoodError::Api {
                status: status.as_u16(),
                message: redacted_response_body_message(&body),
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

struct BufferedResponse {
    status: reqwest::StatusCode,
    url: reqwest::Url,
    body: String,
}

fn handle_read_response<T: DeserializeOwned>(response: BufferedResponse) -> Result<T> {
    deserialize_response(response.status, response.url, response.body)
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
    deserialize_response(status, url, body)
}

fn deserialize_response<T: DeserializeOwned>(
    status: reqwest::StatusCode,
    url: reqwest::Url,
    body: String,
) -> Result<T> {
    tracing::debug!(
        status = status.as_u16(),
        url = %url,
        body_len = body.len(),
        "API response"
    );
    if !status.is_success() {
        return Err(RhoodError::Api {
            status: status.as_u16(),
            message: redacted_response_body_message(&body),
        });
    }
    serde_json::from_str::<T>(&body).map_err(|e| RhoodError::Api {
        status: status.as_u16(),
        message: format!("Failed to parse response from {url}: {e}"),
    })
}

pub(super) fn redacted_response_body_message(body: &str) -> String {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(body) else {
        return format!("Response body omitted ({} bytes)", body.len());
    };

    let mut diagnostic = serde_json::Map::new();
    if let Some(instruments) = fields
        .get("missing_instruments")
        .and_then(serde_json::Value::as_array)
    {
        diagnostic.insert(
            "missing_instruments".to_string(),
            serde_json::Value::Array(
                instruments
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(|instrument| serde_json::Value::String(instrument.to_string()))
                    .collect(),
            ),
        );
    }
    for key in ["detail", "message"] {
        if let Some(value) = fields
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(|value| serde_json::Value::String(value.to_string()))
        {
            diagnostic.insert(key.to_string(), value);
        }
    }
    if let Some(message) = fields
        .get("error")
        .and_then(serde_json::Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        diagnostic.insert(
            "error".to_string(),
            serde_json::json!({ "message": message }),
        );
    }

    if diagnostic.is_empty() {
        format!("Response body omitted ({} bytes)", body.len())
    } else {
        serde_json::Value::Object(diagnostic).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::read_policy::ManualClock;
    use super::super::test_config_with_tempdir;
    use super::*;
    use crate::auth::AuthState;
    use crate::models::order::{MarketHours, OrderType, Side, TimeInForce, Trigger};
    use secrecy::SecretString;
    use serde::Deserialize;
    use std::sync::Arc;
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

    /// Builds an authenticated client whose three configured base URLs point
    /// at the two supplied servers, so a test can distinguish "a host this
    /// client talks to" from "the host that served this page".
    async fn authenticated_client_for_hosts(
        base_url: &str,
        phoenix_url: &str,
    ) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = base_url.to_string();
        config.api.phoenix_url = phoenix_url.to_string();
        config.api.bonfire_url = base_url.to_string();
        let client = RobinhoodClient::with_config(config).unwrap();
        *client.auth_state.write().await = AuthState::Authenticated {
            access_token: SecretString::from("access-token"),
            token_type: "Bearer".to_string(),
            refresh_token: SecretString::from("refresh-token"),
        };
        (dir, client)
    }

    /// The bearer token must never reach a host this client is not configured
    /// to talk to, and the proof is that the foreign server
    /// receives no request at all rather than one without a header.
    #[tokio::test]
    async fn pagination_refuses_a_next_link_outside_the_configured_hosts() {
        let server = MockServer::start().await;
        let foreign = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [1],
                "next": format!("{}/steal", foreign.uri()),
                "previous": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [99], "next": null, "previous": null
            })))
            .mount(&foreign)
            .await;

        let (_dir, client) = authenticated_client(&server.uri()).await;
        let url = format!("{}/page-1", server.uri());
        let error = client
            .get_paginated::<i32>(&url, &[])
            .await
            .expect_err("a foreign pagination host must be refused");

        assert!(
            matches!(
                &error,
                RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "pagination"
            ),
            "expected a fail-closed pagination error, got {error}"
        );
        assert!(
            !error.to_string().contains(&foreign.uri()),
            "the rejected host must not be echoed: {error}"
        );
        assert_eq!(
            foreign.received_requests().await.unwrap().len(),
            0,
            "the client must never contact the foreign host"
        );
    }

    /// The rule is about this client's configuration, not about the host that
    /// served the page: a `next` on a different *configured* base URL is
    /// legitimate and must still work.
    #[tokio::test]
    async fn pagination_follows_a_next_link_on_another_configured_host() {
        let server = MockServer::start().await;
        let phoenix = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [1],
                "next": format!("{}/page-2", phoenix.uri()),
                "previous": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [2], "next": null, "previous": null
            })))
            .mount(&phoenix)
            .await;

        let (_dir, client) = authenticated_client_for_hosts(&server.uri(), &phoenix.uri()).await;
        let url = format!("{}/page-1", server.uri());
        let values = client.get_paginated::<i32>(&url, &[]).await.unwrap();

        assert_eq!(values, vec![1, 2]);
    }

    /// The peer does not get to decide when the loop ends.
    #[tokio::test]
    async fn pagination_refuses_a_repeated_page_link() {
        let server = MockServer::start().await;
        let self_link = format!("{}/page-1", server.uri());
        Mock::given(method("GET"))
            .and(path("/page-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [1],
                "next": self_link,
                "previous": null
            })))
            .mount(&server)
            .await;

        let (_dir, client) = authenticated_client(&server.uri()).await;
        let url = format!("{}/page-1", server.uri());
        let error = client
            .get_paginated::<i32>(&url, &[])
            .await
            .expect_err("a self-referential next link must be refused");

        assert!(
            matches!(
                &error,
                RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "pagination"
            ),
            "expected a fail-closed pagination error, got {error}"
        );
    }

    async fn authenticated_client_with_clock(
        base_url: &str,
    ) -> (tempfile::TempDir, RobinhoodClient, Arc<ManualClock>) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = base_url.to_string();
        let clock = Arc::new(ManualClock::new());
        let client = RobinhoodClient::with_config_and_clock(config, clock.clone()).unwrap();
        *client.auth_state.write().await = AuthState::Authenticated {
            access_token: SecretString::from("access-token"),
            token_type: "Bearer".to_string(),
            refresh_token: SecretString::from("refresh-token"),
        };
        (dir, client, clock)
    }

    async fn request_count(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    async fn wait_for_request_count(server: &MockServer, expected: usize) {
        for _ in 0..100 {
            if request_count(server).await == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "expected {expected} requests, observed {}",
            request_count(server).await
        );
    }

    async fn wait_for_policy_sleepers(clock: &ManualClock, expected: usize) {
        for _ in 0..100 {
            if clock.pending_sleepers() >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "expected at least {expected} policy sleepers, observed {}",
            clock.pending_sleepers()
        );
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
    async fn concurrent_clones_space_authenticated_get_starts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "ok"
            })))
            .mount(&server)
            .await;
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;

        let first_client = client.clone();
        let first_url = format!("{}/first", server.uri());
        let first = tokio::spawn(async move { first_client.get::<TestBody>(&first_url).await });
        wait_for_request_count(&server, 1).await;

        let second_client = client.clone();
        let second_url = format!("{}/second", server.uri());
        let second = tokio::spawn(async move { second_client.get::<TestBody>(&second_url).await });
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);

        clock.advance(std::time::Duration::from_millis(249));
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);

        clock.advance(std::time::Duration::from_millis(1));
        wait_for_request_count(&server, 2).await;
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests[0].url.path(), "/first");
        assert_eq!(requests[1].url.path(), "/second");
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
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/page-1", server.uri());
        let task = tokio::spawn(async move {
            client
                .get_paginated::<i32>(&url, &[("nonzero", "true")])
                .await
        });

        wait_for_request_count(&server, 1).await;
        clock.advance(std::time::Duration::from_millis(249));
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);
        clock.advance(std::time::Duration::from_millis(1));
        wait_for_request_count(&server, 2).await;
        let values = task.await.unwrap().unwrap();

        assert_eq!(values, vec![1, 2, 3]);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests[0].url.path(), "/page-1");
        assert_eq!(requests[1].url.path(), "/page-2");
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
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/futures", server.uri());
        let task = tokio::spawn(async move {
            client
                .get_futures_cursor_paginated::<i32>(&url, &[("symbol", "ES")])
                .await
        });

        wait_for_request_count(&server, 1).await;
        clock.advance(std::time::Duration::from_millis(249));
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);
        clock.advance(std::time::Duration::from_millis(1));
        wait_for_request_count(&server, 2).await;
        let values = task.await.unwrap().unwrap();

        assert_eq!(values, vec![10, 20]);
        let requests = server.received_requests().await.unwrap();
        assert!(requests[0].url.query().unwrap().contains("symbol=ES"));
        assert!(requests[1].url.query().unwrap().contains("cursor=cursor-2"));
    }

    #[tokio::test]
    async fn futures_ordinary_next_pagination_preserves_headers_order_and_next_url() {
        let server = MockServer::start().await;
        let next_url = format!("{}/positions/page-2?opaque=kept", server.uri());
        Mock::given(method("GET"))
            .and(path("/positions/page-1"))
            .and(header("Authorization", "Bearer access-token"))
            .and(header(
                FUTURES_CONTRACT_HEADER,
                FUTURES_CONTRACT_HEADER_VALUE,
            ))
            .and(query_param("initial", "kept"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [10, 20],
                "next": next_url,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/positions/page-2"))
            .and(header("Authorization", "Bearer access-token"))
            .and(header(
                FUTURES_CONTRACT_HEADER,
                FUTURES_CONTRACT_HEADER_VALUE,
            ))
            .and(query_param("opaque", "kept"))
            .and(query_param_is_missing("cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [30],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/positions/page-1?initial=kept", server.uri());
        let task = tokio::spawn(async move { client.get_futures_paginated::<i32>(&url).await });

        wait_for_request_count(&server, 1).await;
        clock.advance(std::time::Duration::from_millis(250));
        wait_for_request_count(&server, 2).await;
        let values = task.await.unwrap().unwrap();

        assert_eq!(values, vec![10, 20, 30]);
        server.verify().await;
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
    async fn writes_execute_once_on_network_rate_limit_and_server_failures() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/network-error"))
            .respond_with_err(|_: &wiremock::Request| std::io::Error::other("connection closed"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/rate-limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/server-error"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/stock-order-network-error"))
            .respond_with_err(|_: &wiremock::Request| std::io::Error::other("connection closed"))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let post = client
            .post_json::<serde_json::Value, _>(
                &format!("{}/network-error", server.uri()),
                &serde_json::json!({ "value": true }),
            )
            .await;
        let patch = client
            .patch_json::<serde_json::Value, _>(
                &format!("{}/rate-limited", server.uri()),
                &serde_json::json!({ "value": true }),
            )
            .await;
        let delete = client
            .delete(&format!("{}/server-error", server.uri()))
            .await;
        let stock_order = client
            .submit_stock_order_form(
                &format!("{}/stock-order-network-error", server.uri()),
                &StockOrderPayload {
                    account: "account".to_string(),
                    instrument: "instrument".to_string(),
                    symbol: "HOOD".to_string(),
                    quantity: "1".to_string(),
                    side: Side::Buy,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Gfd,
                    trigger: Trigger::Immediate,
                    market_hours: MarketHours::RegularHours,
                    extended_hours: false,
                    price: None,
                    stop_price: None,
                    ask_price: None,
                    bid_price: None,
                    override_day_trade_checks: false,
                    override_dtbp_checks: false,
                    ref_id: "test-ref".to_string(),
                    order_form_version: 6,
                },
            )
            .await;

        assert!(matches!(post, Err(RhoodError::Http(_))));
        assert!(matches!(patch, Err(RhoodError::RateLimited { .. })));
        assert!(matches!(delete, Err(RhoodError::Api { status: 503, .. })));
        assert!(matches!(
            stock_order,
            Err(RhoodError::SubmissionUncertain { .. })
        ));
        assert_eq!(request_count(&server).await, 4);
    }

    #[tokio::test]
    async fn authenticated_get_retries_after_retry_after_delay() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "17"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/limited"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "recovered"
            })))
            .mount(&server)
            .await;
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited", server.uri());
        let task = tokio::spawn(async move { client.get::<TestBody>(&url).await });

        wait_for_request_count(&server, 1).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(16_999));
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);
        clock.advance(std::time::Duration::from_millis(1));
        wait_for_request_count(&server, 2).await;

        assert_eq!(task.await.unwrap().unwrap().value, "recovered");
    }

    #[tokio::test]
    async fn authenticated_get_honors_future_http_date_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited-date"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "Tue, 14 Nov 2023 22:13:30 GMT"),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/limited-date"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": "recovered"
            })))
            .mount(&server)
            .await;
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited-date", server.uri());
        let task = tokio::spawn(async move { client.get::<TestBody>(&url).await });

        wait_for_request_count(&server, 1).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(250));
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        assert_eq!(request_count(&server).await, 1);
        clock.advance(std::time::Duration::from_millis(9_750));
        wait_for_request_count(&server, 2).await;

        assert_eq!(task.await.unwrap().unwrap().value, "recovered");
    }

    #[tokio::test]
    async fn future_http_date_above_cap_is_terminal() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited-date"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "Tue, 14 Nov 2023 22:14:21 GMT"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client, _clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited-date", server.uri());
        let task = tokio::spawn(async move { client.get::<serde_json::Value>(&url).await });

        wait_for_request_count(&server, 1).await;
        for _ in 0..100 {
            if task.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(task.is_finished(), "over-cap HTTP-date should be terminal");
        let error = task.await.unwrap().unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RateLimited {
                retry_after_secs: 61
            }
        ));
        assert_eq!(request_count(&server).await, 1);
    }

    #[tokio::test]
    async fn past_http_date_retries_at_normal_pacing_and_reports_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited-date"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "Tue, 14 Nov 2023 22:13:19 GMT"),
            )
            .expect(3)
            .mount(&server)
            .await;
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited-date", server.uri());
        let task = tokio::spawn(async move { client.get::<serde_json::Value>(&url).await });

        wait_for_request_count(&server, 1).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(249));
        tokio::task::yield_now().await;
        assert_eq!(request_count(&server).await, 1);
        clock.advance(std::time::Duration::from_millis(1));
        wait_for_request_count(&server, 2).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(250));
        wait_for_request_count(&server, 3).await;
        let error = task.await.unwrap().unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RateLimited {
                retry_after_secs: 0
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
        let (_dir, client, clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited", server.uri());
        let task = tokio::spawn(async move { client.get::<serde_json::Value>(&url).await });
        wait_for_request_count(&server, 1).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(250));
        wait_for_request_count(&server, 2).await;
        wait_for_policy_sleepers(&clock, 1).await;
        clock.advance(std::time::Duration::from_millis(500));
        wait_for_request_count(&server, 3).await;
        let err = task.await.unwrap().unwrap_err();

        assert!(matches!(
            err,
            RhoodError::RateLimited {
                retry_after_secs: DEFAULT_RETRY_AFTER_SECS
            }
        ));
    }

    #[tokio::test]
    async fn retry_after_above_cap_is_terminal_without_an_early_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "61"))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client, _clock) = authenticated_client_with_clock(&server.uri()).await;

        let error = client
            .get::<serde_json::Value>(&format!("{}/limited", server.uri()))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RateLimited {
                retry_after_secs: 61
            }
        ));
        assert_eq!(request_count(&server).await, 1);
    }

    #[tokio::test]
    async fn overflowing_retry_after_is_terminal_and_saturates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/limited-overflow"))
            .respond_with(
                ResponseTemplate::new(429).insert_header("retry-after", " 184467440737095516160 "),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client, _clock) = authenticated_client_with_clock(&server.uri()).await;
        let url = format!("{}/limited-overflow", server.uri());

        let task = tokio::spawn(async move { client.get::<serde_json::Value>(&url).await });
        for _ in 0..100 {
            if task.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            task.is_finished(),
            "overflowing delay-seconds should be terminal"
        );
        let error = task.await.unwrap().unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RateLimited {
                retry_after_secs: u64::MAX
            }
        ));
        assert_eq!(request_count(&server).await, 1);
    }

    #[tokio::test]
    async fn handle_response_maps_api_and_parse_errors() {
        let server = MockServer::start().await;
        let token = "access-token-should-not-appear";
        Mock::given(method("GET"))
            .and(path("/api-error"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_string(format!(r#"{{"access_token":"{token}"}}"#)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad-json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
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
            } if message.starts_with("Response body omitted (") && !message.contains(token)
        ));
        assert!(matches!(
            parse_err,
            RhoodError::Api {
                status: 200,
                message
            } if message.contains("Failed to parse response")
        ));
    }

    #[tokio::test]
    async fn transport_error_preserves_missing_instruments_for_display() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing-instrument"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "missing_instruments": ["NOTAREALSYM"]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let err = client
            .get::<serde_json::Value>(&format!("{}/missing-instrument", server.uri()))
            .await
            .unwrap_err();

        assert!(format!("{err}").contains("NOTAREALSYM"));
    }

    #[tokio::test]
    async fn transport_error_preserves_allowlisted_diagnostic_without_token() {
        let server = MockServer::start().await;
        let token = "access-token-should-not-appear";
        Mock::given(method("GET"))
            .and(path("/insufficient-buying-power"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "detail": "insufficient buying power",
                "access_token": token
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client(&server.uri()).await;

        let err = client
            .get::<serde_json::Value>(&format!("{}/insufficient-buying-power", server.uri()))
            .await
            .unwrap_err();
        let display = format!("{err}");

        assert!(display.contains("insufficient buying power"));
        assert!(!display.contains(token));
    }
}
