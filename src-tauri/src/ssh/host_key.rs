//! SSH host-key fingerprinting and known-host comparison
//! (docs/design-v1.md §9, AGENTS.md §6.2).
//!
//! Hard rules (AGENTS.md §6.2):
//! - First connection must show algorithm + SHA-256 fingerprint and require
//!   explicit user confirmation.
//! - Known host keys are bound by `(host, port)`.
//! - A changed host key must block — never auto-accept, never equivalent to
//!   `StrictHostKeyChecking=no`.

use crate::error::SshError;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Utc};
use russh::keys::PublicKey;
use russh::keys::PublicKeyBase64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A trusted known-host record. Non-secret: the public key and fingerprint
/// may be persisted in the (non-sensitive) profile store (design §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHostRecord {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    #[serde(with = "serde_bytes_compat")]
    pub public_key: Vec<u8>,
    pub sha256_fingerprint: String,
    pub trusted_at: DateTime<Utc>,
}

/// SSH wire algorithm name for a public key (e.g. `ssh-ed25519`, `rsa-sha2-512`).
pub fn algorithm_name(key: &PublicKey) -> String {
    key.algorithm().as_str().to_string()
}

/// SSH SHA-256 fingerprint in the OpenSSH `SHA256:base64` format.
pub fn sha256_fingerprint(key: &PublicKey) -> String {
    // Computed over the canonical SSH wire encoding of the public key so it
    // matches `ssh-keygen -lf` and is stable across implementations.
    let wire = key.public_key_bytes();
    let digest = Sha256::digest(&wire);
    format!("SHA256:{}", B64.encode(digest))
}

/// Outcome of comparing a server-presented key against a known host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyCheck {
    /// No prior record: the user must confirm before trusting.
    Unknown(HostKeyFacts),
    /// Matches a previously confirmed key.
    Matched,
    /// A key was previously confirmed but differs from the server's key.
    /// Connection must be blocked (FR-008).
    Changed {
        expected: HostKeyFacts,
        presented: HostKeyFacts,
    },
}

/// Non-secret facts shown to the user for first-time confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyFacts {
    pub algorithm: String,
    pub sha256_fingerprint: String,
}

impl HostKeyFacts {
    pub fn from_key(key: &PublicKey) -> Self {
        HostKeyFacts {
            algorithm: algorithm_name(key),
            sha256_fingerprint: sha256_fingerprint(key),
        }
    }
}

/// Compare a presented key against a stored known host for `(host, port)`.
pub fn check_known_host(
    host: &str,
    port: u16,
    known: Option<&KnownHostRecord>,
    presented: &PublicKey,
) -> Result<HostKeyCheck, SshError> {
    let facts = HostKeyFacts::from_key(presented);
    let Some(record) = known else {
        return Ok(HostKeyCheck::Unknown(facts));
    };
    // A record for another endpoint is not evidence of trust here. Treat the
    // current endpoint as unknown so the user must explicitly confirm it.
    if record.host != host || record.port != port {
        return Ok(HostKeyCheck::Unknown(facts));
    }
    let presented_wire = presented.public_key_bytes();
    if record.public_key == presented_wire
        && record.algorithm == facts.algorithm
        && record.sha256_fingerprint == facts.sha256_fingerprint
    {
        return Ok(HostKeyCheck::Matched);
    }
    // Mismatch: block. Provide both old and new facts for the replacement flow.
    Ok(HostKeyCheck::Changed {
        expected: HostKeyFacts {
            algorithm: record.algorithm.clone(),
            sha256_fingerprint: record.sha256_fingerprint.clone(),
        },
        presented: facts,
    })
}

/// Build a record to persist once the user has explicitly confirmed a key.
pub fn record_from_key(
    host: String,
    port: u16,
    key: &PublicKey,
) -> (KnownHostRecord, HostKeyFacts) {
    let facts = HostKeyFacts::from_key(key);
    let record = KnownHostRecord {
        host,
        port,
        algorithm: facts.algorithm.clone(),
        public_key: key.public_key_bytes(),
        sha256_fingerprint: facts.sha256_fingerprint.clone(),
        trusted_at: Utc::now(),
    };
    (record, facts)
}

/// Serde helper to store the public-key wire bytes as base64 (stable across
/// platforms and human-inspectable), while keeping the in-memory type `Vec<u8>`.
mod serde_bytes_compat {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        B64.encode(v).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        B64.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::Algorithm;
    use russh::keys::PrivateKey;

    fn ed25519_keypair() -> PrivateKey {
        // Deterministic-ish ephemeral key just for fingerprint comparison tests.
        PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519 {}).unwrap()
    }

    fn to_public(priv_key: &PrivateKey) -> PublicKey {
        priv_key.public_key().clone()
    }

    #[test]
    fn fingerprint_is_ssh_sha256_format() {
        let key = ed25519_keypair();
        let pub_key = to_public(&key);
        let fp = sha256_fingerprint(&pub_key);
        assert!(fp.starts_with("SHA256:"), "{fp}");
        // base64 digest of 32 bytes is 44 chars without padding.
        let rest = fp.trim_start_matches("SHA256:");
        assert!(!rest.is_empty() && !rest.contains(' '));
    }

    #[test]
    fn unknown_when_no_record() {
        let key = ed25519_keypair();
        let pub_key = to_public(&key);
        let check = check_known_host("host", 22, None, &pub_key).unwrap();
        assert!(matches!(check, HostKeyCheck::Unknown(_)));
    }

    #[test]
    fn matched_when_same_key() {
        let key = ed25519_keypair();
        let pub_key = to_public(&key);
        let (record, _) = record_from_key("host".into(), 22, &pub_key);
        let check = check_known_host("host", 22, Some(&record), &pub_key).unwrap();
        assert_eq!(check, HostKeyCheck::Matched);
    }

    #[test]
    fn changed_when_different_key() {
        let a = ed25519_keypair();
        let b = ed25519_keypair();
        let pub_a = to_public(&a);
        let pub_b = to_public(&b);
        let (record, _) = record_from_key("host".into(), 22, &pub_a);
        let check = check_known_host("host", 22, Some(&record), &pub_b).unwrap();
        match check {
            HostKeyCheck::Changed {
                expected,
                presented,
            } => {
                assert_ne!(expected.sha256_fingerprint, presented.sha256_fingerprint);
            }
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn same_key_on_another_endpoint_is_unknown() {
        let key = ed25519_keypair();
        let pub_key = to_public(&key);
        let (record, _) = record_from_key("host-a".into(), 22, &pub_key);

        let check = check_known_host("host-b", 22, Some(&record), &pub_key).unwrap();
        assert!(matches!(check, HostKeyCheck::Unknown(_)));
    }

    #[test]
    fn record_round_trips_through_json() {
        let key = ed25519_keypair();
        let pub_key = to_public(&key);
        let (record, _) = record_from_key("vps".into(), 22, &pub_key);
        let json = serde_json::to_string(&record).unwrap();
        // public key stored as base64, not raw bytes array, for stability.
        assert!(json.contains("\"public_key\":\""));
        let back: KnownHostRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, record);
    }
}
