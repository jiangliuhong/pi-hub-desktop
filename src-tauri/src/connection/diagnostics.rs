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
