use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use russh::{cipher, client, compression, kex, mac, AlgorithmKind, Preferred};
use russh::keys::{Algorithm, EcdsaCurve, HashAlg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegotiationProfile {
    Modern,
    LegacyRsaSha1,
}

const MODERN_KEX_ALGOS: &[kex::Name] = &[
    kex::MLKEM768X25519_SHA256,
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_G16_SHA512,
    kex::DH_G14_SHA256,
];

const MODERN_HOST_KEY_ALGOS: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
];

const LEGACY_RSA_SHA1_HOST_KEY_ALGOS: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP256,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP384,
    },
    Algorithm::Ecdsa {
        curve: EcdsaCurve::NistP521,
    },
    Algorithm::Rsa { hash: None },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha512),
    },
    Algorithm::Rsa {
        hash: Some(HashAlg::Sha256),
    },
];

const MODERN_CIPHER_ALGOS: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_256_CTR,
    cipher::AES_128_CTR,
];

const MODERN_MAC_ALGOS: &[mac::Name] = &[
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512,
    mac::HMAC_SHA256,
];

const MODERN_COMPRESSION_ALGOS: &[compression::Name] = &[compression::NONE];
const DEFAULT_CLIENT_KEEPALIVE_SECS: u64 = 0;

#[derive(Clone, Default)]
pub struct NegotiationProfileCache {
    inner: Arc<Mutex<HashMap<String, NegotiationProfile>>>,
}

impl NegotiationProfileCache {
    pub fn preferred_profile_for_endpoint(&self, host: &str, port: u16) -> NegotiationProfile {
        self.inner
            .lock()
            .unwrap()
            .get(&endpoint_cache_key(host, port))
            .copied()
            .unwrap_or(NegotiationProfile::Modern)
    }

    pub fn remember_successful_profile(
        &self,
        host: &str,
        port: u16,
        profile: NegotiationProfile,
    ) {
        self.inner
            .lock()
            .unwrap()
            .insert(endpoint_cache_key(host, port), profile);
    }
}

#[derive(Debug)]
pub enum ConnectAttemptError {
    Russh(russh::Error),
    Timeout,
}

impl fmt::Display for ConnectAttemptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Russh(error) => write!(f, "{}", error),
            Self::Timeout => write!(f, "Connection timeout"),
        }
    }
}

impl std::error::Error for ConnectAttemptError {}

impl From<russh::Error> for ConnectAttemptError {
    fn from(value: russh::Error) -> Self {
        Self::Russh(value)
    }
}

pub fn build_client_config(
    keep_alive_interval: Option<u64>,
    profile: NegotiationProfile,
) -> client::Config {
    let mut client_config = client::Config::default();
    client_config.preferred = Preferred {
        kex: Cow::Borrowed(MODERN_KEX_ALGOS),
        key: Cow::Borrowed(host_key_algorithms_for(profile)),
        cipher: Cow::Borrowed(MODERN_CIPHER_ALGOS),
        mac: Cow::Borrowed(MODERN_MAC_ALGOS),
        compression: Cow::Borrowed(MODERN_COMPRESSION_ALGOS),
    };

    // russh's config keepalive is driven by receive silence. DuskTerm instead
    // supervises fixed outbound deadlines from the owning session runtime.
    let _ = keep_alive_interval;

    client_config
}

pub(crate) fn effective_keepalive_interval(keep_alive_interval: Option<u64>) -> u64 {
    match keep_alive_interval {
        Some(value) => value,
        None => DEFAULT_CLIENT_KEEPALIVE_SECS,
    }
}

pub fn validate_private_key_algorithm(algorithm: &Algorithm) -> Result<(), String> {
    let is_allowed = matches!(
        algorithm,
        Algorithm::Ed25519 | Algorithm::Ecdsa { .. } | Algorithm::Rsa { .. }
    );

    if is_allowed {
        Ok(())
    } else {
        Err(format!(
            "Key type {} not allowed. Supported private keys: Ed25519, ECDSA, RSA.",
            algorithm
        ))
    }
}

pub fn should_retry_with_legacy(
    profile: NegotiationProfile,
    error: &ConnectAttemptError,
) -> bool {
    if profile != NegotiationProfile::Modern {
        return false;
    }

    matches!(
        error,
        ConnectAttemptError::Russh(russh::Error::NoCommonAlgo {
            kind: AlgorithmKind::Key,
            ..
        } | russh::Error::UnknownAlgo
            | russh::Error::KexInit
            | russh::Error::WrongServerSig)
    )
}

fn endpoint_cache_key(host: &str, port: u16) -> String {
    format!("{}:{}", host.trim().to_ascii_lowercase(), port)
}

fn host_key_algorithms_for(profile: NegotiationProfile) -> &'static [Algorithm] {
    match profile {
        NegotiationProfile::Modern => MODERN_HOST_KEY_ALGOS,
        NegotiationProfile::LegacyRsaSha1 => LEGACY_RSA_SHA1_HOST_KEY_ALGOS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_profile_excludes_ssh_rsa_sha1() {
        let modern = host_key_algorithms_for(NegotiationProfile::Modern);

        assert!(modern.iter().any(|algorithm| {
            matches!(
                algorithm,
                Algorithm::Rsa {
                    hash: Some(HashAlg::Sha512)
                }
            )
        }));
        assert!(modern.iter().any(|algorithm| {
            matches!(
                algorithm,
                Algorithm::Rsa {
                    hash: Some(HashAlg::Sha256)
                }
            )
        }));
        assert!(!modern
            .iter()
            .any(|algorithm| matches!(algorithm, Algorithm::Rsa { hash: None })));
    }

    #[test]
    fn legacy_retry_only_applies_to_modern_negotiation_failures() {
        let retryable = ConnectAttemptError::Russh(russh::Error::NoCommonAlgo {
            kind: AlgorithmKind::Key,
            ours: vec![],
            theirs: vec![],
        });

        assert!(should_retry_with_legacy(
            NegotiationProfile::Modern,
            &retryable
        ));
        assert!(!should_retry_with_legacy(
            NegotiationProfile::LegacyRsaSha1,
            &retryable
        ));
    }

    #[test]
    fn rsa_private_keys_are_allowed() {
        assert!(validate_private_key_algorithm(&Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        })
        .is_ok());
    }
}
