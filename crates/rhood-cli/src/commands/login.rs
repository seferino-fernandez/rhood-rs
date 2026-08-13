use rhood_core::{RhoodConfig, RhoodError, RobinhoodClient};
use secrecy::ExposeSecret;

/// Where the MFA value for this login attempt comes from.
enum MfaSource {
    /// Taken from configuration; never prompted for.
    Configured(String),
    /// Not configured and cannot be asked for, because there is no terminal.
    Absent,
    /// Not configured, and the user can be asked.
    Prompt,
}

/// Decides whether to use a configured MFA secret, prompt for one, or proceed
/// without.
///
/// Skipping the prompt is about whether we can prompt at all — never about
/// whether *other* credentials happen to be configured. Keying it on username
/// and password meant a user who kept those two in a password manager and
/// expected to type their one-time code was silently never asked for it.
fn mfa_source(configured: Option<&secrecy::SecretString>, interactive: bool) -> MfaSource {
    match configured {
        Some(secret) if !secret.expose_secret().is_empty() => {
            MfaSource::Configured(secret.expose_secret().to_string())
        }
        _ if interactive => MfaSource::Prompt,
        _ => MfaSource::Absent,
    }
}

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

    let interactive = {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    };

    let mfa = match mfa_source(config.auth.mfa_secret.as_ref(), interactive) {
        MfaSource::Configured(secret) => secret,
        MfaSource::Absent => String::new(),
        // Concealed, like the password above: this prompt accepts a TOTP
        // secret, which is long-lived reusable authentication material and
        // would otherwise be left in terminal scrollback and session
        // recordings.
        MfaSource::Prompt => dialoguer::Password::new()
            .with_prompt("MFA code (or TOTP secret, empty to skip)")
            .allow_empty_password(true)
            .interact()?,
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
            let code: String = dialoguer::Password::new()
                .with_prompt("Enter verification code")
                .interact()?;
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

#[cfg(test)]
mod tests {
    use super::{MfaSource, mfa_source};
    use secrecy::SecretString;

    /// BL-87. The old rule skipped the prompt whenever username and password
    /// were preloaded, so this configuration — the common one, with the two
    /// long-lived secrets in a password manager and the one-time code typed —
    /// silently logged in without MFA.
    #[test]
    fn an_unconfigured_secret_is_prompted_for_when_a_terminal_exists() {
        assert!(matches!(mfa_source(None, true), MfaSource::Prompt));
        assert!(matches!(
            mfa_source(Some(&SecretString::from("")), true),
            MfaSource::Prompt
        ));
    }

    #[test]
    fn a_configured_secret_is_used_without_prompting() {
        let configured = SecretString::from("JBSWY3DPEHPK3PXP");
        match mfa_source(Some(&configured), true) {
            MfaSource::Configured(secret) => assert_eq!(secret, "JBSWY3DPEHPK3PXP"),
            _ => panic!("a configured secret must never be prompted for"),
        }
        assert!(matches!(
            mfa_source(Some(&configured), false),
            MfaSource::Configured(_)
        ));
    }

    #[test]
    fn without_a_terminal_an_unconfigured_secret_is_absent_rather_than_prompted() {
        assert!(matches!(mfa_source(None, false), MfaSource::Absent));
    }
}
