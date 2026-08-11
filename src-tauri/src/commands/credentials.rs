//! Credential commands (docs/design-v1.md §13.1, §10).
//!
//! Thin adapters over [`crate::credential::CredentialStore`]. Secret bytes are
//! accepted, stored to Keychain, and immediately dropped from command memory.
//! The frontend clears its input state right after calling these
//! (AGENTS.md §6.1).

use crate::commands::profiles::map_err;
use crate::credential::{CredentialId, CredentialKind, CredentialStore, SecretValue};
use crate::error::{AppError, CredentialError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PutCredentialKind {
    SshPassword,
    SshPrivateKey,
    SshKeyPassphrase,
    PiHubPassword,
}

impl PutCredentialKind {
    fn into_domain(self) -> CredentialKind {
        match self {
            PutCredentialKind::SshPassword => CredentialKind::SshPassword,
            PutCredentialKind::SshPrivateKey => CredentialKind::SshPrivateKey,
            PutCredentialKind::SshKeyPassphrase => CredentialKind::SshKeyPassphrase,
            PutCredentialKind::PiHubPassword => CredentialKind::PiHubPassword,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PutCredentialRequest {
    pub credential_id: String,
    pub kind: PutCredentialKind,
    /// Secret value. For passwords this is UTF-8 text; for private keys it is
    /// the OpenSSH PEM text. Never logged (SR-005).
    pub secret: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PutCredentialResponse {
    pub credential_id: String,
}

#[tauri::command]
pub async fn put_credential(
    store: tauri::State<'_, Arc<dyn CredentialStore>>,
    request: PutCredentialRequest,
) -> Result<PutCredentialResponse, crate::error::ErrorDto> {
    let id = CredentialId(request.credential_id.clone());
    let kind = request.kind.into_domain();
    // Hold the secret for the minimum scope: wrap, store, drop.
    let value = SecretValue::new(kind, request.secret.into_bytes());
    store.put(&id, kind, value).await.map_err(map_err)?;
    Ok(PutCredentialResponse {
        credential_id: id.0,
    })
}

#[tauri::command]
pub async fn delete_credential(
    store: tauri::State<'_, Arc<dyn CredentialStore>>,
    credential_id: String,
    kind: PutCredentialKind,
) -> Result<(), crate::error::ErrorDto> {
    let id = CredentialId(credential_id);
    store
        .delete(&id, kind.into_domain())
        .await
        .map_err(map_err)?;
    Ok(())
}

/// Internal helper used by the delete flow: drop a credential if no remaining
/// profile references it. Not a Tauri command — called from the manager/profile
/// teardown. Returns Ok if the item was missing (idempotent).
pub async fn drop_if_orphan(
    store: &Arc<dyn CredentialStore>,
    id: &str,
    kind: CredentialKind,
) -> Result<(), AppError> {
    let cid = CredentialId(id.to_string());
    match store.delete(&cid, kind).await {
        Ok(()) => Ok(()),
        Err(CredentialError::NotFound) => Ok(()),
        Err(e) => Err(AppError::from(e)),
    }
}

/// Delete every known kind for a credential id. A credential id may carry
/// multiple kinds (e.g. `ssh-private-key` + `ssh-key-passphrase`); deleting all
/// kinds is idempotent (missing items are not errors). Used by the delete and
/// update flows to purge Keychain items no longer referenced by any profile
/// (FR-004, design §10.3).
pub async fn purge_credential(store: &Arc<dyn CredentialStore>, id: &str) {
    for kind in [
        CredentialKind::SshPassword,
        CredentialKind::SshPrivateKey,
        CredentialKind::SshKeyPassphrase,
        CredentialKind::PiHubPassword,
    ] {
        if let Err(e) = drop_if_orphan(store, id, kind).await {
            // Best-effort: a failed Keychain delete must not roll back the
            // already-applied profile change. Log the non-sensitive id only.
            tracing::warn!(credential_id = id, error = ?e, "failed to purge credential");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::in_memory::InMemoryCredentialStore;

    #[tokio::test]
    async fn purge_deletes_all_kinds_idempotently() {
        let store: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::new());
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

        purge_credential(&store, id.as_str()).await;

        // All kinds gone.
        assert!(matches!(
            store.get(&id, CredentialKind::SshPrivateKey).await,
            Err(CredentialError::NotFound)
        ));
        assert!(matches!(
            store.get(&id, CredentialKind::SshKeyPassphrase).await,
            Err(CredentialError::NotFound)
        ));
        // Idempotent: purging a non-existent id is a no-op.
        purge_credential(&store, "never-existed").await;
    }
}
