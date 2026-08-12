//! Tauri command adapters (docs/design-v1.md §5, §13.1).
//!
//! Commands are thin: input validation → call domain service → map error →
//! return serializable DTO (AGENTS.md §5.2). They never accumulate SSH, storage
//! or lifecycle logic — that lives in the domain modules.

pub mod connections;
pub mod credentials;
pub mod local_runtime;
pub mod package_management;
pub mod profiles;
pub mod viewer;
