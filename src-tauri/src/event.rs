//! Structured event payloads (docs/design-v1.md §13.2).
//!
//! Events pushed to the Trusted App Shell:
//! - `connection://state-changed`
//! - `connection://diagnostics-updated`
//! - `ssh://host-key-challenge`
//! - `viewer://closed`
//! - `app://foregrounded`
//! - `app://backgrounded`
//!
//! Event payloads must never contain credentials, Authorization, cookies or
//! page content (AGENTS.md §6.1, §6.4).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Non-sensitive payload for `connection://state-changed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChangedPayload {
    pub service_id: Uuid,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_url: Option<String>,
}

/// Non-sensitive payload for `connection://diagnostics-updated`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsPayload {
    pub service_id: Uuid,
    #[serde(flatten)]
    pub diagnostics: crate::connection::diagnostics::ConnectionDiagnostics,
}

/// Non-sensitive payload for `ssh://host-key-challenge` (FR-007). Carries only
/// algorithm + fingerprint + endpoint, never a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostKeyChallengePayload {
    pub challenge_id: Uuid,
    pub service_id: Uuid,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub algorithm: String,
    pub sha256_fingerprint: String,
}

/// Payload for `viewer://closed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerClosedPayload {
    pub service_id: Uuid,
}
