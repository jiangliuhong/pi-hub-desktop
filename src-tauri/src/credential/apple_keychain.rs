//! Apple Keychain-backed `CredentialStore` (docs/design-v1.md §10.1).
//!
//! Compiled only on Apple targets (`target_vendor = "apple"`). Uses the
//! `security-framework` crate which is pure-Darwin and supports both macOS and
//! iOS password item APIs. Secrets are stored as generic password items keyed
//! by the bundle-id `service` and a per-credential `account` path
//! (design §6.2): `credential/<uuid>/<kind>`.
//!
//! Verification status: the module compiles on Apple targets. It must be
//! validated on a real macOS and iPhone device per AGENTS.md §12.4 / §15
//! before claiming V1 complete; the Linux CI cannot exercise it.

use crate::credential::{
    CredentialError, CredentialId, CredentialKind, CredentialStore, SecretValue,
};
use crate::APP_BUNDLE_ID;
use async_trait::async_trait;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain-backed store. All items share the fixed `service` (bundle id);
/// `account` disambiguates credential id + kind.
#[derive(Default)]
pub struct AppleKeychainStore;

impl AppleKeychainStore {
    pub fn new() -> Self {
        AppleKeychainStore
    }

    fn account(id: &CredentialId, kind: CredentialKind) -> String {
        format!("credential/{}/{}", id.as_str(), kind.as_str())
    }
}

#[async_trait]
impl CredentialStore for AppleKeychainStore {
    async fn put(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
        value: SecretValue,
    ) -> Result<(), CredentialError> {
        let account = Self::account(id, kind);
        // `set_generic_password` overwrites an existing item with the same
        // service+account, which is the desired update semantics.
        let mut bytes = value.into_secret();
        let result = set_generic_password(APP_BUNDLE_ID, &account, &bytes)
            .map_err(|e| CredentialError::Backend(e.to_string()));
        bytes.fill(0);
        result
    }

    async fn get(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
    ) -> Result<SecretValue, CredentialError> {
        let account = Self::account(id, kind);
        let bytes = get_generic_password(APP_BUNDLE_ID, &account).map_err(|e| match e.code() {
            // `errSecItemNotFound` surfaces as NotFound.
            -25300 => CredentialError::NotFound,
            _ => CredentialError::Backend(e.to_string()),
        })?;
        Ok(SecretValue::new(kind, bytes))
    }

    async fn delete(&self, id: &CredentialId, kind: CredentialKind) -> Result<(), CredentialError> {
        let account = Self::account(id, kind);
        match delete_generic_password(APP_BUNDLE_ID, &account) {
            Ok(()) => Ok(()),
            // Missing item is not an error (idempotent delete).
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }
}
