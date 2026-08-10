//! Tauri command adapters (docs/design-v1.md §5, §13.1).
//!
//! Commands are thin: input validation → call domain service → map error →
//! return serializable DTO. They must not accumulate SSH, storage or lifecycle
//! logic (AGENTS.md §5.2).
//!
//! Intended command files (added per phase):
//! - `profiles.rs` — service profile CRUD (`list/get/create/update/delete`)
//! - `credentials.rs` — `put_credential`, `delete_credential`
//! - `connections.rs` — connect, disconnect, status, host-key flows
//! - `viewer.rs` — `open_service_view`, `close_service_view`
