use std::path::Path;

use secrecy::SecretString;

use crate::{Result, RhoodError};

/// Rejects a secret-bearing file that grants any group or other access.
///
/// On non-Unix platforms, filesystem permission bits are not portable, so
/// this check is intentionally a no-op.
pub fn ensure_secret_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(RhoodError::InvalidParameter(format!(
                "Secret-bearing file {} has insecure permissions {mode:04o}; run `chmod 600 {}` to restrict access",
                path.display(),
                path.display()
            )));
        }
    }

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

fn read_secret_file(path: &str, field_name: &str) -> Result<String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(RhoodError::InvalidParameter(format!(
            "Secret file not found: {path} (from {field_name})"
        )));
    }
    ensure_secret_file_permissions(p)?;
    std::fs::read_to_string(p)
        .map(|contents| contents.trim().to_string())
        .map_err(|e| {
            RhoodError::InvalidParameter(format!("Failed to read secret file {path}: {e}"))
        })
}

/// Resolves a secret value from either a direct value or a file path.
///
/// If both `direct` and `file_path` are set, returns an error. If only `file_path`
/// is set, reads the file, trims whitespace, and stores the result in `direct`.
pub fn resolve_secret(
    direct: &mut Option<SecretString>,
    file_path: Option<String>,
    field_name: &str,
) -> Result<()> {
    let file_field = format!("{field_name}_file");
    match (direct.is_some(), file_path) {
        (true, Some(_)) => Err(RhoodError::InvalidParameter(format!(
            "Conflicting config: both '{field_name}' and '{file_field}' are set. Use one or the other."
        ))),
        (false, Some(path)) => {
            *direct = Some(SecretString::from(read_secret_file(&path, &file_field)?));
            Ok(())
        }
        _ => Ok(()),
    }
}
