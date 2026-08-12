//! Package management Tauri commands (docs/requirements-v3.md §12, §14;
//! design §14).
//!
//! Thin adapters over [`PackageManagementManager`]. They only validate enum /
//! UUID / token inputs, call the domain service, map errors to the stable DTO
//! and return serializable output (AGENTS.md §5.3). The frontend can never
//! pass package names, version specs, commands, args, PIDs or paths (V3-SR-001).
//!
//! Capability: these commands belong to the trusted `main` window only. The
//! Service WebView keeps zero capability (V3-SR-006).

use crate::error::ErrorDto;
use crate::package_management::manager::PackageManagementManager;
use crate::package_management::model::{
    PackageManagementSnapshot, PackageOperationLogLine, ProductId,
};
use serde::Deserialize;
use tauri::State;

/// Whether this build supports package management. iOS returns false so the
/// frontend hides the entry (requirements-v3 §4.2).
#[tauri::command]
pub async fn get_package_management_platform_support() -> bool {
    crate::package_management::platform_supported()
}

#[tauri::command]
pub async fn get_package_management_status(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
) -> Result<PackageManagementSnapshot, ErrorDto> {
    Ok(manager.snapshot().await)
}

#[tauri::command]
pub async fn scan_managed_products(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    product: ProductId,
) -> Result<PackageManagementSnapshot, ErrorDto> {
    manager.scan(product).await.map_err(map_err)
}

#[tauri::command]
pub async fn check_product_updates(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    product: ProductId,
    force: Option<bool>,
) -> Result<PackageManagementSnapshot, ErrorDto> {
    manager
        .check_product_update(product, force.unwrap_or(true))
        .await
        .map_err(map_err)
}

/// Product id accepted from the frontend. Only the fixed enum; never a package
/// name or spec.
#[derive(Debug, Clone, Deserialize)]
pub struct ProductOpInput {
    pub product: ProductId,
    /// Opaque token minted by `check_product_updates` (design §9.3).
    pub release_token: String,
}

#[tauri::command]
pub async fn start_product_install(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    input: ProductOpInput,
) -> Result<crate::package_management::model::PackageOperationDto, ErrorDto> {
    let mgr = manager.inner().clone();
    mgr.start_install(input.product, input.release_token)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn start_product_update(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    input: ProductOpInput,
) -> Result<crate::package_management::model::PackageOperationDto, ErrorDto> {
    let mgr = manager.inner().clone();
    mgr.start_update(input.product, input.release_token)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn confirm_pi_hub_update_restart(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    operation_id: uuid::Uuid,
) -> Result<crate::package_management::model::PackageOperationDto, ErrorDto> {
    let mgr = manager.inner().clone();
    mgr.confirm_pi_hub_update_restart(operation_id)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn cancel_package_operation(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    operation_id: uuid::Uuid,
) -> Result<(), ErrorDto> {
    manager.cancel(operation_id).await.map_err(map_err)
}

#[tauri::command]
pub async fn activate_managed_product(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    product: ProductId,
) -> Result<PackageManagementSnapshot, ErrorDto> {
    manager.activate(product).await.map_err(map_err)
}

#[tauri::command]
pub async fn get_package_operation_log(
    manager: State<'_, std::sync::Arc<PackageManagementManager>>,
    operation_id: uuid::Uuid,
    limit: Option<u32>,
) -> Result<Vec<PackageOperationLogLine>, ErrorDto> {
    Ok(manager.operation_log(operation_id, limit).await)
}

/// Map a domain error to the stable error DTO.
fn map_err(err: crate::error::PackageManagementError) -> ErrorDto {
    crate::error::AppError::PackageManagement(err).to_dto()
}

/// Re-export so the command list in `lib.rs` can name the manager/broadcaster types.
pub use crate::package_management::manager::{
    NoopBroadcaster, PackageStatusBroadcaster, TauriBroadcaster,
};
