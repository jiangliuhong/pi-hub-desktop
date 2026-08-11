//! Pi Hub Client — Rust core.
//!
//! V1 implementation of the connection + profile + credential + SSH forward
//! domain layer, exposed to the Trusted App Shell through thin Tauri commands
//! (docs/design-v1.md §5, AGENTS.md §5.2).
//!
//! Security invariants (regardless of phase):
//! - Tauri capability is bound to the trusted `main` window only; the remote
//!   Service WebView receives zero capability (AGENTS.md §6.4).
//! - Sensitive values never enter profile serialization, logs or event
//!   payloads (AGENTS.md §6.1, SR-005).
//! - The Rust side is the single source of truth for connection lifecycle
//!   (AGENTS.md §5.3).
//! - SSH host keys are strictly verified; unknown/changed keys always block
//!   (AGENTS.md §6.2).

pub mod commands;
pub mod connection;
pub mod credential;
pub mod error;
pub mod event;
pub mod local_runtime;
pub mod platform;
pub mod profile;
pub mod ssh;
pub mod viewer;

use crate::connection::manager::ConnectionManager;
use crate::credential::{default_store, CredentialStore};
use crate::local_runtime::manager::{LocalRuntimeManager, TauriBroadcaster};
use crate::local_runtime::settings::LocalRuntimeSettingsStore;
use crate::profile::repository::ProfileStore;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

/// Product display name shared with the frontend / about surface.
pub const APP_NAME: &str = "Pi Hub Client";

/// Bundle identifier segment used for Keychain `service` (design §6.2).
pub const APP_BUNDLE_ID: &str = "top.jiangliuhong.pihubclient";

/// Locate the on-disk profile store for this app. Resolves an OS-appropriate
/// config directory; falls back to a file next to the executable if no
/// platform directory is available (non-Apple dev/test only).
pub fn resolve_store_path() -> PathBuf {
    if let Some(dir) = dirs_config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        return dir.join(platform::default_store_filename());
    }
    PathBuf::from(platform::default_store_filename())
}

#[cfg(target_vendor = "apple")]
fn dirs_config_dir() -> Option<PathBuf> {
    // macOS: ~/Library/Application Support/<bundle>; iOS: app container.
    // `dirs` is not a V1 dependency; use the security-framework-adjacent path
    // via the standard `HOME`-based layout on macOS. iOS path is provided by
    // the app container via Tauri at runtime; this helper is a best-effort.
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library")
            .join("Application Support")
            .join(APP_BUNDLE_ID)
    })
}

#[cfg(not(target_vendor = "apple"))]
fn dirs_config_dir() -> Option<PathBuf> {
    // Linux/dev fallback: XDG config home if present.
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join(APP_BUNDLE_ID))
}

/// Locate the on-disk local-runtime settings file (design-v2 §7).
pub fn resolve_local_runtime_store_path() -> PathBuf {
    if let Some(dir) = dirs_config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("local-runtime.json");
    }
    PathBuf::from("local-runtime.json")
}

/// Build the managed state graph (profile store + credential store +
/// connection manager) and load the store. Returned so tests / alternate
/// entry points can assemble the same graph without the Tauri builder.
pub async fn build_state(
    store_path: PathBuf,
    credential_store: Option<Arc<dyn CredentialStore>>,
) -> anyhow_result::Result<State, crate::error::ProfileError> {
    let profile_store = Arc::new(ProfileStore::new(store_path));
    profile_store.load().await?;
    let credentials: Arc<dyn CredentialStore> =
        credential_store.unwrap_or_else(|| Arc::from(default_store()));
    let manager = Arc::new(ConnectionManager::new(
        profile_store.clone(),
        credentials.clone(),
    ));
    Ok(State {
        profile_store,
        credentials,
        manager,
    })
}

/// Owned handles to the managed state graph, shared with Tauri.
pub struct State {
    pub profile_store: Arc<ProfileStore>,
    pub credentials: Arc<dyn CredentialStore>,
    pub manager: Arc<ConnectionManager>,
}

/// Minimal `Result` alias module so we don't pull in `anyhow` (not a V1 dep).
mod anyhow_result {
    pub type Result<T, E> = core::result::Result<T, E>;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store_path = resolve_store_path();
    // Build the state graph synchronously by running the async builder on a
    // short-lived runtime. Loading the store is fast (one small JSON file).
    let state = {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                panic!("failed to start runtime to load profile store: {e}");
            }
        };
        rt.block_on(async { build_state(store_path, None).await })
    }
    .expect("failed to load profile store");

    let credential_store = state.credentials.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state.profile_store)
        .manage(state.credentials)
        .manage(state.manager)
        .setup(move |app| {
            // Build the V2 local runtime manager with a Tauri-backed
            // broadcaster (design-v2 §16, §17.2). Construction in `setup`
            // gives us the AppHandle needed for event emission.
            let handle = app.handle().clone();
            let local_settings = std::sync::Arc::new(LocalRuntimeSettingsStore::new(
                resolve_local_runtime_store_path(),
            ));
            // Load settings best-effort (defaults are usable on failure).
            let local_settings_load = local_settings.clone();
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(async move {
                let _ = local_settings_load.load().await;
            });
            let credentials = credential_store.clone();
            let manager = Arc::new(LocalRuntimeManager::platform_default(
                local_settings,
                credentials,
                Arc::new(TauriBroadcaster::new(handle)),
            ));
            app.manage(manager.clone());
            // Async app-launch init: scan + optional auto-start (design-v2 §14.1).
            manager.spawn_initialize();
            Ok(())
        })
        .on_window_event(|window, event| {
            // V2-FR-016: refresh observable local-runtime state when the app
            // regains focus (cheap, server-driven; never optimistic local
            // state). Window-close is intentionally NOT handled here — true
            // app exit stops the managed process via `ExitRequested` below.
            if let tauri::WindowEvent::Focused(true) = event {
                if let Some(manager) = window.app_handle().try_state::<Arc<LocalRuntimeManager>>() {
                    let manager = manager.inner().clone();
                    tokio::spawn(async move {
                        let _ = manager.refresh().await;
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::profiles::list_services,
            commands::profiles::get_service,
            commands::profiles::create_service,
            commands::profiles::update_service,
            commands::profiles::delete_service,
            commands::credentials::put_credential,
            commands::credentials::delete_credential,
            commands::connections::connect_service,
            commands::connections::respond_host_key_challenge,
            commands::connections::disconnect_service,
            commands::connections::get_connection_status,
            commands::connections::replace_known_host_and_connect,
            commands::viewer::open_service_view,
            commands::viewer::close_service_view,
            commands::local_runtime::get_local_runtime_status,
            commands::local_runtime::get_local_runtime_platform_support,
            commands::local_runtime::scan_local_installations,
            commands::local_runtime::validate_local_installation,
            commands::local_runtime::run_local_environment_doctor,
            commands::local_runtime::start_local_pi_hub,
            commands::local_runtime::stop_local_pi_hub,
            commands::local_runtime::restart_local_pi_hub,
            commands::local_runtime::get_local_runtime_settings,
            commands::local_runtime::update_local_runtime_settings,
            commands::local_runtime::get_local_runtime_logs,
            commands::local_runtime::clear_local_runtime_logs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Pi Hub Client");

    // design-v2 §14.3: on true app exit, stop the managed Pi Hub if the setting
    // is enabled. We block on a bounded, dedicated current-thread runtime so a
    // graceful SIGTERM (then SIGKILL) can complete before the process exits,
    // without depending on the app's runtime being multi-threaded. External
    // processes are never touched. Window-close while the app keeps running is
    // intentionally NOT handled here.
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            if let Some(manager) = app_handle.try_state::<Arc<LocalRuntimeManager>>() {
                let manager = manager.inner().clone();
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(rt) = rt {
                    rt.block_on(async move {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_secs(8),
                            manager.on_app_exit(),
                        )
                        .await;
                    });
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn build_state_loads_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        let state = build_state(path.clone(), None).await.unwrap();
        assert!(state.profile_store.list().await.unwrap().is_empty());
        // A subsequent build observes the same (still empty) file.
        let state2 = build_state(path, None).await.unwrap();
        assert!(state2.profile_store.list().await.unwrap().is_empty());
    }
}
