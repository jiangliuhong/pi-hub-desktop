//! Pi Hub Client — Rust core.
//!
//! V1 initialization stage. This crate currently holds only the Tauri app
//! skeleton and the module boundaries from docs/design-v1.md §5. Actual
//! connection, credential, profile, SSH and Service View behavior lands in
//! V1 Phase 1 / Phase 2; modules here are intentionally stubs and must not be
//! treated as implemented.
//!
//! Security invariants are enforced regardless of phase:
//! - Tauri capability is bound to the trusted `main` window only; the remote
//!   Service WebView receives zero capability (AGENTS.md §6.4).
//! - Sensitive values never enter profile serialization, logs or event
//!   payloads (AGENTS.md §6.1).
//! - The Rust side is the single source of truth for connection lifecycle
//!   (AGENTS.md §5.3).

// Module boundaries (docs/design-v1.md §5). Each module is a stub until its
// phase adds real behavior plus tests.
mod commands;
mod connection;
mod credential;
mod error;
mod event;
mod platform;
mod profile;
mod ssh;
mod viewer;

/// Product display name shared with the frontend / about surface.
pub const APP_NAME: &str = "Pi Hub Client";

/// Bundle identifier segment used for Keychain `service` (design §6.2).
pub const APP_BUNDLE_ID: &str = "top.jiangliuhong.pihubclient";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // The opener plugin is available to the trusted App Shell only for
        // handing external links to the system browser (design §14.5).
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running Pi Hub Client");
}

#[cfg(test)]
mod tests {
    use super::{APP_BUNDLE_ID, APP_NAME};

    #[test]
    fn app_identity_is_non_empty() {
        assert!(!APP_NAME.is_empty());
        assert!(!APP_BUNDLE_ID.is_empty());
    }

    #[test]
    fn app_bundle_id_matches_identifier() {
        // Keep the Rust constant in sync with the Tauri identifier in
        // `tauri.conf.json`, which is used for Keychain namespacing.
        assert_eq!(APP_BUNDLE_ID, "top.jiangliuhong.pihubclient");
    }
}
