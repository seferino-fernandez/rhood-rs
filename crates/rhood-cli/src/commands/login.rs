use rhood_core::{RhoodConfig, RhoodError, RobinhoodClient};
use secrecy::ExposeSecret;

pub async fn run_login(config: RhoodConfig) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config.clone())?;

    let username = match config.auth.username {
        Some(ref name) if !name.is_empty() => name.clone(),
        _ => dialoguer::Input::<String>::new()
            .with_prompt("Robinhood username")
            .interact_text()?,
    };

    let password = match config.auth.password {
        Some(ref secret) if !secret.expose_secret().is_empty() => {
            secret.expose_secret().to_string()
        }
        _ => dialoguer::Password::new()
            .with_prompt("Robinhood password")
            .interact()?,
    };

    let credentials_preloaded = config
        .auth
        .username
        .as_ref()
        .is_some_and(|name| !name.is_empty())
        && config
            .auth
            .password
            .as_ref()
            .is_some_and(|secret| !secret.expose_secret().is_empty());

    let mfa = match config.auth.mfa_secret {
        Some(ref secret) if !secret.expose_secret().is_empty() => {
            secret.expose_secret().to_string()
        }
        _ if credentials_preloaded => String::new(),
        _ => dialoguer::Input::new()
            .with_prompt("MFA code (or TOTP secret, empty to skip)")
            .allow_empty(true)
            .interact_text()?,
    };

    let mfa_ref = if mfa.is_empty() {
        None
    } else {
        Some(mfa.as_str())
    };

    match client.login(&username, &password, mfa_ref).await {
        Ok(()) => {
            println!("Login successful.");
            Ok(())
        }
        Err(RhoodError::ChallengeRequired(challenge_type)) => {
            let challenge_id = match client.auth_state().await {
                rhood_core::auth::AuthState::Challenged { challenge_id, .. } => challenge_id,
                _ => anyhow::bail!("Challenge required but no challenge ID available"),
            };
            println!("Verification required via {challenge_type}. Check your device.");
            let code: String = dialoguer::Input::new()
                .with_prompt("Enter verification code")
                .interact_text()?;
            client
                .submit_challenge_response(&challenge_id, &code, &username, &password, mfa_ref)
                .await?;
            println!("Login successful.");
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

pub async fn run_logout(config: RhoodConfig) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    client.logout().await?;
    println!("Logged out.");
    Ok(())
}
