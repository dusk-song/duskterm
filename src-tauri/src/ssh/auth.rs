use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::{
    client::{self, AuthResult},
    keys::{load_secret_key, PrivateKeyWithHashAlg},
};
use zeroize::Zeroize;

use crate::connection_log;

#[cfg(unix)]
fn ensure_private_key_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_key_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn normalize_auth_result(result: AuthResult) -> Result<(), String> {
    match result {
        AuthResult::Success => Ok(()),
        AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => Err(format!(
            "Authentication rejected by server (partial_success={}, remaining_methods={:?})",
            partial_success, remaining_methods
        )),
    }
}

pub(crate) async fn authenticate_session<H>(
    session_id: &str,
    session: &mut client::Handle<H>,
    username: String,
    private_key_path: Option<String>,
    mut password: Option<String>,
    mut passphrase: Option<String>,
) -> Result<(), String>
where
    H: client::Handler + Send + 'static,
    H::Error: std::fmt::Display,
{
    if passphrase
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        passphrase = None;
    }

    let result = if let Some(key_path) =
        private_key_path.and_then(|value| (!value.trim().is_empty()).then_some(value))
    {
        let key_path_buf = PathBuf::from(&key_path);
        if let Err(error) = ensure_private_key_permissions(&key_path_buf) {
            Err(format!("Private key permissions must be 0600: {}", error))
        } else {
            match load_secret_key(key_path, passphrase.as_deref()) {
                Ok(key_pair) => {
                    let key_algorithm = key_pair.algorithm();
                    if let Err(error) = crate::ssh_algorithms::validate_private_key_algorithm(&key_algorithm) {
                        Err(error)
                    } else {
                        match session.best_supported_rsa_hash().await {
                            Ok(hash_algorithm) => {
                                let hash_algorithm = hash_algorithm.flatten();
                                if key_algorithm.is_rsa() {
                                    connection_log::append(
                                        session_id,
                                        format!(
                                            "RSA authentication signature hash selected={:?}",
                                            hash_algorithm
                                        ),
                                    );
                                }
                                session
                                    .authenticate_publickey(
                                        username,
                                        PrivateKeyWithHashAlg::new(
                                            Arc::new(key_pair),
                                            hash_algorithm,
                                        ),
                                    )
                                    .await
                                    .map_err(|e| format!("Authentication failed: {}", e))
                                    .and_then(normalize_auth_result)
                            }
                            Err(error) => Err(format!(
                                "Failed to negotiate RSA signature algorithm: {}",
                                error
                            )),
                        }
                    }
                }
                Err(error) if passphrase.is_none() => Err(format!(
                    "Private key could not be loaded; an encrypted key may require a passphrase: {}",
                    error
                )),
                Err(error) => Err(format!("Failed to load private key: {}", error)),
            }
        }
    } else if let Some(password_value) = password.as_mut() {
        if password_value.trim().is_empty() {
            Err("Empty password not allowed".to_string())
        } else {
            session
                .authenticate_password(username, password_value.clone())
                .await
                .map_err(|e| format!("Authentication failed: {}", e))
                .and_then(normalize_auth_result)
        }
    } else {
        Err("No authentication method provided".to_string())
    };

    if let Some(value) = password.as_mut() {
        value.zeroize();
    }
    if let Some(value) = passphrase.as_mut() {
        value.zeroize();
    }

    if let Err(error) = &result {
        connection_log::append(
            session_id,
            format!("authentication failed details={}", error),
        );
    }

    result
}
