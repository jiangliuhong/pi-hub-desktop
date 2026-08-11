//! Local runtime Tauri commands (docs/design-v2.md §16).
//!
//! Thin adapters over [`LocalRuntimeManager`]. They only validate input, call
//! the domain service, map errors to the stable DTO and return serializable
//! output (AGENTS.md §5.2). The frontend can never pass arbitrary commands,
//! args or PIDs — only allowlisted settings fields and booleans (V2-SR-001).
//!
//! Capability: these commands belong to the trusted `main` window only. The
//! Service WebView keeps zero capability (V2-SR-005, design-v2 §16.2).

use crate::commands::profiles::map_err;
use crate::error::ErrorDto;
use crate::local_runtime::manager::LocalRuntimeManager;
use crate::local_runtime::model::{LocalRuntimeSnapshot, LogLine};
use crate::local_runtime::settings::{LocalRuntimeSettings, LocalRuntimeSettingsUpdate};
use serde::Deserialize;
use std::path::PathBuf;
use tauri::State;

/// Manual-selection validation input (V2-FR-003). Only absolute paths — never
/// arbitrary shell strings.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidateInstallationInput {
    #[serde(default)]
    pub node_executable: Option<PathBuf>,
    #[serde(default)]
    pub pi_hub_entrypoint: Option<PathBuf>,
    #[serde(default)]
    pub pi_hub_package_root: Option<PathBuf>,
}

/// Whether this build supports local Pi Hub management. iOS (`cfg(mobile)`)
/// returns false so the frontend hides the entry (design-v2 §16.1).
#[tauri::command]
pub async fn get_local_runtime_platform_support() -> bool {
    crate::local_runtime::platform_supported()
}

/// Result of a manual selection validation: the discovered facts (versions,
/// canonical paths) or an error explaining why the pair is unusable.
pub type ValidateInstallationOutput = crate::local_runtime::model::InstallationSet;

#[tauri::command]
pub async fn get_local_runtime_status(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSnapshot, ErrorDto> {
    Ok(manager.snapshot().await)
}

#[tauri::command]
pub async fn scan_local_installations(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSnapshot, ErrorDto> {
    manager.refresh().await.map_err(map_err)
}

#[tauri::command]
pub async fn validate_local_installation(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
    input: ValidateInstallationInput,
) -> Result<ValidateInstallationOutput, ErrorDto> {
    manager.validate_installation(input).await.map_err(map_err)
}

#[tauri::command]
pub async fn run_local_environment_doctor(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
    force: Option<bool>,
) -> Result<crate::local_runtime::model::EnvironmentReport, ErrorDto> {
    manager
        .run_doctor(force.unwrap_or(true))
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn start_local_pi_hub(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSnapshot, ErrorDto> {
    manager.start().await.map_err(map_err)
}

#[tauri::command]
pub async fn stop_local_pi_hub(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSnapshot, ErrorDto> {
    manager.stop().await.map_err(map_err)
}

#[tauri::command]
pub async fn restart_local_pi_hub(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSnapshot, ErrorDto> {
    manager.restart().await.map_err(map_err)
}

#[tauri::command]
pub async fn get_local_runtime_settings(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<LocalRuntimeSettings, ErrorDto> {
    Ok(manager.settings().await)
}

#[tauri::command]
pub async fn update_local_runtime_settings(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
    input: LocalRuntimeSettingsUpdate,
) -> Result<LocalRuntimeSettings, ErrorDto> {
    manager.update_settings(input).await.map_err(map_err)
}

#[tauri::command]
pub async fn get_local_runtime_logs(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
    limit: Option<u32>,
) -> Result<Vec<LogLine>, ErrorDto> {
    Ok(manager.logs(limit).await)
}

#[tauri::command]
pub async fn clear_local_runtime_logs(
    manager: State<'_, std::sync::Arc<LocalRuntimeManager>>,
) -> Result<(), ErrorDto> {
    manager.clear_logs().await;
    Ok(())
}

/// Re-exported so the command list in `lib.rs` can name the manager type.
pub use crate::local_runtime::manager::{NoopBroadcaster, StatusBroadcaster, TauriBroadcaster};
