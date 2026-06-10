//! Disk-backed OAuth token cache for the Robinhood API client.
//!
//! [`CachedToken`] holds the credential fields needed to restore a session,
//! and [`TokenCache`] manages reading and writing those tokens as a JSON file
//! with restricted file permissions on Unix.

use crate::Result;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Represents a set of cached OAuth credentials for a Robinhood session.
#[derive(Clone)]
pub struct CachedToken {
    /// The OAuth access token used to authorize API requests.
    pub access_token: SecretString,
    /// The OAuth refresh token used to obtain a new access token.
    pub refresh_token: SecretString,
    /// The token type prefix for the `Authorization` header (typically `"Bearer"`).
    pub token_type: String,
    /// The device token identifying this client to the Robinhood API.
    pub device_token: String,
    /// The Unix timestamp (seconds) at which the access token expires, if known.
    pub expires_at: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct CachedTokenOnDisk {
    access_token: String,
    refresh_token: String,
    token_type: String,
    device_token: String,
    expires_at: Option<i64>,
}

impl From<&CachedToken> for CachedTokenOnDisk {
    fn from(token: &CachedToken) -> Self {
        Self {
            access_token: token.access_token.expose_secret().to_owned(),
            refresh_token: token.refresh_token.expose_secret().to_owned(),
            token_type: token.token_type.clone(),
            device_token: token.device_token.clone(),
            expires_at: token.expires_at,
        }
    }
}

impl From<CachedTokenOnDisk> for CachedToken {
    fn from(token: CachedTokenOnDisk) -> Self {
        Self {
            access_token: SecretString::from(token.access_token),
            refresh_token: SecretString::from(token.refresh_token),
            token_type: token.token_type,
            device_token: token.device_token,
            expires_at: token.expires_at,
        }
    }
}

impl std::fmt::Debug for CachedToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedToken")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field("device_token", &self.device_token)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Manages reading and writing [`CachedToken`] values to a JSON file on disk.
///
/// On Unix systems, the token file is created with mode `0600` (owner read/write only).
#[derive(Debug, Clone)]
pub struct TokenCache {
    path: PathBuf,
}

impl TokenCache {
    /// Creates a new `TokenCache` that reads from and writes to the given file path.
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads a cached token from disk, returning `None` if the file does not exist,
    /// cannot be read, or contains malformed/unrecognized JSON. A corrupt cache is
    /// treated the same as an absent one so the caller can prompt the user to log in
    /// again rather than surfacing a raw deserialization error.
    pub fn load(&self) -> Result<Option<CachedToken>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&self.path)?;
        let on_disk: CachedTokenOnDisk = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %err,
                    "Token cache is corrupt or unreadable — treating as absent; \
                     run `rhood login` to create a fresh session"
                );
                return Ok(None);
            }
        };
        let token = CachedToken::from(on_disk);
        if let Some(exp) = token.expires_at
            && chrono::Utc::now().timestamp() >= exp
        {
            self.clear()?;
            return Ok(None);
        }
        Ok(Some(token))
    }

    /// Serializes and writes the given token to disk with restricted file permissions.
    ///
    /// Creates parent directories as needed so callers can pass a path whose
    /// ancestors do not yet exist (e.g. `~/.rhood/.rhood-token` on a fresh host).
    pub fn save(&self, token: &CachedToken) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let on_disk = CachedTokenOnDisk::from(token);
        let data = serde_json::to_string_pretty(&on_disk)?;
        Self::write_restricted(&self.path, data.as_bytes())?;
        Ok(())
    }

    #[cfg(unix)]
    fn write_restricted(path: &std::path::Path, data: &[u8]) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(data)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn write_restricted(path: &std::path::Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Deletes the cached token file from disk, if it exists.
    pub fn clear(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(access: &str, expires_at: Option<i64>) -> CachedToken {
        CachedToken {
            access_token: SecretString::from(access),
            refresh_token: SecretString::from("ref"),
            token_type: "Bearer".into(),
            device_token: "dev".into(),
            expires_at,
        }
    }

    #[test]
    fn round_trip_token_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TokenCache::with_path(dir.path().join("token.json"));

        assert!(cache.load().unwrap().is_none());

        let token = make_token("acc", Some(chrono::Utc::now().timestamp() + 3600));
        cache.save(&token).unwrap();

        let loaded = cache.load().unwrap().unwrap();
        assert_eq!(loaded.access_token.expose_secret(), "acc");

        cache.clear().unwrap();
        assert!(cache.load().unwrap().is_none());
    }

    #[test]
    fn expired_token_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TokenCache::with_path(dir.path().join("token.json"));

        let token = make_token("old", Some(0));
        cache.save(&token).unwrap();
        assert!(cache.load().unwrap().is_none());
    }

    #[test]
    fn corrupt_cache_returns_none_instead_of_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        // Write malformed JSON that will fail CachedTokenOnDisk deserialization.
        std::fs::write(&path, b"{\"not_a_token\": true}").unwrap();
        let cache = TokenCache::with_path(path);
        // A corrupt cache should be treated as absent, not as a hard error.
        let result = cache.load().unwrap();
        assert!(
            result.is_none(),
            "expected None for corrupt cache, got Some"
        );
    }

    #[test]
    fn completely_invalid_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"this is not json at all!!!").unwrap();
        let cache = TokenCache::with_path(path);
        assert!(cache.load().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn token_file_has_restricted_permissions() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());

        let token = make_token("secret", Some(chrono::Utc::now().timestamp() + 3600));
        cache.save(&token).unwrap();

        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600, "Token file should be owner-only (0600)");
    }
}
