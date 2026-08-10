//! Connection layer (docs/design-v1.md §7, §12).
//!
//! Owns the connection state machine and resource lifecycle as the single
//! source of truth (AGENTS.md §5.3). Intended internal modules:
//! - `manager.rs`    — `ConnectionManager`, per-service dedup, cancel handles
//! - `provider.rs`   — `ConnectionProvider` trait
//! - `direct.rs`     — `DirectUrlProvider`
//! - `ssh_forward.rs`— `SshForwardProvider`
//! - `state.rs`      — `ConnectionState` enum + legal transitions
//! - `diagnostics.rs`— non-sensitive diagnostic snapshot
