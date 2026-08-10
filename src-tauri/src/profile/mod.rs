//! Service profile persistence (docs/design-v1.md §6.1, §11).
//!
//! Versioned store holding non-sensitive profile data and known hosts only.
//! Intended internal modules:
//! - `model.rs`      — `ServiceProfile` tagged enum + metadata
//! - `repository.rs` — atomic read/validate/write
//! - `migration.rs`  — forward schema migrations
//!
//! Sensitive fields never enter the serialized structure (AGENTS.md §6.1, §11).
//! This module is a stub until V1 Phase 1.
