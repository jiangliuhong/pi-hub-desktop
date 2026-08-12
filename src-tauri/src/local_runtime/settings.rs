//! Versioned, atomic local-runtime settings store (docs/design-v2.md §7,
//! AGENTS.md §12).
//!
//! Stored in its own file namespace (`local-runtime.json`) and migrated
//! independently of V1 profiles (requirements-v2 §9 V2-FR-015, NFR-004).
//! Only non-sensitive data lives here; the Pi Hub password is referenced by
//! `credential_id` and resolved from Keychain at start time (V2-SR-003).

use crate::error::LocalRuntimeError;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::RwLock;

/// Current settings schema version. Bump + add a migration on every structural
/// change (AGENTS.md §12).
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Default loopback port (requirements-v2 §9 V2-FR-015).
const DEFAULT_PORT: u16 = super::model::DEFAULT_LOCAL_PORT;

/// The V2 local runtime settings document (design-v2 §7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeSettings {
    pub schema_version: u32,
    pub port: u16,
    pub auto_start_on_app_launch: bool,
    pub stop_managed_on_app_exit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_executable: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_hub_entrypoint: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_hub_package_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_agent_dir: Option<PathBuf>,
    /// Reference only — the secret itself lives in Keychain (V2-SR-003).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_hub_credential_id: Option<String>,
    /// Most recent crash-loop protection state (design-v2 §14.2). Persisted so
    /// an app restart remembers the suppression window. Holds only timestamps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_start_failures: Vec<chrono::DateTime<Utc>>,
}

impl Default for LocalRuntimeSettings {
    fn default() -> Self {
        LocalRuntimeSettings {
            schema_version: CURRENT_SCHEMA_VERSION,
            port: DEFAULT_PORT,
            // Off by default (requirements-v2 §9 V2-FR-012).
            auto_start_on_app_launch: false,
            // On by default (requirements-v2 §9 V2-FR-013).
            stop_managed_on_app_exit: true,
            node_executable: None,
            pi_hub_entrypoint: None,
            pi_hub_package_root: None,
            pi_agent_dir: None,
            pi_hub_credential_id: None,
            auto_start_failures: Vec::new(),
        }
    }
}

/// DTO for partial updates from the frontend. Only allowlisted fields are
/// accepted — never arbitrary env vars or commands (V2-SR-001).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LocalRuntimeSettingsUpdate {
    pub port: Option<u16>,
    pub auto_start_on_app_launch: Option<bool>,
    pub stop_managed_on_app_exit: Option<bool>,
    pub node_executable: Option<PathBuf>,
    pub pi_hub_entrypoint: Option<PathBuf>,
    pub pi_hub_package_root: Option<PathBuf>,
    pub pi_agent_dir: Option<PathBuf>,
    pub pi_hub_credential_id: Option<String>,
}

/// Atomic, versioned store for local runtime settings.
pub struct LocalRuntimeSettingsStore {
    path: Option<PathBuf>,
    state: RwLock<LocalRuntimeSettings>,
}

impl LocalRuntimeSettingsStore {
    /// Create a store backed by a file on disk.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        LocalRuntimeSettingsStore {
            path: Some(path.into()),
            state: RwLock::new(LocalRuntimeSettings::default()),
        }
    }

    /// In-memory store for tests.
    pub fn in_memory() -> Self {
        LocalRuntimeSettingsStore {
            path: None,
            state: RwLock::new(LocalRuntimeSettings::default()),
        }
    }

    /// Load (or initialize defaults) from disk atomically.
    pub async fn load(&self) -> Result<(), LocalRuntimeError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let loaded = match fs::read_to_string(path).await {
            Ok(s) if !s.trim().is_empty() => parse_and_migrate(&s)?,
            Ok(_) => LocalRuntimeSettings::default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalRuntimeSettings::default(),
            Err(e) => {
                return Err(LocalRuntimeError::Internal(format!("read settings: {e}")));
            }
        };
        *self.state.write().await = loaded;
        Ok(())
    }

    async fn persist(&self, state: &LocalRuntimeSettings) -> Result<(), LocalRuntimeError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| LocalRuntimeError::Internal(format!("serialize settings: {e}")))?;
        atomic_write(path, &json).await
    }

    /// Snapshot the current settings.
    pub async fn get(&self) -> LocalRuntimeSettings {
        self.state.read().await.clone()
    }

    /// Replace the entire settings document (validated first).
    pub async fn replace(
        &self,
        settings: LocalRuntimeSettings,
    ) -> Result<LocalRuntimeSettings, LocalRuntimeError> {
        validate(&settings)?;
        let mut state = self.state.write().await;
        *state = settings;
        let snapshot = state.clone();
        drop(state);
        self.persist(&snapshot).await?;
        Ok(snapshot)
    }

    /// Apply a partial update on top of the current settings (returns the new
    /// snapshot). Path fields are canonicalized before persistence so stored
    /// paths are stable (design-v2 §7, §6.4).
    pub async fn update(
        &self,
        update: LocalRuntimeSettingsUpdate,
    ) -> Result<LocalRuntimeSettings, LocalRuntimeError> {
        let mut state = self.state.write().await;
        if let Some(port) = update.port {
            state.port = port;
        }
        if let Some(v) = update.auto_start_on_app_launch {
            state.auto_start_on_app_launch = v;
        }
        if let Some(v) = update.stop_managed_on_app_exit {
            state.stop_managed_on_app_exit = v;
        }
        if let Some(p) = update.node_executable {
            state.node_executable = canonicalize_optional(p);
        }
        if let Some(p) = update.pi_hub_entrypoint {
            state.pi_hub_entrypoint = canonicalize_optional(p);
        }
        if let Some(p) = update.pi_hub_package_root {
            state.pi_hub_package_root = canonicalize_optional(p);
        }
        if let Some(p) = update.pi_agent_dir {
            state.pi_agent_dir = canonicalize_optional(p);
        }
        if let Some(v) = update.pi_hub_credential_id {
            // Empty string means "clear"; normalize to None.
            state.pi_hub_credential_id = if v.trim().is_empty() { None } else { Some(v) };
        }
        validate(&state)?;
        let snapshot = state.clone();
        drop(state);
        self.persist(&snapshot).await?;
        Ok(snapshot)
    }

    /// Record an auto-start failure timestamp for crash-loop protection and
    /// persist it (design-v2 §14.2). Holds only timestamps — no logs.
    pub async fn record_auto_start_failure(
        &self,
        at: chrono::DateTime<Utc>,
    ) -> Result<(), LocalRuntimeError> {
        let mut state = self.state.write().await;
        state.auto_start_failures.push(at);
        // Keep only failures inside the suppression window (5 min).
        let cutoff = at
            - chrono::Duration::from_std(std::time::Duration::from_secs(
                super::manager::CRASH_LOOP_WINDOW_SECS,
            ))
            .expect("valid duration");
        state.auto_start_failures.retain(|t| *t >= cutoff);
        let snapshot = state.clone();
        drop(state);
        self.persist(&snapshot).await
    }

    /// Snapshot the recorded auto-start failures inside the suppression window.
    pub async fn auto_start_failures(&self) -> Vec<chrono::DateTime<Utc>> {
        self.state.read().await.auto_start_failures.clone()
    }

    /// Clear crash-loop history (e.g. after a manual start or setting change).
    pub async fn clear_auto_start_failures(&self) -> Result<(), LocalRuntimeError> {
        let mut state = self.state.write().await;
        if state.auto_start_failures.is_empty() {
            return Ok(());
        }
        state.auto_start_failures.clear();
        let snapshot = state.clone();
        drop(state);
        self.persist(&snapshot).await
    }
}

/// Validate invariant constraints on settings (port range, etc.).
fn validate(s: &LocalRuntimeSettings) -> Result<(), LocalRuntimeError> {
    if s.port == 0 {
        return Err(LocalRuntimeError::Internal("port must be > 0".into()));
    }
    Ok(())
}

/// Canonicalize a path if it exists; otherwise keep the raw path verbatim so a
/// user can save a not-yet-present target and have it re-validated later
/// (design-v2 §6.4: paths are re-validated before use, not blindly trusted).
fn canonicalize_optional(p: PathBuf) -> Option<PathBuf> {
    if p.as_os_str().is_empty() {
        None
    } else {
        Some(canonicalize_or_keep(&p))
    }
}

fn canonicalize_or_keep(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Parse raw JSON and apply forward migrations to the current schema.
fn parse_and_migrate(json: &str) -> Result<LocalRuntimeSettings, LocalRuntimeError> {
    let value = serde_json::from_str::<serde_json::Value>(json)
        .map_err(|e| LocalRuntimeError::Internal(format!("invalid settings json: {e}")))?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    if version > CURRENT_SCHEMA_VERSION {
        return Err(LocalRuntimeError::Internal(format!(
            "settings schema_version {version} is newer than supported {CURRENT_SCHEMA_VERSION}"
        )));
    }
    // No real migrations yet (v1 baseline). Future bumps append explicit steps.
    let mut settings: LocalRuntimeSettings = serde_json::from_value(value)
        .map_err(|e| LocalRuntimeError::Internal(format!("settings schema mismatch: {e}")))?;
    settings.schema_version = CURRENT_SCHEMA_VERSION;
    // Older UI versions serialized cleared optional path inputs as "". Treat
    // those as absent so Doctor and the detector use their documented default
    // paths instead of interpreting an empty PathBuf as an explicit override.
    settings.node_executable = settings.node_executable.and_then(canonicalize_optional);
    settings.pi_hub_entrypoint = settings.pi_hub_entrypoint.and_then(canonicalize_optional);
    settings.pi_hub_package_root = settings.pi_hub_package_root.and_then(canonicalize_optional);
    settings.pi_agent_dir = settings.pi_agent_dir.and_then(canonicalize_optional);
    validate(&settings)?;
    Ok(settings)
}

/// Atomic write: temp sibling file + fsync-style write + rename (POSIX atomic).
async fn atomic_write(path: &Path, contents: &str) -> Result<(), LocalRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| LocalRuntimeError::Internal("settings path has no parent".into()))?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| LocalRuntimeError::Internal(format!("create_dir: {e}")))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("settings")
    ));
    fs::write(&tmp, contents)
        .await
        .map_err(|e| LocalRuntimeError::Internal(format!("write tmp: {e}")))?;
    fs::rename(&tmp, path)
        .await
        .map_err(|e| LocalRuntimeError::Internal(format!("rename: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn defaults_match_design() {
        let store = LocalRuntimeSettingsStore::in_memory();
        let s = store.get().await;
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(s.port, DEFAULT_PORT);
        assert!(!s.auto_start_on_app_launch);
        assert!(s.stop_managed_on_app_exit);
        assert!(s.node_executable.is_none());
    }

    #[tokio::test]
    async fn update_applies_partial_fields() {
        let store = LocalRuntimeSettingsStore::in_memory();
        let s = store
            .update(LocalRuntimeSettingsUpdate {
                port: Some(30200),
                auto_start_on_app_launch: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(s.port, 30200);
        assert!(s.auto_start_on_app_launch);
        // Untouched default preserved.
        assert!(s.stop_managed_on_app_exit);
    }

    #[tokio::test]
    async fn rejects_port_zero() {
        let store = LocalRuntimeSettingsStore::in_memory();
        let res = store
            .update(LocalRuntimeSettingsUpdate {
                port: Some(0),
                ..Default::default()
            })
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn persists_and_reloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local-runtime.json");
        {
            let store = LocalRuntimeSettingsStore::new(&path);
            store.load().await.unwrap();
            store
                .update(LocalRuntimeSettingsUpdate {
                    port: Some(31000),
                    ..Default::default()
                })
                .await
                .unwrap();
        }
        let store = LocalRuntimeSettingsStore::new(&path);
        store.load().await.unwrap();
        assert_eq!(store.get().await.port, 31000);
    }

    #[tokio::test]
    async fn rejects_newer_schema_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local-runtime.json");
        fs::write(&path, r#"{"schema_version": 999}"#)
            .await
            .unwrap();
        let store = LocalRuntimeSettingsStore::new(&path);
        assert!(store.load().await.is_err());
    }

    #[tokio::test]
    async fn credential_id_empty_string_clears() {
        let store = LocalRuntimeSettingsStore::in_memory();
        store
            .update(LocalRuntimeSettingsUpdate {
                pi_hub_credential_id: Some("abc".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            store.get().await.pi_hub_credential_id.as_deref(),
            Some("abc")
        );
        store
            .update(LocalRuntimeSettingsUpdate {
                pi_hub_credential_id: Some("".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(store.get().await.pi_hub_credential_id.is_none());
    }

    #[tokio::test]
    async fn empty_optional_path_update_clears_override() {
        let store = LocalRuntimeSettingsStore::in_memory();
        store
            .update(LocalRuntimeSettingsUpdate {
                pi_agent_dir: Some(PathBuf::from("/tmp/pi-agent")),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(store.get().await.pi_agent_dir.is_some());

        store
            .update(LocalRuntimeSettingsUpdate {
                pi_agent_dir: Some(PathBuf::new()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(store.get().await.pi_agent_dir.is_none());
    }

    #[tokio::test]
    async fn load_normalizes_legacy_empty_optional_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local-runtime.json");
        fs::write(
            &path,
            r#"{
              "schema_version": 1,
              "port": 30142,
              "auto_start_on_app_launch": false,
              "stop_managed_on_app_exit": true,
              "node_executable": "",
              "pi_hub_entrypoint": "",
              "pi_hub_package_root": "",
              "pi_agent_dir": ""
            }"#,
        )
        .await
        .unwrap();

        let store = LocalRuntimeSettingsStore::new(&path);
        store.load().await.unwrap();
        let loaded = store.get().await;
        assert!(loaded.node_executable.is_none());
        assert!(loaded.pi_hub_entrypoint.is_none());
        assert!(loaded.pi_hub_package_root.is_none());
        assert!(loaded.pi_agent_dir.is_none());
    }

    #[tokio::test]
    async fn crash_loop_failures_pruned_to_window() {
        let store = LocalRuntimeSettingsStore::in_memory();
        let now = Utc::now();
        // An old failure outside the window is retained on insert only if
        // within window; older ones are dropped.
        store
            .record_auto_start_failure(now - chrono::Duration::minutes(10))
            .await
            .unwrap();
        store.record_auto_start_failure(now).await.unwrap();
        let failures = store.auto_start_failures().await;
        // The 10-min-old failure is outside the 5-min window and pruned.
        assert!(failures
            .iter()
            .all(|t| *t > now - chrono::Duration::minutes(6)));
    }
}
