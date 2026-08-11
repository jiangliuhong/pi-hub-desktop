//! Pure in-memory `CredentialStore` used by tests and as the non-Apple fallback
//! so the crate compiles on Linux. The production store on Apple targets is
//! the Keychain implementation (design §10.1).

use crate::credential::{
    CredentialError, CredentialId, CredentialKind, CredentialStore, SecretValue,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// Keyed by `(credential id, kind)` so a single credential id may carry
/// multiple kinds (e.g. private key + passphrase) without collision.
#[derive(Default)]
pub struct InMemoryCredentialStore {
    inner: Mutex<HashMap<(String, CredentialKind), Vec<u8>>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        InMemoryCredentialStore::default()
    }
}

#[async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn put(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
        value: SecretValue,
    ) -> Result<(), CredentialError> {
        let bytes = value.into_secret();
        let mut guard = self.inner.lock().expect("credential mutex poisoned");
        guard.insert((id.0.clone(), kind), bytes);
        Ok(())
    }

    async fn get(
        &self,
        id: &CredentialId,
        kind: CredentialKind,
    ) -> Result<SecretValue, CredentialError> {
        let guard = self.inner.lock().expect("credential mutex poisoned");
        guard
            .get(&(id.0.clone(), kind))
            .map(|b| SecretValue::new(kind, b.clone()))
            .ok_or(CredentialError::NotFound)
    }

    async fn delete(&self, id: &CredentialId, kind: CredentialKind) -> Result<(), CredentialError> {
        let mut guard = self.inner.lock().expect("credential mutex poisoned");
        guard.remove(&(id.0.clone(), kind));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_delete_roundtrip() {
        let store = InMemoryCredentialStore::new();
        let id = CredentialId::new();
        let v = SecretValue::new(CredentialKind::SshPassword, b"hunter2".to_vec());
        store
            .put(&id, CredentialKind::SshPassword, v)
            .await
            .unwrap();
        let got = store.get(&id, CredentialKind::SshPassword).await.unwrap();
        assert_eq!(got.secret_bytes(), b"hunter2");
        store
            .delete(&id, CredentialKind::SshPassword)
            .await
            .unwrap();
        assert!(matches!(
            store.get(&id, CredentialKind::SshPassword).await,
            Err(CredentialError::NotFound)
        ));
    }

    #[tokio::test]
    async fn different_kinds_are_isolated() {
        let store = InMemoryCredentialStore::new();
        let id = CredentialId::new();
        store
            .put(
                &id,
                CredentialKind::SshPrivateKey,
                SecretValue::new(CredentialKind::SshPrivateKey, b"KEY".to_vec()),
            )
            .await
            .unwrap();
        store
            .put(
                &id,
                CredentialKind::SshKeyPassphrase,
                SecretValue::new(CredentialKind::SshKeyPassphrase, b"PASS".to_vec()),
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&id, CredentialKind::SshPrivateKey)
                .await
                .unwrap()
                .secret_bytes(),
            b"KEY"
        );
        assert_eq!(
            store
                .get(&id, CredentialKind::SshKeyPassphrase)
                .await
                .unwrap()
                .secret_bytes(),
            b"PASS"
        );
    }
}
