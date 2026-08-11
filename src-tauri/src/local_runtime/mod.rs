//! V2 local runtime domain — macOS-local Pi Hub management
//! (docs/requirements-v2.md, docs/design-v2.md).
//!
//! This module is the **single source of truth** for local installation
//! detection, environment diagnostics, the managed process lifecycle and the
//! observable runtime state (AGENTS.md §5.2, §5.4). It never lives inside the
//! V1 `ConnectionManager`; both domains may share the Viewer but keep
//! independent state models (design-v2 §3).
//!
//! Safety invariants (regardless of phase):
//! - No `sh -c` / `zsh -c` / shell string building (V2-SR-001).
//! - Only the process whose `Child` handle lives in this app's memory may be
//!   stopped — never PID/port/name guessing (V2-SR-002, design-v2 §11.8).
//! - Secrets never enter models, logs, events or Doctor output (V2-SR-003/004).
//! - The Service WebView keeps zero capability (V2-SR-005).
//!
//! Platform gating: the domain services are implemented platform-agnostically
//! (they work on Linux for unit/integration tests and on macOS in production).
//! On iOS (`cfg(mobile)`), `build_default` constructs a manager with no
//! services so every operation returns `unsupported_platform` (design-v2
//! §16.1, requirements-v2 §4.2).

pub mod detector;
pub mod doctor;
pub mod health;
pub mod logs;
pub mod manager;
pub mod model;
pub mod process;
pub mod redaction;
pub mod settings;

pub use manager::LocalRuntimeManager;
pub use model::*;

use std::fmt;

/// Illegal runtime state transition (design-v2 §4.3). Surfaced as an internal
/// error; never silently coerces state.
#[derive(Debug, thiserror::Error)]
#[error("illegal local runtime state transition: {from:?} -> {to:?}")]
pub struct LocalRuntimeStateError {
    pub from: LocalRuntimeState,
    pub to: LocalRuntimeState,
}

impl fmt::Display for LocalRuntimeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.api_name())
    }
}

/// Whether the current build supports managing a local Pi Hub process.
///
/// `true` on macOS desktop (and Linux dev/test where the same POSIX code path
/// runs); `false` on iOS where no local process management is offered
/// (requirements-v2 §4.2, design-v2 §16.1).
#[cfg(not(mobile))]
pub const fn platform_supported() -> bool {
    true
}

#[cfg(mobile)]
pub const fn platform_supported() -> bool {
    false
}
