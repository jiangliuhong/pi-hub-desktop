//! Service WebView lifecycle (docs/design-v1.md §14).
//!
//! Manages the isolated, untrusted Pi Hub Service View. It must have zero
//! Tauri capability and cannot read Keychain, Store or the filesystem
//! (AGENTS.md §6.4). Intended internal modules:
//! - `manager.rs`   — open/close, per-service WebView data store isolation
//! - `navigation.rs`— allowlist + external-link-to-system-browser policy
//! - `auth.rs`      — HTTP Basic challenge handled by the trusted bridge only
//!
//! Final approach (native window vs. Darwin WKWebView plugin) is decided by
//! the Phase 0 Service View spike. This module is a stub until then.
