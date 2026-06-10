//! Robinhood "pathfinder" device-verification flow for [`RobinhoodClient`].
//!
//! Owns the multi-step verification dance (machine creation, inquiry polling,
//! push-notification approval, workflow confirmation) triggered when the server
//! requires the user to approve a new device on their phone.

use super::{MAX_SERVER_ERROR_RETRIES, RobinhoodClient};
use crate::api::paths;
use crate::auth::AuthState;
use crate::models::auth::{
    PathfinderInput, PathfinderInquiryResponse, PathfinderMachinePayload,
    PathfinderMachineResponse, PushStatusResponse, WorkflowConfirmPayload, WorkflowUserInput,
};
use crate::{ChallengeType, Result, RhoodError};

impl RobinhoodClient {
    /// Run the full Robinhood "pathfinder" device-verification flow:
    /// 1. Create a pathfinder machine from the workflow ID
    /// 2. Poll the inquiry to discover the challenge type
    /// 3. For push-notification ("prompt") challenges, poll until the user approves
    /// 4. Confirm the workflow is approved
    pub(super) async fn handle_device_verification(&self, workflow_id: &str) -> Result<()> {
        *self.auth_state.write().await = AuthState::DeviceVerification {
            workflow_id: workflow_id.to_string(),
        };

        // Step 1: Create pathfinder machine
        let machine_payload = PathfinderMachinePayload {
            device_id: self.device_token.read().await.clone(),
            flow: "suv",
            input: PathfinderInput {
                workflow_id: workflow_id.to_string(),
            },
        };
        let machine_url = self.api_url(paths::PATHFINDER_USER_MACHINE);
        let res = self
            .http
            .post(&machine_url)
            .json(&machine_payload)
            .send()
            .await?;
        let machine_data: PathfinderMachineResponse = res.json().await?;
        tracing::debug!(body = ?machine_data, "Pathfinder machine response");
        let machine_id = &machine_data.id;
        tracing::debug!(machine_id, "Created pathfinder machine");

        // Step 2: Get challenge info from inquiry
        let inquiry_url = format!(
            "{}{machine_id}/user_view/",
            self.api_url(paths::PATHFINDER_INQUIRIES)
        );
        let res = self.http.get(&inquiry_url).send().await?;
        let inquiry_data: PathfinderInquiryResponse = res.json().await?;
        tracing::debug!(body = ?inquiry_data, "Pathfinder inquiry response");

        let challenge = inquiry_data
            .context
            .as_ref()
            .and_then(|ctx| ctx.sheriff_challenge.as_ref());
        let challenge_id = challenge.map_or("", |chal| &chal.id).to_string();
        let challenge_type = challenge.map_or("", |chal| chal.challenge_type.as_str());
        let challenge_status = challenge.map_or("", |chal| chal.status.as_str());

        tracing::debug!(
            challenge_type,
            challenge_status,
            "Device verification challenge"
        );

        // Step 3: Wait for challenge resolution
        if challenge_status != "validated" {
            match challenge_type {
                "prompt" => {
                    self.poll_push_approval(&challenge_id).await?;
                }
                "sms" => {
                    return Err(RhoodError::ChallengeRequired(ChallengeType::Sms));
                }
                "email" => {
                    return Err(RhoodError::ChallengeRequired(ChallengeType::Email));
                }
                other => {
                    return Err(RhoodError::Api {
                        status: 400,
                        message: format!("Unsupported device verification challenge type: {other}"),
                    });
                }
            }
        }

        // Step 4: Confirm workflow approved
        self.confirm_device_workflow(&inquiry_url).await
    }

    /// Poll Robinhood's push-notification endpoint until the user approves
    /// on their mobile device, or timeout.
    async fn poll_push_approval(&self, challenge_id: &str) -> Result<()> {
        let poll_interval =
            std::time::Duration::from_secs(self.config.device_verification.poll_interval_secs);
        let timeout = std::time::Duration::from_secs(self.config.device_verification.timeout_secs);

        let url = format!(
            "{}{challenge_id}/get_prompts_status/",
            self.api_url(paths::PUSH_STATUS)
        );
        let start = tokio::time::Instant::now();

        loop {
            tokio::time::sleep(poll_interval).await;

            if start.elapsed() > timeout {
                return Err(RhoodError::Timeout(
                    "Device verification timed out — no approval received".into(),
                ));
            }

            let res = self.http.get(&url).send().await?;
            let data: PushStatusResponse = res.json().await?;
            tracing::debug!(body = ?data, "Push approval poll response");

            if data.challenge_status == "validated" {
                tracing::debug!("Push notification approved");
                return Ok(());
            }
        }
    }

    /// POST to the pathfinder inquiry to confirm the workflow, polling until
    /// the result is "workflow_status_approved" or timeout.
    async fn confirm_device_workflow(&self, inquiry_url: &str) -> Result<()> {
        let poll_interval =
            std::time::Duration::from_secs(self.config.device_verification.poll_interval_secs);
        let timeout = std::time::Duration::from_secs(self.config.device_verification.timeout_secs);

        let payload = WorkflowConfirmPayload {
            sequence: 0,
            user_input: WorkflowUserInput { status: "continue" },
        };
        let start = tokio::time::Instant::now();
        let mut server_error_count = 0u32;

        loop {
            if start.elapsed() > timeout {
                return Err(RhoodError::Timeout(
                    "Workflow confirmation timed out".into(),
                ));
            }

            let res = self.http.post(inquiry_url).json(&payload).send().await?;

            if res.status().is_server_error() {
                server_error_count += 1;
                if server_error_count >= MAX_SERVER_ERROR_RETRIES {
                    return Err(RhoodError::Api {
                        status: res.status().as_u16(),
                        message: "Pathfinder inquiry returned repeated server errors".into(),
                    });
                }
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            let data: PathfinderInquiryResponse = res.json().await?;
            tracing::debug!(body = ?data, "Workflow confirmation poll response");
            let result = data
                .type_context
                .as_ref()
                .and_then(|tc| tc.result.as_deref());
            if result == Some("workflow_status_approved") {
                tracing::debug!("Device workflow approved");
                return Ok(());
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_config_with_tempdir;
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_for_server(base_url: &str) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = base_url.to_string();
        config.device_verification.poll_interval_secs = 0;
        config.device_verification.timeout_secs = 5;
        let client = RobinhoodClient::with_config(config).unwrap();
        (dir, client)
    }

    fn machine_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "machine-1"
        }))
    }

    fn inquiry_response(challenge_type: &str, status: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "context": {
                "sheriff_challenge": {
                    "id": "challenge-1",
                    "type": challenge_type,
                    "status": status
                }
            },
            "type_context": null
        }))
    }

    fn approved_workflow_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "context": null,
            "type_context": {
                "result": "workflow_status_approved"
            }
        }))
    }

    #[tokio::test]
    async fn handle_device_verification_confirms_already_validated_challenge() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/user_machine/"))
            .and(body_string_contains(r#""workflow_id":"wf-1""#))
            .respond_with(machine_response())
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(inquiry_response("prompt", "validated"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .and(body_string_contains(r#""status":"continue""#))
            .respond_with(approved_workflow_response())
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        client.handle_device_verification("wf-1").await.unwrap();

        assert!(matches!(
            client.auth_state().await,
            AuthState::DeviceVerification { workflow_id } if workflow_id == "wf-1"
        ));
    }

    #[tokio::test]
    async fn handle_device_verification_polls_prompt_until_validated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/user_machine/"))
            .respond_with(machine_response())
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(inquiry_response("prompt", "issued"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/push/challenge-1/get_prompts_status/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "challenge_status": "validated"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(approved_workflow_response())
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        client.handle_device_verification("wf-1").await.unwrap();
    }

    #[tokio::test]
    async fn handle_device_verification_returns_challenge_for_sms_and_email() {
        for (challenge_type, expected) in
            [("sms", ChallengeType::Sms), ("email", ChallengeType::Email)]
        {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/pathfinder/user_machine/"))
                .respond_with(machine_response())
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/pathfinder/inquiries/machine-1/user_view/"))
                .respond_with(inquiry_response(challenge_type, "issued"))
                .mount(&server)
                .await;
            let (_dir, client) = client_for_server(&server.uri()).await;

            let err = client.handle_device_verification("wf-1").await.unwrap_err();

            assert!(matches!(err, RhoodError::ChallengeRequired(actual) if actual == expected));
        }
    }

    #[tokio::test]
    async fn handle_device_verification_rejects_unsupported_challenge_type() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/user_machine/"))
            .respond_with(machine_response())
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(inquiry_response("voice", "issued"))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let err = client.handle_device_verification("wf-1").await.unwrap_err();

        assert!(matches!(
            err,
            RhoodError::Api {
                status: 400,
                message
            } if message.contains("Unsupported device verification challenge type: voice")
        ));
    }

    #[tokio::test]
    async fn confirm_device_workflow_retries_transient_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/inquiry"))
            .respond_with(ResponseTemplate::new(500).set_body_string("try again"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/inquiry"))
            .respond_with(approved_workflow_response())
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        client
            .confirm_device_workflow(&format!("{}/inquiry", server.uri()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn confirm_device_workflow_fails_after_repeated_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/inquiry"))
            .respond_with(ResponseTemplate::new(500).set_body_string("still down"))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let err = client
            .confirm_device_workflow(&format!("{}/inquiry", server.uri()))
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            RhoodError::Api {
                status: 500,
                message
            } if message == "Pathfinder inquiry returned repeated server errors"
        ));
    }

    #[tokio::test]
    async fn poll_push_approval_times_out_without_validation() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = server.uri();
        config.device_verification.poll_interval_secs = 0;
        config.device_verification.timeout_secs = 0;
        let client = RobinhoodClient::with_config(config).unwrap();

        let err = client.poll_push_approval("challenge-1").await.unwrap_err();

        assert!(matches!(err, RhoodError::Timeout(message) if message.contains("timed out")));
    }
}
