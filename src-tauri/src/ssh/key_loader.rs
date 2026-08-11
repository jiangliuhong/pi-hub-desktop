//! OpenSSH private-key decoding (docs/design-v1.md §10.2, AGENTS.md §7.1).
//!
//! Supports OpenSSH Ed25519 and RSA private keys, including encrypted keys that
//! require a passphrase. Decoding never logs key material; only a credential id
//! / kind is ever logged at the call site (AGENTS.md §6.1, SR-005).

use crate::error::SshError;
use russh::keys::PrivateKey;
use russh::keys::{decode_secret_key, Error as RusshKeyError};

/// Decode an OpenSSH private key PEM. `passphrase` is required for encrypted
/// keys and ignored for plaintext keys.
///
/// Maps decoding failures to actionable error codes:
/// - bad format / unsupported → `PrivateKeyInvalid`
/// - encrypted but no/wrong passphrase → `PrivateKeyPassphraseRequired`
pub fn decode(pem: &str, passphrase: Option<&str>) -> Result<PrivateKey, SshError> {
    decode_secret_key(pem, passphrase).map_err(|e| map_key_error(e, passphrase))
}

fn map_key_error(e: RusshKeyError, passphrase: Option<&str>) -> SshError {
    let msg = e.to_string().to_ascii_lowercase();
    // Encrypted-key decryption failures surface with password/passphrase/encrypted
    // wording depending on whether a passphrase was supplied.
    if msg.contains("password") || msg.contains("passphrase") || msg.contains("encrypted") {
        return SshError::PrivateKeyPassphraseRequired;
    }
    let _ = passphrase;
    SshError::PrivateKeyInvalid
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::ssh_key::LineEnding;
    use russh::keys::Algorithm;

    fn fresh_ed25519_pem() -> String {
        let key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519).unwrap();
        key.to_openssh(LineEnding::LF).unwrap().to_string()
    }

    #[test]
    fn decodes_plaintext_ed25519() {
        let pem = fresh_ed25519_pem();
        let key = decode(&pem, None).expect("decode");
        assert_eq!(key.algorithm().as_str(), "ssh-ed25519");
    }

    #[test]
    fn decodes_same_key_with_unneeded_passphrase() {
        // A plaintext key ignores the passphrase.
        let pem = fresh_ed25519_pem();
        let key = decode(&pem, Some("ignored")).expect("decode");
        assert_eq!(key.algorithm().as_str(), "ssh-ed25519");
    }

    #[test]
    fn rejects_garbage_as_invalid() {
        let err = decode("not a key", None).unwrap_err();
        assert!(matches!(err, SshError::PrivateKeyInvalid));
    }
}
