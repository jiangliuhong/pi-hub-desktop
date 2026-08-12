//! Connection layer (docs/design-v1.md §7, §12).
//!
//! Owns the connection state machine and resource lifecycle as the single
//! source of truth (AGENTS.md §5.3).

pub mod broadcaster;
pub mod diagnostics;
pub mod direct;
pub mod manager;
pub mod provider;
pub mod ssh_forward;
pub mod state;

pub use manager::*;
pub use provider::*;
pub use state::*;
