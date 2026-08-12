//! Non-sensitive connection diagnostics (docs/requirements-v1.md FR-016,
//! docs/design-v1.md §20). Must never contain secrets, Authorization, cookies,
//! private-key material or business data (SR-005).

use serde::{Deserialize, Serialize};

/// Diagnostic snapshot surfaced on the failure page. Mirrors the frontend
/// `ConnectionDiagnostics` type exactly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    pub listener_started: bool,
    pub retry_count: u32,
    // --- Reliability diagnostics (plan-remote-pi-hub-performance §4.1) ---
    /// Monotonic operation generation. Changes when a connect/disconnect/
    /// reconnect supersedes in-flight work; stale async results are discarded
    /// by comparing against the current generation (plan §5.5.7).
    #[serde(default)]
    pub generation: u64,
    /// Number of reconnect attempts since the last successful connect.
    /// Distinct from `retry_count` (the per-attempt host-key-retry counter).
    #[serde(default)]
    pub reconnect_count: u32,
    /// Non-sensitive classification of why the last session ended, if any
    /// (one of `SessionCloseReason::as_str`). Never a raw error string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_close_reason: Option<String>,
}

impl ConnectionDiagnostics {
    pub fn new() -> Self {
        ConnectionDiagnostics::default()
    }

    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    pub fn set_error(&mut self, code: impl Into<String>) {
        self.error_code = Some(code.into());
    }
}
