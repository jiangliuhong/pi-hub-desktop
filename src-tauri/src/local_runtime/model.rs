//! V2 local runtime domain models (docs/design-v2.md §4, §5, §7, §8).
//!
//! All of these are safe to serialize across the Tauri boundary: they carry no
//! secrets, no child handles, no full environment dumps and no raw stdout
//! (AGENTS.md §6.1, V2-SR-003/004). `Debug` is derived only for types whose
//! fields are themselves non-secret.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Minimum Node.js version Pi Hub requires (requirements-v2 §8.2 DEP-NODE-001).
/// Kept as a constant rather than parsed from a string at runtime so the
/// baseline can never drift in a panic path.
pub const NODE_REQUIRED_MAJOR: u64 = 22;
pub const NODE_REQUIRED_MINOR: u64 = 19;
pub const NODE_REQUIRED_PATCH: u64 = 0;

/// Default loopback port the managed Pi Hub binds to (requirements-v2 §9
/// V2-FR-006, design-v2 §7).
pub const DEFAULT_LOCAL_PORT: u16 = 30142;

/// Client protocol range supported by this Desktop build (design-v2 §10.4).
pub const SUPPORTED_CLIENT_PROTOCOL_MIN: u32 = 1;
pub const SUPPORTED_CLIENT_PROTOCOL_MAX: u32 = 1;

/// Installation lifecycle states (requirements-v2 §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationState {
    Unknown,
    NotFound,
    Invalid,
    Incompatible,
    Ready,
}

impl InstallationState {
    pub fn api_name(self) -> &'static str {
        match self {
            InstallationState::Unknown => "unknown",
            InstallationState::NotFound => "not_found",
            InstallationState::Invalid => "invalid",
            InstallationState::Incompatible => "incompatible",
            InstallationState::Ready => "ready",
        }
    }

    /// Whether the installation can be used to start a managed Pi Hub.
    pub fn is_ready(self) -> bool {
        matches!(self, InstallationState::Ready)
    }
}

/// Runtime lifecycle states (requirements-v2 §7.2, design-v2 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRuntimeState {
    Unknown,
    Checking,
    Stopped,
    Starting,
    RunningManaged,
    RunningExternal,
    Stopping,
    PortConflict,
    Failed,
}

impl LocalRuntimeState {
    pub fn api_name(self) -> &'static str {
        match self {
            LocalRuntimeState::Unknown => "unknown",
            LocalRuntimeState::Checking => "checking",
            LocalRuntimeState::Stopped => "stopped",
            LocalRuntimeState::Starting => "starting",
            LocalRuntimeState::RunningManaged => "running_managed",
            LocalRuntimeState::RunningExternal => "running_external",
            LocalRuntimeState::Stopping => "stopping",
            LocalRuntimeState::PortConflict => "port_conflict",
            LocalRuntimeState::Failed => "failed",
        }
    }

    /// Whether a service is currently listening and usable.
    pub fn is_running(self) -> bool {
        matches!(
            self,
            LocalRuntimeState::RunningManaged | LocalRuntimeState::RunningExternal
        )
    }

    /// Whether an operation (start/stop/restart/scan) is in flight.
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            LocalRuntimeState::Checking | LocalRuntimeState::Starting | LocalRuntimeState::Stopping
        )
    }

    /// Legal successor states (design-v2 §4.3). Illegal transitions are rejected
    /// by the manager rather than silently applied.
    pub fn legal_successors(self) -> &'static [LocalRuntimeState] {
        use LocalRuntimeState::*;
        match self {
            Unknown => &[Checking],
            Checking => &[
                Stopped,
                RunningManaged,
                RunningExternal,
                PortConflict,
                Failed,
            ],
            Stopped => &[Starting, Checking],
            Starting => &[RunningManaged, Stopped, PortConflict, Failed],
            RunningManaged => &[Stopping, Failed, Checking],
            RunningExternal => &[Checking],
            Stopping => &[Stopped, Failed],
            PortConflict => &[Checking],
            Failed => &[Checking, Starting],
        }
    }

    /// Validate a transition; returns the next state on success.
    pub fn transition(
        self,
        next: LocalRuntimeState,
    ) -> Result<LocalRuntimeState, super::LocalRuntimeStateError> {
        if self.legal_successors().contains(&next) {
            Ok(next)
        } else {
            Err(super::LocalRuntimeStateError {
                from: self,
                to: next,
            })
        }
    }
}

/// Pi environment overall status (requirements-v2 §7.3, design-v2 §8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    Ready,
    Degraded,
    Blocked,
    Unknown,
}

impl EnvironmentStatus {
    pub fn api_name(self) -> &'static str {
        match self {
            EnvironmentStatus::Ready => "ready",
            EnvironmentStatus::Degraded => "degraded",
            EnvironmentStatus::Blocked => "blocked",
            EnvironmentStatus::Unknown => "unknown",
        }
    }
}

/// Where an installation was discovered (design-v2 §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationSource {
    Persisted,
    Path,
    Homebrew,
    Nvm,
    Volta,
    Fnm,
    Asdf,
    Mise,
    Manual,
    /// V3: a Desktop-managed copy under the app's Application Support dir
    /// (docs/requirements-v3.md §3.2). Reused by the package-management
    /// detector to mark owned installations.
    DesktopManaged,
}

impl InstallationSource {
    pub fn api_name(self) -> &'static str {
        match self {
            InstallationSource::Persisted => "persisted",
            InstallationSource::Path => "path",
            InstallationSource::Homebrew => "homebrew",
            InstallationSource::Nvm => "nvm",
            InstallationSource::Volta => "volta",
            InstallationSource::Fnm => "fnm",
            InstallationSource::Asdf => "asdf",
            InstallationSource::Mise => "mise",
            InstallationSource::Manual => "manual",
            InstallationSource::DesktopManaged => "desktop_managed",
        }
    }
}

/// Verified Node.js installation (design-v2 §5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInstallation {
    pub executable: PathBuf,
    pub canonical_executable: PathBuf,
    pub version: String,
    pub source: InstallationSource,
}

/// Verified Pi Hub installation (design-v2 §5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiHubInstallation {
    pub package_root: PathBuf,
    pub entrypoint: PathBuf,
    pub version: String,
    pub node_requirement: String,
    pub source: InstallationSource,
}

/// Optional external Pi CLI (design-v2 §5.3). Informational only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiCliInstallation {
    pub executable: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub kind: PiCliKind,
    pub source: InstallationSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiCliKind {
    Npm,
    Standalone,
    Unknown,
}

/// The full set of verified installations (design-v2 §6).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallationSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeInstallation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pi_hub: Option<PiHubInstallation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pi_cli: Option<PiCliInstallation>,
}

/// A single environment check result (requirements-v2 §8.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub id: String,
    pub category: CheckCategory,
    pub severity: CheckSeverity,
    pub status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    /// Allowlisted, non-sensitive details only (design-v2 §8.4).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    Runtime,
    PiHub,
    PiEnvironment,
    AuthAndModels,
    OptionalTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckSeverity {
    Required,
    Recommended,
    Informational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skipped,
}

impl CheckStatus {
    /// Whether this status counts as a failure for aggregation purposes.
    pub fn is_failure(self) -> bool {
        matches!(self, CheckStatus::Fail)
    }
}

/// Aggregated environment summary returned to the UI (design-v2 §8.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentReport {
    pub overall: EnvironmentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<DateTime<Utc>>,
    pub checks: Vec<CheckResult>,
}

impl Default for EnvironmentReport {
    fn default() -> Self {
        EnvironmentReport {
            overall: EnvironmentStatus::Unknown,
            generated_at: None,
            checks: Vec::new(),
        }
    }
}

/// Summary of a managed process surfaced to the UI (design-v2 §4.2). Contains
/// no child handle and no secrets — just observable facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedProcessSummary {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_at: Option<DateTime<Utc>>,
    pub node_executable: PathBuf,
    pub pi_hub_entrypoint: PathBuf,
    pub port: u16,
}

/// The full non-sensitive snapshot published to the UI (design-v2 §4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeSnapshot {
    pub installation_state: InstallationState,
    pub runtime_state: LocalRuntimeState,
    pub environment: EnvironmentReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation: Option<InstallationSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_process: Option<ManagedProcessSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<crate::error::ErrorDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<DateTime<Utc>>,
}

impl Default for LocalRuntimeSnapshot {
    fn default() -> Self {
        LocalRuntimeSnapshot {
            installation_state: InstallationState::Unknown,
            runtime_state: LocalRuntimeState::Unknown,
            environment: EnvironmentReport::default(),
            installation: None,
            managed_process: None,
            effective_url: None,
            last_error: None,
            checked_at: None,
        }
    }
}

/// A redacted log line (design-v2 §15.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub stream: LogStream,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Summary of how a managed process exited.
#[derive(Debug, Clone, Copy)]
pub struct ExitSummary {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}
