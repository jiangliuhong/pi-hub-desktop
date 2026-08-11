//! Service profile persistence (docs/design-v1.md §6.1, §11).
//!
//! Versioned store holding non-sensitive profile data and known hosts only.
//! Sensitive fields never enter the serialized structure (AGENTS.md §6.1, §11).

#![allow(unused_imports)]

pub mod migration;
pub mod model;
pub mod repository;

pub use model::*;
pub use repository::{ProfileStore, StoredState};
