//! Connection commands (docs/design-v1.md §13.1).
//!
//! Thin adapters over [`crate::connection::manager::ConnectionManager`]. The
//! Rust manager is the single source of truth for connection lifecycle; the UI
//! only reflects what these commands return (AGENTS.md §5.3).

use crate::commands::profiles::map_err;
use crate::connection::manager::{ConnectFlavor, ConnectionManager, HostKeyChallenge};
use crate::error::ErrorDto;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Result of `connect_service`. Either ready (with the URL to load) or a host
/// key confirmation the UI must show (FR-007).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectResult {
    Connected { effective_url: String },
    HostKeyChallenge(HostKeyChallengeDto),
}

#[derive(Debug, Clone, Serialize)]
pub struct HostKeyChallengeDto {
    pub challenge_id: Uuid,
    pub connection_id: String,
    pub service_id: Uuid,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub algorithm: String,
    pub sha256_fingerprint: String,
}

impl From<HostKeyChallenge> for HostKeyChallengeDto {
    fn from(c: HostKeyChallenge) -> Self {
        HostKeyChallengeDto {
            challenge_id: c.challenge_id,
            connection_id: c.connection_id.0.to_string(),
            service_id: c.service_id,
            ssh_host: c.ssh_host,
            ssh_port: c.ssh_port,
            algorithm: c.algorithm,
            sha256_fingerprint: c.sha256_fingerprint,
        }
    }
}

#[tauri::command]
pub async fn connect_service(
    manager: tauri::State<'_, std::sync::Arc<ConnectionManager>>,
    service_id: Uuid,
) -> Result<ConnectResult, ErrorDto> {
    match manager.connect(service_id).await.map_err(map_err)? {
        ConnectFlavor::Connected { effective_url } => {
            Ok(ConnectResult::Connected { effective_url })
        }
        ConnectFlavor::HostKeyChallenge(c) => Ok(ConnectResult::HostKeyChallenge(c.into())),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RespondHostKeyRequest {
    pub challenge_id: Uuid,
    pub accept: bool,
}

#[tauri::command]
pub async fn respond_host_key_challenge(
    manager: tauri::State<'_, std::sync::Arc<ConnectionManager>>,
    request: RespondHostKeyRequest,
) -> Result<ConnectResult, ErrorDto> {
    match manager
        .confirm_host_key(request.challenge_id, request.accept)
        .await
        .map_err(map_err)?
    {
        ConnectFlavor::Connected { effective_url } => {
            Ok(ConnectResult::Connected { effective_url })
        }
        ConnectFlavor::HostKeyChallenge(c) => Ok(ConnectResult::HostKeyChallenge(c.into())),
    }
}

#[tauri::command]
pub async fn disconnect_service(
    manager: tauri::State<'_, std::sync::Arc<ConnectionManager>>,
    service_id: Uuid,
) -> Result<(), ErrorDto> {
    manager
        .disconnect_service(service_id)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn get_connection_status(
    manager: tauri::State<'_, std::sync::Arc<ConnectionManager>>,
    service_id: Uuid,
) -> Result<Option<ConnectionStatusDto>, ErrorDto> {
    let snap = manager
        .status_for_service(service_id)
        .await
        .map_err(map_err)?;
    Ok(snap.map(Into::into))
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatusDto {
    pub state: String,
    pub effective_url: Option<String>,
    pub diagnostics: crate::connection::diagnostics::ConnectionDiagnostics,
}

impl From<crate::connection::manager::ConnectionSnapshot> for ConnectionStatusDto {
    fn from(s: crate::connection::manager::ConnectionSnapshot) -> Self {
        ConnectionStatusDto {
            state: s.state.api_name().to_string(),
            effective_url: s.effective_url,
            diagnostics: s.diagnostics,
        }
    }
}

#[tauri::command]
pub async fn replace_known_host_and_connect(
    manager: tauri::State<'_, std::sync::Arc<ConnectionManager>>,
    service_id: Uuid,
) -> Result<ConnectResult, ErrorDto> {
    match manager
        .replace_known_host_and_connect(service_id)
        .await
        .map_err(map_err)?
    {
        ConnectFlavor::Connected { effective_url } => {
            Ok(ConnectResult::Connected { effective_url })
        }
        ConnectFlavor::HostKeyChallenge(c) => Ok(ConnectResult::HostKeyChallenge(c.into())),
    }
}
