//! Disk-backed OAuth token cache for the Robinhood API client.
//!
//! [`CachedToken`] holds the credential fields needed to restore a session,
//! and [`TokenCache`] manages reading and writing those tokens as a JSON file
//! with restricted file permissions on Unix.

use crate::Result;
#[cfg(unix)]
use crate::config::ensure_secret_file_permissions;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

impl CachedToken {
    pub(crate) fn access_token_is_expired_at(&self, unix_timestamp: i64) -> bool {
        self.expires_at
            .is_some_and(|expires_at| unix_timestamp >= expires_at)
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

    /// Loads a cached token from disk.
    ///
    /// Returns `None` if the file does not exist or contains malformed or unrecognized
    /// JSON. A corrupt cache is removed and treated as absent so the caller can prompt
    /// the user to log in again rather than surfacing a raw deserialization error.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache has insecure permissions or another filesystem
    /// I/O operation fails.
    pub fn load(&self) -> Result<Option<CachedToken>> {
        if !self.path.try_exists()? {
            return Ok(None);
        }
        #[cfg(unix)]
        ensure_secret_file_permissions(&self.path)?;
        let data = std::fs::read_to_string(&self.path)?;
        let on_disk: CachedTokenOnDisk = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %err,
                    "Token cache is corrupt or unrecognized so treating as absent; \
                     run `rhood login` to create a fresh session"
                );
                self.clear()?;
                return Ok(None);
            }
        };
        Ok(Some(CachedToken::from(on_disk)))
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

    fn write_restricted(path: &Path, data: &[u8]) -> Result<()> {
        use std::io::Write;

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::Builder::new()
            .prefix(".rhood-token-")
            .tempfile_in(parent)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Err(error) = temporary
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
            {
                return Self::fail_and_cleanup(temporary, error);
            }
        }

        if let Err(error) = temporary.write_all(data) {
            return Self::fail_and_cleanup(temporary, error);
        }
        if let Err(error) = temporary.as_file().sync_all() {
            return Self::fail_and_cleanup(temporary, error);
        }

        match temporary.persist(path) {
            Ok(file) => drop(file),
            Err(error) => return Self::fail_and_cleanup(error.file, error.error),
        }

        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;

        Ok(())
    }

    fn fail_and_cleanup(
        temporary: tempfile::NamedTempFile,
        operation_error: std::io::Error,
    ) -> Result<()> {
        if let Err(cleanup_error) = temporary.close() {
            tracing::warn!(%cleanup_error, "Failed to clean up token-cache temporary file");
        }
        Err(operation_error.into())
    }

    /// Deletes the cached token file from disk, if it exists.
    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
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

        let token = make_token("acc", Some(jiff::Timestamp::now().as_second() + 3600));
        cache.save(&token).unwrap();

        let loaded = cache.load().unwrap().unwrap();
        assert_eq!(loaded.access_token.expose_secret(), "acc");

        cache.clear().unwrap();
        assert!(cache.load().unwrap().is_none());
    }

    #[test]
    fn epoch_second_boundaries_are_stored_as_json_integers() {
        for (expires_at, expected) in [
            (i64::MIN, "-9223372036854775808"),
            (i64::MAX, "9223372036854775807"),
        ] {
            let serialized = serde_json::to_value(CachedTokenOnDisk::from(&make_token(
                "acc",
                Some(expires_at),
            )))
            .unwrap();
            assert_eq!(
                serialized["expires_at"].to_string(),
                expected,
                "epoch-second boundary {expires_at}"
            );
        }
    }

    #[test]
    fn save_atomically_replaces_existing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());

        cache.save(&make_token("old", None)).unwrap();
        cache.save(&make_token("new", None)).unwrap();

        let loaded = cache.load().unwrap().unwrap();
        assert_eq!(loaded.access_token.expose_secret(), "new");
    }

    #[test]
    fn failed_replacement_cleans_up_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), b"force replacement failure").unwrap();
        let cache = TokenCache::with_path(path.clone());

        assert!(cache.save(&make_token("new", None)).is_err());

        let entries = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![path.file_name().unwrap()]);
    }

    #[test]
    fn expired_access_token_loads_with_recovery_credentials_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());

        let token = make_token("old", Some(0));
        cache.save(&token).unwrap();
        let loaded = cache.load().unwrap().unwrap();

        assert!(loaded.access_token_is_expired_at(1));
        assert_eq!(loaded.refresh_token.expose_secret(), "ref");
        assert_eq!(loaded.device_token, "dev");
        assert!(path.exists());
    }

    #[test]
    fn corrupt_cache_returns_none_instead_of_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        // Write malformed JSON that will fail CachedTokenOnDisk deserialization.
        std::fs::write(&path, b"{\"not_a_token\": true}").unwrap();
        #[cfg(unix)]
        set_mode(&path, 0o600);
        let cache = TokenCache::with_path(path.clone());
        // A corrupt cache should be treated as absent, not as a hard error.
        let result = cache.load().unwrap();
        assert!(
            result.is_none(),
            "expected None for corrupt cache, got Some"
        );
        assert!(!path.exists(), "corrupt cache should be removed");
    }

    #[test]
    fn completely_invalid_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"this is not json at all!!!").unwrap();
        #[cfg(unix)]
        set_mode(&path, 0o600);
        let cache = TokenCache::with_path(path.clone());
        assert!(cache.load().unwrap().is_none());
        assert!(!path.exists(), "corrupt cache should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn corrupt_cache_cleanup_failure_is_propagated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"not-json").unwrap();
        set_mode(&path, 0o600);
        set_mode(dir.path(), 0o500);
        let cache = TokenCache::with_path(path.clone());

        let result = cache.load();

        set_mode(dir.path(), 0o700);
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            crate::RhoodError::Io(ref source)
                if source.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(path.exists());
    }

    #[cfg(unix)]
    fn set_mode(path: &std::path::Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_token_file_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());
        let token = make_token("secret", Some(jiff::Timestamp::now().as_second() + 3600));

        cache.save(&token).unwrap();
        set_mode(&path, 0o600);

        assert!(cache.load().unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_token_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());
        let token = make_token("secret", Some(jiff::Timestamp::now().as_second() + 3600));

        cache.save(&token).unwrap();
        set_mode(&path, 0o644);

        let error = cache.load().unwrap_err();
        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn save_tightens_existing_token_file_permissions() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());
        let token = make_token("secret", Some(jiff::Timestamp::now().as_second() + 3600));

        cache.save(&token).unwrap();
        set_mode(&path, 0o644);
        cache.save(&token).unwrap();

        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600, "Token file should be owner-only (0600)");
    }

    #[cfg(unix)]
    #[test]
    fn failed_save_preserves_existing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path);
        cache.save(&make_token("old", None)).unwrap();

        set_mode(dir.path(), 0o500);
        let result = cache.save(&make_token("new", None));
        set_mode(dir.path(), 0o700);

        assert!(result.is_err());
        let loaded = cache.load().unwrap().unwrap();
        assert_eq!(loaded.access_token.expose_secret(), "old");
    }

    #[cfg(unix)]
    #[test]
    fn token_file_has_restricted_permissions() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let cache = TokenCache::with_path(path.clone());

        let token = make_token("secret", Some(jiff::Timestamp::now().as_second() + 3600));
        cache.save(&token).unwrap();

        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600, "Token file should be owner-only (0600)");
    }
}
