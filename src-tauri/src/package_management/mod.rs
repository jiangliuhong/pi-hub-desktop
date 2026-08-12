//! V3 package management domain — Pi & Pi Hub detection, release checks,
//! managed install/update (docs/requirements-v3.md,
//! docs/pi-and-pi-hub-package-management-design.md).
//!
//! This module is a **separate domain** from V2 `LocalRuntimeManager`
//! (AGENTS.md §5.6): it owns detection facts, version metadata, the managed
//! copy store and install/update transactions. It never duplicates Pi Hub
//! process management — Pi Hub activation delegates to `LocalRuntimeManager`
//! via a minimal adapter (design §19.1).
//!
//! Safety invariants (regardless of phase):
//! - No `sh -c` / `zsh -c` / shell string building; only verified absolute
//!   Node/npm CLI with Rust-constructed args (V3-SR-001, AGENTS.md §6.6).
//! - The only process this module may stop is an npm child whose handle lives
//!   in this app's memory — never PID/port/name guessing (V3-SR-002).
//! - Secrets never enter models, logs, events or store (V3-SR-005).
//! - The Service WebView keeps zero capability (V3-SR-006).
//! - Writes are confined to the canonicalized managed root (V3-SR-003).
//!
//! Platform gating: the domain is implemented platform-agnostically and runs
//! on macOS in production (and Linux for unit/integration tests). On iOS
//! (`cfg(mobile)`) the manager is constructed with no services so every
//! operation returns `unsupported_platform` (requirements-v3 §4.2).

pub mod installer;
pub mod managed_store;
pub mod manager;
pub mod model;
pub mod npm_toolchain;
pub mod operation;
pub mod release_client;
pub mod verifier;

pub use manager::PackageManagementManager;
pub use model::*;

use std::fmt;

/// Illegal package-management state/transition (internal). Surfaced as an
/// internal error; never silently coerces state.
#[derive(Debug, thiserror::Error)]
#[error("illegal package management transition: {from:?} -> {to:?}")]
pub struct PackageStateError {
    pub from: String,
    pub to: String,
}

#[allow(dead_code)]
impl PackageStateError {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        PackageStateError {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// Whether the current build supports package management.
///
/// `true` on macOS desktop (and Linux dev/test); `false` on iOS where no
/// package management is offered (requirements-v3 §4.2).
#[cfg(not(mobile))]
pub const fn platform_supported() -> bool {
    true
}

#[cfg(mobile)]
pub const fn platform_supported() -> bool {
    false
}

impl fmt::Display for ProductId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.api_name())
    }
}

impl fmt::Display for ProductInstallState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.api_name())
    }
}

impl fmt::Display for UpdateStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.api_name())
    }
}
