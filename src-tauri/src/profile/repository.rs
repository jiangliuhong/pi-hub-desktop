//! Atomic, versioned profile store (docs/design-v1.md §11).
//!
//! The store holds non-sensitive profile data and known hosts. Persistence is
//! an atomic read-validate-write: load + migrate + validate, mutate in memory,
//! then write to a temp file and rename. On write failure the in-memory state
//! is left untouched (design §11).

use crate::error::ProfileError;
use crate::profile::migration::migrate_to_current;
use crate::profile::model::{ProfileInput, ServiceProfile, CURRENT_SCHEMA_VERSION};
use crate::ssh::host_key::KnownHostRecord;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

/// On-disk persisted state. Secrets are never stored here (AGENTS.md §6.1).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredState {
    pub schema_version: u32,
    #[serde(default)]
    pub profiles: Vec<ServiceProfile>,
    #[serde(default)]
    pub known_hosts: Vec<KnownHostRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_service_id: Option<Uuid>,
}

impl StoredState {
    /// Create a fresh v1 store with no profiles.
    pub fn new() -> Self {
        StoredState {
            schema_version: CURRENT_SCHEMA_VERSION,
            profiles: Vec::new(),
            known_hosts: Vec::new(),
            last_opened_service_id: None,
        }
    }

    fn from_json(json: &str) -> Result<Self, ProfileError> {
        let value = serde_json::from_str::<serde_json::Value>(json)
            .map_err(|e| ProfileError::Storage(format!("invalid json: {e}")))?;
        let migrated = migrate_to_current(value)?;
        serde_json::from_value::<StoredState>(migrated)
            .map_err(|e| ProfileError::Storage(format!("schema mismatch: {e}")))
    }

    fn to_json(&self) -> Result<String, ProfileError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| ProfileError::Storage(format!("serialize: {e}")))
    }

    fn find_index(&self, id: Uuid) -> Option<usize> {
        self.profiles.iter().position(|p| p.id() == id)
    }
}

/// Profile store: atomic persistence of non-sensitive service configuration.
///
/// Behind the `in_memory` flag this also serves as the test double used by the
/// connection and command tests (design §22.1).
pub struct ProfileStore {
    path: Option<PathBuf>,
    state: tokio::sync::RwLock<StoredState>,
}

impl ProfileStore {
    /// Create a store backed by a file on disk.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        ProfileStore {
            path: Some(path.into()),
            state: tokio::sync::RwLock::new(StoredState::new()),
        }
    }

    /// Create an in-memory store (tests, and the runtime before first load).
    pub fn in_memory() -> Self {
        ProfileStore {
            path: None,
            state: tokio::sync::RwLock::new(StoredState::new()),
        }
    }

    /// Load (or initialize) the store from disk. Missing file => empty store.
    pub async fn load(&self) -> Result<(), ProfileError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let loaded = match fs::read_to_string(path).await {
            Ok(s) if !s.trim().is_empty() => StoredState::from_json(&s)?,
            Ok(_) => StoredState::new(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoredState::new(),
            Err(e) => {
                return Err(ProfileError::Storage(format!("read: {e}")));
            }
        };
        *self.state.write().await = loaded;
        Ok(())
    }

    async fn persist(&self, state: &StoredState) -> Result<(), ProfileError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        atomic_write(path, &state.to_json()?).await
    }

    /// List all profiles (snapshot copy).
    pub async fn list(&self) -> Result<Vec<ServiceProfile>, ProfileError> {
        Ok(self.state.read().await.profiles.clone())
    }

    pub async fn get(&self, id: Uuid) -> Result<ServiceProfile, ProfileError> {
        let guard = self.state.read().await;
        guard
            .find_index(id)
            .map(|i| guard.profiles[i].clone())
            .ok_or(ProfileError::NotFound)
    }

    /// Create a profile from input, validate and persist atomically.
    pub async fn create(&self, input: ProfileInput) -> Result<ServiceProfile, ProfileError> {
        let profile = input.into_profile();
        profile.validate()?;
        let mut state = self.state.write().await;
        state.profiles.push(profile.clone());
        self.persist(&state).await?;
        Ok(profile)
    }

    /// Replace an existing profile (full update). Validates the new value.
    /// Replace an existing profile (full update). Validates the new value and
    /// preserves the original `created_at` (only `updated_at` is bumped).
    pub async fn update(&self, profile: ServiceProfile) -> Result<ServiceProfile, ProfileError> {
        profile.validate()?;
        let mut state = self.state.write().await;
        let idx = state
            .find_index(profile.id())
            .ok_or(ProfileError::NotFound)?;
        let original_created_at = state.profiles[idx].metadata().created_at;
        let mut updated = profile.clone();
        {
            let meta = updated.metadata_mut();
            meta.created_at = original_created_at;
            meta.updated_at = Utc::now();
        }
        state.profiles[idx] = updated.clone();
        self.persist(&state).await?;
        Ok(updated)
    }

    /// Delete a profile. Returns the deleted profile so callers can compute
    /// credential-reference cleanup (design §10.3).
    pub async fn delete(&self, id: Uuid) -> Result<ServiceProfile, ProfileError> {
        let mut state = self.state.write().await;
        let idx = state.find_index(id).ok_or(ProfileError::NotFound)?;
        let removed = state.profiles.remove(idx);
        self.persist(&state).await?;
        Ok(removed)
    }

    // ---- known hosts (non-secret, stored alongside profiles) ----

    pub async fn list_known_hosts(&self) -> Result<Vec<KnownHostRecord>, ProfileError> {
        Ok(self.state.read().await.known_hosts.clone())
    }

    /// Insert or replace the known host for `(host, port)`. Used by the
    /// explicit "replace host key" flow (FR-008) — never silently on connect.
    pub async fn upsert_known_host(&self, record: KnownHostRecord) -> Result<(), ProfileError> {
        let mut state = self.state.write().await;
        match state
            .known_hosts
            .iter()
            .position(|h| h.host == record.host && h.port == record.port)
        {
            Some(i) => state.known_hosts[i] = record,
            None => state.known_hosts.push(record),
        }
        self.persist(&state).await
    }

    pub async fn find_known_host(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Option<KnownHostRecord>, ProfileError> {
        Ok(self.state.read().await.known_hosts.iter().find_map(|h| {
            if h.host == host && h.port == port {
                Some(h.clone())
            } else {
                None
            }
        }))
    }

    /// Delete the known host for `(host, port)` — used when a service pointing
    /// at that endpoint is removed (FR-004).
    pub async fn delete_known_host(&self, host: &str, port: u16) -> Result<(), ProfileError> {
        let mut state = self.state.write().await;
        state
            .known_hosts
            .retain(|h| !(h.host == host && h.port == port));
        self.persist(&state).await
    }

    /// Compute which credential ids are still referenced after a hypothetical
    /// set of removals. Used by the delete flow to drop orphaned Keychain
    /// items only when no remaining profile references them (design §10.3).
    pub async fn orphaned_credentials(
        &self,
        removed: &[ServiceProfile],
    ) -> Result<Vec<String>, ProfileError> {
        let state = self.state.read().await;
        let surviving_refs: std::collections::HashSet<String> = state
            .profiles
            .iter()
            .flat_map(|p| p.credential_references())
            .collect();
        let mut orphans = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for p in removed {
            for id in p.credential_references() {
                if !surviving_refs.contains(&id) && seen.insert(id.clone()) {
                    orphans.push(id);
                }
            }
        }
        Ok(orphans)
    }
}

/// Atomic write: write to a temp sibling file then rename (POSIX atomic).
async fn atomic_write(path: &Path, contents: &str) -> Result<(), ProfileError> {
    let parent = path
        .parent()
        .ok_or_else(|| ProfileError::Storage("path has no parent".into()))?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| ProfileError::Storage(format!("create_dir: {e}")))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("store")
    ));
    fs::write(&tmp, contents)
        .await
        .map_err(|e| ProfileError::Storage(format!("write tmp: {e}")))?;
    fs::rename(&tmp, path)
        .await
        .map_err(|e| ProfileError::Storage(format!("rename: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::model::ProfileInput;
    use tempfile::tempdir;

    fn direct_input(name: &str) -> ProfileInput {
        ProfileInput::DirectUrl {
            name: name.into(),
            base_url: url::Url::parse("https://pi.example.com").unwrap(),
            pi_hub_credential_id: None,
        }
    }

    #[tokio::test]
    async fn create_get_update_delete_roundtrip() {
        let store = ProfileStore::in_memory();
        let created = store.create(direct_input("Cloud")).await.unwrap();
        let id = created.id();
        let fetched = store.get(id).await.unwrap();
        assert_eq!(fetched, created);

        let mut edited = fetched.clone();
        if let ServiceProfile::DirectUrl(d) = &mut edited {
            d.metadata.name = "Cloud2".into();
        }
        let updated = store.update(edited).await.unwrap();
        assert_eq!(updated.metadata().name, "Cloud2");

        let removed = store.delete(id).await.unwrap();
        assert_eq!(removed.id(), id);
        assert!(matches!(store.get(id).await, Err(ProfileError::NotFound)));
    }

    #[tokio::test]
    async fn persists_to_disk_and_reloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");

        {
            let store = ProfileStore::new(&path);
            store.load().await.unwrap();
            store.create(direct_input("Cloud")).await.unwrap();
        }
        // New handle reading the same file must observe the saved profile.
        let store = ProfileStore::new(&path);
        store.load().await.unwrap();
        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].metadata().name, "Cloud");
        assert_eq!(
            store.state.read().await.schema_version,
            CURRENT_SCHEMA_VERSION
        );
    }

    #[tokio::test]
    async fn missing_file_initializes_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let store = ProfileStore::new(&path);
        store.load().await.unwrap();
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn orphaned_credentials_skips_still_referenced() {
        let store = ProfileStore::in_memory();
        let a = store
            .create(ProfileInput::SshForward {
                name: "a".into(),
                ssh_host: "h".into(),
                ssh_port: 22,
                ssh_username: "u".into(),
                ssh_auth_type: crate::profile::model::SshAuthType::Password,
                ssh_credential_id: "shared-cred".into(),
                target_host: "127.0.0.1".into(),
                target_port: 30142,
                service_scheme: crate::profile::model::ServiceScheme::Http,
                service_base_path: "/".into(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();
        let _b = store
            .create(ProfileInput::SshForward {
                name: "b".into(),
                ssh_host: "h".into(),
                ssh_port: 22,
                ssh_username: "u".into(),
                ssh_auth_type: crate::profile::model::SshAuthType::Password,
                ssh_credential_id: "shared-cred".into(),
                target_host: "127.0.0.1".into(),
                target_port: 30142,
                service_scheme: crate::profile::model::ServiceScheme::Http,
                service_base_path: "/".into(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();
        // Removing only `a`: "shared-cred" is still used by `b`.
        let orphans = store.orphaned_credentials(&[a]).await.unwrap();
        assert!(orphans.is_empty());
    }

    #[tokio::test]
    async fn update_preserves_created_at_and_bumps_updated_at() {
        let store = ProfileStore::in_memory();
        let created = store.create(direct_input("Cloud")).await.unwrap();
        let original_created_at = created.metadata().created_at;
        // Wait a tick so updated_at strictly advances.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let mut edited = created.clone();
        if let ServiceProfile::DirectUrl(d) = &mut edited {
            d.metadata.name = "Cloud2".into();
        }
        let updated = store.update(edited).await.unwrap();
        assert_eq!(updated.metadata().created_at, original_created_at);
        assert!(updated.metadata().updated_at > original_created_at);
        assert_eq!(updated.metadata().name, "Cloud2");
    }
}
