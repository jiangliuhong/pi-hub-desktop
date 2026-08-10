//! Typed application errors (docs/design-v1.md §19).
//!
//! Recoverable errors use a typed enum; at the Tauri boundary they are
//! converted to a stable, allowlist-serialized error-code DTO. `unwrap()`,
//! `expect()` and undocumented panics are forbidden in business paths
//! (AGENTS.md §9).
//!
//! Stable error codes (AGENTS.md §9): `invalid_profile`, `dns_failed`,
//! `ssh_connect_timeout`, `host_key_unknown`, `host_key_changed`,
//! `authentication_failed`, `private_key_invalid`,
//! `private_key_passphrase_required`, `target_unreachable`,
//! `local_listener_failed`, `service_http_error`, `tls_error`, `cancelled`,
//! `unsupported_platform`.
//!
//! This module is a stub until V1 Phase 1; the enum and DTO are added together
//! with the first command that can fail.
