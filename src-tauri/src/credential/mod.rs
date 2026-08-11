//! Credential storage abstraction (docs/design-v1.md §10).
//!
//! Secrets live only in Apple Keychain on real devices; profiles hold
//! references. This module defines the abstract `CredentialStore` trait plus a
//! pure in-memory implementation used by tests and the connection layer's
//! unit tests. The Apple Keychain implementation is compiled only on Apple
//! targets (`#[cfg(target_vendor = "apple")]`) and selected as the default
//! store there (design §10.1).

pub mod in_memory;

#[cfg(target_vendor = "apple")]
pub mod apple_keychain;

use crate::error::CredentialError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Kind tag embedded in the Keychain account path (design §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialKind {
    SshPassword,
    SshPrivateKey,
    SshKeyPassphrase,
    PiHubPassword,
}

impl CredentialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialKind::SshPassword => "ssh-password",
            CredentialKind::SshPrivateKey => "ssh-private-key",
            CredentialKind::SshKeyPassphrase => "ssh-key-passphrase",
            CredentialKind::PiHubPassword => "pi-hub-password",
        }
    }
}

impl fmt::Display for CredentialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Opaque credential id. Stable, non-secret; safe to store in the profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialId(pub String);

impl CredentialId {
    pub fn new() -> Self {
        CredentialId(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CredentialId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A secret value held briefly in memory while being stored or used. Never
/// serialized into profiles, logs or events (AGENTS.md §6.1).
#[derive(Debug, Clone)]
pub struct SecretValue {
    kind: CredentialKind,
    bytes: Vec<u8>,
}

impl SecretValue {
    pub fn new(kind: CredentialKind, bytes: Vec<u8>) -> Self {
        SecretValue { kind, bytes }
    }

    pub fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// Borrow the secret bytes. Callers must not persist or log them.
    pub fn secret_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume into the raw bytes. Best-effort wipe happens on drop of any
    /// remaining `SecretValue`; callers take ownership and must not persist or
    /// log the returned bytes (AGENTS.md §6.1).
    pub fn into_secret(mut self) -> Vec<u8> {
        // Take the bytes out without moving out of a `Drop` type.
        std::mem::take(&mut self.bytes)
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        // Best-effort wipe of the in-memory buffer.
        for b in self.bytes.iter_mut() {
            *b = 0;
        }
    }
}

/// Abstract credential store (design §10.1).
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// Store a secret under `(id, kind)`. Returns the credential id.
    async fn put(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
        value: SecretValue,
    ) -> Result<(), CredentialError>;

    /// Load a secret. Returns `NotFound` if missing.
    async fn get(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
    ) -> Result<SecretValue, CredentialError>;

    /// Delete a secret. Missing is not an error.
    async fn delete(&self, id: &CredentialId, kind: CredentialKind) -> Result<(), CredentialError>;
}

/// Construct the platform default credential store. On Apple targets this is
/// the Keychain-backed store; elsewhere it returns an in-memory store so the
/// crate compiles and tests run on Linux (the real device always uses
/// Keychain — see AGENTS.md §15 about honest verification).
pub fn default_store() -> Box<dyn CredentialStore> {
    #[cfg(target_vendor = "apple")]
    {
        return Box::new(apple_keychain::AppleKeychainStore::new());
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        Box::new(in_memory::InMemoryCredentialStore::new())
    }
}
