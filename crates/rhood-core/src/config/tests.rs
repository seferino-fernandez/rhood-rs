use std::path::Path;

use super::{loader::*, types::*};
use crate::env::{Env, MapEnv, SystemEnv};
use secrecy::{ExposeSecret, SecretString};
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
fn write_file_with_mode(path: &Path, contents: &str, mode: u32) -> std::io::Result<()> {
    std::fs::write(path, contents)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Expose an `Option<SecretString>` as `Option<&str>` for test assertions.
fn expose_secret_opt(secret: &Option<SecretString>) -> Option<&str> {
    secret.as_ref().map(|val| val.expose_secret())
}

#[test]
fn system_env_reads_process_env() {
    // Smoke test only: setting a concrete process env var here would race
    // with parallel tests reading the same key. We assert the absence of
    // a key that is unlikely to be set in any CI environment instead.
    let env = SystemEnv;
    let value = env.get("RHOOD_UNSET_KEY_FOR_SYSTEM_ENV_SMOKE_TEST");
    assert!(
        value.is_none(),
        "expected unset key to return None, got {value:?}"
    );
}

#[test]
fn map_env_get_returns_inserted_values() {
    let env = MapEnv::new()
        .with("RHOOD_USERNAME", "alice")
        .with("RHOOD_API_URL", "https://example.com");
    assert_eq!(env.get("RHOOD_USERNAME").as_deref(), Some("alice"));
    assert_eq!(
        env.get("RHOOD_API_URL").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(env.get("RHOOD_MISSING"), None);
}

#[test]
fn map_env_preserves_empty_strings() {
    // Matches SystemEnv / std::env::var semantics: an empty value is not
    // the same as an unset key.
    let env = MapEnv::new().with("RHOOD_EMPTY", "");
    assert_eq!(env.get("RHOOD_EMPTY").as_deref(), Some(""));
}

#[test]
fn map_env_default_is_empty() {
    let env = MapEnv::default();
    assert_eq!(env.get("ANY_KEY"), None);
}

#[test]
fn default_config_has_expected_values() {
    let cfg = RhoodConfig::default();
    assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
    assert_eq!(cfg.auth.token_expiry_secs, DEFAULT_TOKEN_EXPIRY_SECS);
    assert_eq!(cfg.auth.token_cache_path, DEFAULT_TOKEN_CACHE_PATH);
    assert!(cfg.auth.username.is_none());
    assert!(cfg.auth.password.is_none());
    assert!(cfg.auth.mfa_secret.is_none());
    assert!(cfg.auth.device_token.is_none());
    assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
    assert_eq!(cfg.api.phoenix_url, DEFAULT_PHOENIX_URL);
    assert_eq!(cfg.api.bonfire_url, DEFAULT_BONFIRE_URL);
    assert_eq!(
        cfg.device_verification.poll_interval_secs,
        DEFAULT_DV_POLL_INTERVAL_SECS
    );
    assert_eq!(
        cfg.device_verification.timeout_secs,
        DEFAULT_DV_TIMEOUT_SECS
    );
    assert_eq!(cfg.log.level, DEFAULT_LOG_LEVEL);
}

#[test]
fn load_from_toml_string() {
    let toml = r#"
[auth]
username = "alice"
password = "secret123"
mfa_secret = "JBSWY3DPEHPK3PXP"
client_id = "custom-client-id"
device_token = "dev-tok-123"
token_expiry_secs = 3600
token_cache_path = "/tmp/token"

[api]
base_url = "https://custom.api.com"
phoenix_url = "https://custom.phoenix.com"

[device_verification]
poll_interval_secs = 10
timeout_secs = 300

[log]
level = "debug"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.auth.username.as_deref(), Some("alice"));
    assert_eq!(expose_secret_opt(&cfg.auth.password), Some("secret123"));
    assert_eq!(
        expose_secret_opt(&cfg.auth.mfa_secret),
        Some("JBSWY3DPEHPK3PXP")
    );
    assert_eq!(cfg.auth.client_id, "custom-client-id");
    assert_eq!(
        expose_secret_opt(&cfg.auth.device_token),
        Some("dev-tok-123")
    );
    assert_eq!(cfg.auth.token_expiry_secs, 3600);
    assert_eq!(cfg.auth.token_cache_path, "/tmp/token");
    assert_eq!(cfg.api.base_url, "https://custom.api.com");
    assert_eq!(cfg.api.phoenix_url, "https://custom.phoenix.com");
    assert_eq!(cfg.device_verification.poll_interval_secs, 10);
    assert_eq!(cfg.device_verification.timeout_secs, 300);
    assert_eq!(cfg.log.level, "debug");
}

#[test]
fn load_partial_toml_fills_defaults() {
    let toml = r#"
[auth]
username = "bob"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.auth.username.as_deref(), Some("bob"));
    assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
    assert_eq!(cfg.auth.token_expiry_secs, DEFAULT_TOKEN_EXPIRY_SECS);
    assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
    assert_eq!(
        cfg.device_verification.timeout_secs,
        DEFAULT_DV_TIMEOUT_SECS
    );
    assert_eq!(cfg.log.level, DEFAULT_LOG_LEVEL);
}

#[test]
fn missing_default_file_returns_defaults() {
    // With no RHOOD_CONFIG env and no file at the default path, load(None) returns defaults.
    let cfg = RhoodConfig::load_with_env(None, &MapEnv::new()).unwrap();
    assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
    assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
}

#[test]
fn explicit_missing_file_is_error() {
    let result = RhoodConfig::load_with_env(
        Some(Path::new("/tmp/nonexistent-rhood-config.toml")),
        &MapEnv::new(),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Config file not found"),
        "unexpected error: {err}"
    );
}

#[test]
fn load_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("config.toml");
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[auth]
username = "charlie"
token_expiry_secs = 7200

[log]
level = "warn"
"#
        )
        .unwrap();
    }
    let cfg = RhoodConfig::load_with_env(Some(&file_path), &MapEnv::new()).unwrap();
    assert_eq!(cfg.auth.username.as_deref(), Some("charlie"));
    assert_eq!(cfg.auth.token_expiry_secs, 7200);
    assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
    assert_eq!(cfg.log.level, "warn");
}

#[cfg(unix)]
#[test]
fn owner_only_secret_config_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    write_file_with_mode(&config_path, "[auth]\npassword = \"hunter2\"\n", 0o600).unwrap();

    let config = RhoodConfig::load_with_env(Some(&config_path), &MapEnv::new()).unwrap();

    assert_eq!(expose_secret_opt(&config.auth.password), Some("hunter2"));
}

#[cfg(unix)]
#[test]
fn group_readable_secret_config_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    write_file_with_mode(&config_path, "[auth]\npassword = \"hunter2\"\n", 0o640).unwrap();

    let error = RhoodConfig::load_with_env(Some(&config_path), &MapEnv::new())
        .err()
        .unwrap();
    let message = error.to_string();

    assert!(message.contains(&config_path.display().to_string()));
    assert!(message.contains("0640"));
    assert!(message.contains("chmod 600"));
}

#[cfg(unix)]
#[test]
fn group_readable_secret_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let secret_path = dir.path().join("password.txt");
    write_file_with_mode(&secret_path, "hunter2\n", 0o640).unwrap();
    let toml = format!("[auth]\npassword_file = \"{}\"\n", secret_path.display());

    let error = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new())
        .err()
        .unwrap();
    let message = error.to_string();

    assert!(message.contains(&secret_path.display().to_string()));
    assert!(message.contains("0640"));
    assert!(message.contains("chmod 600"));
}

#[cfg(unix)]
#[test]
fn group_readable_secret_free_config_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    write_file_with_mode(&config_path, "[auth]\nusername = \"alice\"\n", 0o644).unwrap();

    let config = RhoodConfig::load_with_env(Some(&config_path), &MapEnv::new()).unwrap();

    assert_eq!(config.auth.username.as_deref(), Some("alice"));
}

#[test]
fn env_vars_override_toml() {
    let env = MapEnv::new()
        .with(ENV_USERNAME, "env-user")
        .with(ENV_API_URL, "https://env.api.com")
        .with(ENV_LOG_LEVEL, "trace");
    let toml = r#"
[auth]
username = "toml-user"

[api]
base_url = "https://toml.api.com"

[log]
level = "debug"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &env).unwrap();
    assert_eq!(cfg.auth.username.as_deref(), Some("env-user"));
    assert_eq!(cfg.api.base_url, "https://env.api.com");
    assert_eq!(cfg.log.level, "trace");
}

#[test]
fn empty_env_var_does_not_override() {
    let env = MapEnv::new().with(ENV_USERNAME, "");
    let toml = r#"
[auth]
username = "toml-user"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &env).unwrap();
    assert_eq!(cfg.auth.username.as_deref(), Some("toml-user"));
}

#[test]
fn tilde_expansion_in_token_cache_path() {
    let toml = r#"
[auth]
token_cache_path = "~/.rhood-token"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert!(
        !cfg.auth.token_cache_path.starts_with('~'),
        "tilde should be expanded, got: {}",
        cfg.auth.token_cache_path
    );
    let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
    assert_eq!(cfg.auth.token_cache_path, format!("{home}/.rhood-token"));
}

#[test]
fn absolute_path_unchanged() {
    let toml = r#"
[auth]
token_cache_path = "/tmp/my-token"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.auth.token_cache_path, "/tmp/my-token");
}

#[test]
fn base_url_trailing_slash_normalized() {
    let toml = r#"
[api]
base_url = "https://api.robinhood.com/"
phoenix_url = "https://phoenix.robinhood.com/"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.api.base_url, "https://api.robinhood.com");
    assert_eq!(cfg.api.phoenix_url, "https://phoenix.robinhood.com");
}

#[test]
fn default_config_read_only_is_true() {
    let cfg = RhoodConfig::default();
    assert!(cfg.read_only);
}

#[test]
fn read_only_from_toml() {
    let toml = "read_only = true\n";
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert!(cfg.read_only);
}

#[test]
fn read_only_env_override() {
    let env = MapEnv::new().with("RHOOD_READ_ONLY", "true");
    let cfg = RhoodConfig::from_toml_with_env("read_only = false\n", &env).unwrap();
    assert!(cfg.read_only);
}

#[test]
fn read_only_env_override_false_enables_writes() {
    let env = MapEnv::new().with("RHOOD_READ_ONLY", "false");
    let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
    assert!(!cfg.read_only);
}

#[test]
fn secret_file_reads_and_trims() {
    let dir = tempfile::tempdir().unwrap();
    let secret_path = dir.path().join("password.txt");
    std::fs::write(&secret_path, "  hunter2\n  ").unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let toml = format!(
        r#"
[auth]
password_file = "{}"
"#,
        secret_path.display()
    );
    let cfg = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new()).unwrap();
    assert_eq!(expose_secret_opt(&cfg.auth.password), Some("hunter2"));
    assert!(
        cfg.auth.password_file.is_none(),
        "password_file should be consumed"
    );
}

#[test]
fn secret_file_conflict_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let secret_path = dir.path().join("password.txt");
    std::fs::write(&secret_path, "from-file").unwrap();

    let toml = format!(
        r#"
[auth]
password = "inline-value"
password_file = "{}"
"#,
        secret_path.display()
    );
    let err = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Conflicting") && msg.contains("password") && msg.contains("password_file"),
        "unexpected error: {msg}"
    );
}

#[test]
fn secret_file_env_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let secret_path = dir.path().join("password.txt");
    std::fs::write(&secret_path, "from-file").unwrap();

    let env = MapEnv::new()
        .with(ENV_PASSWORD, "from-env")
        .with(ENV_PASSWORD_FILE, secret_path.to_str().unwrap());
    let err = RhoodConfig::from_toml_with_env("", &env).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Conflicting"), "unexpected error: {msg}");
}

#[test]
fn secret_file_missing_is_error() {
    let toml = r#"
[auth]
password_file = "/tmp/nonexistent-rhood-secret-file-12345"
"#;
    let err = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Secret file not found") && msg.contains("password_file"),
        "unexpected error: {msg}"
    );
}

#[test]
fn secret_file_env_overrides_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let secret_path = dir.path().join("password.txt");
    std::fs::write(&secret_path, "from-file").unwrap();

    let env = MapEnv::new().with(ENV_PASSWORD, "from-env");
    let toml = format!(
        r#"
[auth]
password_file = "{}"
"#,
        secret_path.display()
    );
    let cfg = RhoodConfig::from_toml_with_env(&toml, &env).unwrap();
    assert_eq!(expose_secret_opt(&cfg.auth.password), Some("from-env"));
}

#[test]
fn default_config_has_bonfire_url() {
    let cfg = RhoodConfig::default();
    assert_eq!(cfg.api.bonfire_url, "https://bonfire.robinhood.com");
}

#[test]
fn bonfire_url_from_toml() {
    let toml = r#"
[api]
bonfire_url = "https://custom.bonfire.com"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.api.bonfire_url, "https://custom.bonfire.com");
}

#[test]
fn bonfire_url_env_override() {
    let env = MapEnv::new().with("RHOOD_BONFIRE_URL", "https://env.bonfire.com");
    let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
    assert_eq!(cfg.api.bonfire_url, "https://env.bonfire.com");
}

#[test]
fn bonfire_url_trailing_slash_normalized() {
    let toml = r#"
[api]
bonfire_url = "https://bonfire.robinhood.com/"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(cfg.api.bonfire_url, "https://bonfire.robinhood.com");
}

#[test]
fn debug_redacts_auth_secrets() {
    let toml = r#"
[auth]
username = "alice"
password = "hunter2"
mfa_secret = "JBSWY3DPEHPK3PXP"
device_token = "dev-tok-123"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    let debug_output = format!("{:?}", cfg.auth);
    assert!(
        debug_output.contains("[REDACTED]"),
        "Debug should contain [REDACTED]: {debug_output}"
    );
    assert!(
        !debug_output.contains("hunter2"),
        "Debug should not contain password: {debug_output}"
    );
    assert!(
        !debug_output.contains("JBSWY3DPEHPK3PXP"),
        "Debug should not contain mfa_secret: {debug_output}"
    );
    assert!(
        !debug_output.contains("dev-tok-123"),
        "Debug should not contain device_token: {debug_output}"
    );
    assert!(
        debug_output.contains("alice"),
        "Debug should show non-secret username: {debug_output}"
    );
}

#[test]
fn secret_fields_deserialize_from_toml() {
    let toml = r#"
[auth]
password = "p@ssword!"
mfa_secret = "TOTP_SECRET"
device_token = "device-123"
"#;
    let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
    assert_eq!(expose_secret_opt(&cfg.auth.password), Some("p@ssword!"));
    assert_eq!(expose_secret_opt(&cfg.auth.mfa_secret), Some("TOTP_SECRET"));
    assert_eq!(
        expose_secret_opt(&cfg.auth.device_token),
        Some("device-123")
    );
}

#[test]
fn secret_fields_default_to_none() {
    let cfg = RhoodConfig::default();
    assert!(cfg.auth.password.is_none());
    assert!(cfg.auth.mfa_secret.is_none());
    assert!(cfg.auth.device_token.is_none());
}

#[test]
fn http_config_defaults() {
    let cfg = HttpConfig::default();
    assert_eq!(cfg.request_timeout_secs, DEFAULT_HTTP_REQUEST_TIMEOUT_SECS);
    assert_eq!(cfg.connect_timeout_secs, DEFAULT_HTTP_CONNECT_TIMEOUT_SECS);
    assert_eq!(cfg.read_request_interval_ms, 250);
    assert_eq!(cfg.read_max_attempts, 3);
    assert_eq!(cfg.read_retry_backoff_ms, 250);
}

#[test]
fn read_resilience_config_loads_from_toml() {
    let config = RhoodConfig::from_toml_with_env(
        r#"
[http]
read_request_interval_ms = 400
read_max_attempts = 5
read_retry_backoff_ms = 750
"#,
        &MapEnv::new(),
    )
    .unwrap();

    assert_eq!(config.http.read_request_interval_ms, 400);
    assert_eq!(config.http.read_max_attempts, 5);
    assert_eq!(config.http.read_retry_backoff_ms, 750);
}

#[test]
fn read_resilience_environment_overrides_are_strict() {
    let env = MapEnv::new()
        .with("RHOOD_HTTP_READ_REQUEST_INTERVAL_MS", "300")
        .with("RHOOD_HTTP_READ_MAX_ATTEMPTS", "4")
        .with("RHOOD_HTTP_READ_RETRY_BACKOFF_MS", "600");

    let config = RhoodConfig::from_toml_with_env("", &env).unwrap();

    assert_eq!(config.http.read_request_interval_ms, 300);
    assert_eq!(config.http.read_max_attempts, 4);
    assert_eq!(config.http.read_retry_backoff_ms, 600);
}

#[test]
fn zero_read_resilience_values_are_rejected() {
    for field in [
        "read_request_interval_ms",
        "read_max_attempts",
        "read_retry_backoff_ms",
    ] {
        let toml = format!("[http]\n{field} = 0\n");
        let error = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new())
            .err()
            .unwrap();

        assert!(
            error.to_string().contains(field),
            "unexpected error for {field}: {error}"
        );
    }
}

#[test]
fn malformed_read_resilience_environment_values_are_rejected() {
    for name in [
        "RHOOD_HTTP_READ_REQUEST_INTERVAL_MS",
        "RHOOD_HTTP_READ_MAX_ATTEMPTS",
        "RHOOD_HTTP_READ_RETRY_BACKOFF_MS",
    ] {
        let error = RhoodConfig::from_toml_with_env("", &MapEnv::new().with(name, "not-a-number"))
            .err()
            .unwrap();

        assert!(
            error.to_string().contains(name),
            "unexpected error for {name}: {error}"
        );
    }
}

#[test]
fn http_env_overrides_applied() {
    let env = MapEnv::new()
        .with(ENV_HTTP_REQUEST_TIMEOUT_SECS, "45")
        .with(ENV_HTTP_CONNECT_TIMEOUT_SECS, "5");
    let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
    assert_eq!(cfg.http.request_timeout_secs, 45);
    assert_eq!(cfg.http.connect_timeout_secs, 5);
}

#[test]
fn malformed_numeric_environment_override_fails_configuration_loading() {
    let env = MapEnv::new().with(ENV_HTTP_REQUEST_TIMEOUT_SECS, "not-a-number");

    let error = RhoodConfig::from_toml_with_env("", &env)
        .expect_err("malformed override must not fall back to defaults");
    let message = error.to_string();

    assert!(message.contains(ENV_HTTP_REQUEST_TIMEOUT_SECS));
    assert!(message.contains("\"not-a-number\""));
}

#[test]
fn empty_numeric_environment_override_does_not_override_toml() {
    let env = MapEnv::new().with(ENV_HTTP_REQUEST_TIMEOUT_SECS, "");
    let config =
        RhoodConfig::from_toml_with_env("[http]\nrequest_timeout_secs = 45\n", &env).unwrap();

    assert_eq!(config.http.request_timeout_secs, 45);
}

#[test]
fn cache_config_defaults() {
    let cfg = CacheConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.instrument_ttl_secs, DEFAULT_CACHE_INSTRUMENT_TTL_SECS);
    assert_eq!(
        cfg.instrument_max_entries,
        DEFAULT_CACHE_INSTRUMENT_MAX_ENTRIES
    );
    assert_eq!(
        cfg.instrument_id_ttl_secs,
        DEFAULT_CACHE_INSTRUMENT_ID_TTL_SECS
    );
    assert_eq!(
        cfg.instrument_id_max_entries,
        DEFAULT_CACHE_INSTRUMENT_ID_MAX_ENTRIES
    );
    assert_eq!(cfg.index_ttl_secs, DEFAULT_CACHE_INDEX_TTL_SECS);
    assert_eq!(cfg.index_max_entries, DEFAULT_CACHE_INDEX_MAX_ENTRIES);
    assert_eq!(cfg.futures_ttl_secs, DEFAULT_CACHE_FUTURES_TTL_SECS);
    assert_eq!(cfg.futures_max_entries, DEFAULT_CACHE_FUTURES_MAX_ENTRIES);
    assert_eq!(
        cfg.enrichment_batch_size,
        DEFAULT_CACHE_ENRICHMENT_BATCH_SIZE
    );
}

#[test]
fn cache_env_overrides_applied() {
    let env = MapEnv::new()
        .with(ENV_CACHE_ENABLED, "false")
        .with(ENV_CACHE_INSTRUMENT_TTL_SECS, "60")
        .with(ENV_CACHE_INSTRUMENT_MAX_ENTRIES, "1234")
        .with(ENV_CACHE_INSTRUMENT_ID_TTL_SECS, "120")
        .with(ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES, "5678")
        .with(ENV_CACHE_INDEX_TTL_SECS, "30")
        .with(ENV_CACHE_INDEX_MAX_ENTRIES, "9")
        .with(ENV_CACHE_FUTURES_TTL_SECS, "600")
        .with(ENV_CACHE_FUTURES_MAX_ENTRIES, "50")
        .with(ENV_CACHE_ENRICHMENT_BATCH_SIZE, "25");
    let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
    assert!(!cfg.cache.enabled);
    assert_eq!(cfg.cache.instrument_ttl_secs, 60);
    assert_eq!(cfg.cache.instrument_max_entries, 1234);
    assert_eq!(cfg.cache.instrument_id_ttl_secs, 120);
    assert_eq!(cfg.cache.instrument_id_max_entries, 5678);
    assert_eq!(cfg.cache.index_ttl_secs, 30);
    assert_eq!(cfg.cache.index_max_entries, 9);
    assert_eq!(cfg.cache.futures_ttl_secs, 600);
    assert_eq!(cfg.cache.futures_max_entries, 50);
    assert_eq!(cfg.cache.enrichment_batch_size, 25);
}
