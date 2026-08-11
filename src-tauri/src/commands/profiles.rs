//! Service profile CRUD commands (docs/design-v1.md §13.1).
//!
//! Thin adapters over [`crate::profile::repository::ProfileStore`]. Secrets are
//! never accepted here — only credential id references (AGENTS.md §6.1).

use crate::credential::CredentialStore;
use crate::error::{AppError, ErrorDto};
use crate::profile::model::{
    ProfileInput, ServiceProfile, ServiceScheme, SshAuthType, CURRENT_SCHEMA_VERSION,
};
use crate::profile::repository::ProfileStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

/// DTOs accepted from the frontend. Plain flat struct → converted to the typed
/// `ProfileInput`. The frontend discriminated union maps to one of these.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "connection_type", rename_all = "snake_case")]
pub enum ProfileDraft {
    DirectUrl {
        name: String,
        base_url: String,
        #[serde(default)]
        pi_hub_credential_id: Option<String>,
    },
    SshForward {
        name: String,
        ssh_host: String,
        ssh_port: u16,
        ssh_username: String,
        ssh_auth_type: SshAuthType,
        ssh_credential_id: String,
        #[serde(default = "default_target_host")]
        target_host: String,
        #[serde(default = "default_target_port")]
        target_port: u16,
        #[serde(default = "default_service_scheme")]
        service_scheme: SerdeServiceScheme,
        #[serde(default = "default_base_path")]
        service_base_path: String,
        #[serde(default)]
        pi_hub_credential_id: Option<String>,
    },
}

/// Update DTO carries an existing id plus the draft fields.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateDraft {
    pub id: Uuid,
    #[serde(flatten)]
    pub draft: ProfileDraft,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerdeServiceScheme {
    Http,
    Https,
}
impl SerdeServiceScheme {
    fn into_domain(self) -> ServiceScheme {
        match self {
            SerdeServiceScheme::Http => ServiceScheme::Http,
            SerdeServiceScheme::Https => ServiceScheme::Https,
        }
    }
}

fn default_target_host() -> String {
    "127.0.0.1".to_string()
}
fn default_target_port() -> u16 {
    30142
}
fn default_service_scheme() -> SerdeServiceScheme {
    SerdeServiceScheme::Http
}
fn default_base_path() -> String {
    "/".to_string()
}

/// Serialized profile snapshot returned to the frontend. Carries the same
/// tagged union shape as the Rust model (AGENTS.md §11).
#[derive(Debug, Clone, Serialize)]
pub struct ServiceProfileDto {
    pub schema_version: u32,
    #[serde(flatten)]
    pub profile: ServiceProfile,
}

impl From<ServiceProfile> for ServiceProfileDto {
    fn from(profile: ServiceProfile) -> Self {
        ServiceProfileDto {
            schema_version: CURRENT_SCHEMA_VERSION,
            profile,
        }
    }
}

fn draft_to_input(draft: ProfileDraft) -> Result<ProfileInput, AppError> {
    Ok(match draft {
        ProfileDraft::DirectUrl {
            name,
            base_url,
            pi_hub_credential_id,
        } => ProfileInput::DirectUrl {
            name,
            base_url: Url::parse(&base_url).map_err(|e| {
                AppError::Profile(crate::error::ProfileError::Invalid(format!(
                    "base_url: {e}"
                )))
            })?,
            pi_hub_credential_id,
        },
        ProfileDraft::SshForward {
            name,
            ssh_host,
            ssh_port,
            ssh_username,
            ssh_auth_type,
            ssh_credential_id,
            target_host,
            target_port,
            service_scheme,
            service_base_path,
            pi_hub_credential_id,
        } => ProfileInput::SshForward {
            name,
            ssh_host,
            ssh_port,
            ssh_username,
            ssh_auth_type,
            ssh_credential_id,
            target_host,
            target_port,
            service_scheme: service_scheme.into_domain(),
            service_base_path,
            pi_hub_credential_id,
        },
    })
}

/// Map any domain error into the stable error DTO. Tauri commands return
/// this as the `Err` payload so the frontend always sees `{ code, message, ... }`.
/// Generic over the source error so `ProfileError`, `CredentialError`, etc.
/// all funnel through `AppError` first.
pub fn to_dto<E: Into<AppError>>(e: E) -> ErrorDto {
    e.into().to_dto()
}

/// Older name kept as a re-export for any call sites that name it explicitly.
pub fn map_err<E: Into<AppError>>(e: E) -> ErrorDto {
    to_dto(e)
}

impl From<AppError> for ErrorDto {
    fn from(e: AppError) -> Self {
        e.to_dto()
    }
}

#[tauri::command]
pub async fn list_services(
    store: tauri::State<'_, Arc<ProfileStore>>,
) -> Result<Vec<ServiceProfileDto>, ErrorDto> {
    store
        .list()
        .await
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(map_err)
}

#[tauri::command]
pub async fn get_service(
    store: tauri::State<'_, Arc<ProfileStore>>,
    id: Uuid,
) -> Result<ServiceProfileDto, ErrorDto> {
    store.get(id).await.map(Into::into).map_err(map_err)
}

#[tauri::command]
pub async fn create_service(
    store: tauri::State<'_, Arc<ProfileStore>>,
    draft: ProfileDraft,
) -> Result<ServiceProfileDto, ErrorDto> {
    let input = draft_to_input(draft)?;
    store.create(input).await.map(Into::into).map_err(map_err)
}

#[tauri::command]
pub async fn update_service(
    store: tauri::State<'_, Arc<ProfileStore>>,
    credentials: tauri::State<'_, Arc<dyn CredentialStore>>,
    payload: UpdateDraft,
) -> Result<ServiceProfileDto, ErrorDto> {
    // Load the existing profile so we can compute orphaned credentials after
    // the replacement (design §10.3). A new credential id is generated on every
    // save, so the old references must be purged from Keychain.
    let old = store.get(payload.id).await.map_err(map_err)?;
    let input = draft_to_input(payload.draft)?;
    // Materialize then overwrite the id so we replace the right row.
    let mut profile = input.into_profile();
    *profile.metadata_mut_id() = payload.id;
    let updated = store.update(profile).await.map_err(map_err)?;
    // After `update` the store holds the new profile, so orphaned_credentials
    // against `old` yields exactly the credentials that were dropped.
    let orphans = store.orphaned_credentials(&[old]).await.map_err(map_err)?;
    for cred_id in orphans {
        crate::commands::credentials::purge_credential(&credentials, &cred_id).await;
    }
    Ok(updated.into())
}

/// Helper trait to set the id on a freshly materialized profile (the input path
/// always allocates a new id; for updates we keep the existing one).
trait IdSetter {
    fn metadata_mut_id(&mut self) -> &mut Uuid;
}
impl IdSetter for ServiceProfile {
    fn metadata_mut_id(&mut self) -> &mut Uuid {
        match self {
            ServiceProfile::DirectUrl(p) => &mut p.metadata.id,
            ServiceProfile::SshForward(p) => &mut p.metadata.id,
        }
    }
}

#[tauri::command]
pub async fn delete_service(
    store: tauri::State<'_, Arc<ProfileStore>>,
    credentials: tauri::State<'_, Arc<dyn CredentialStore>>,
    manager: tauri::State<'_, Arc<crate::connection::manager::ConnectionManager>>,
    id: Uuid,
) -> Result<(), ErrorDto> {
    // FR-004: disconnect first, then delete profile + orphaned credentials
    // + host-key trust records.
    manager.disconnect_service(id).await.map_err(map_err)?;
    let removed = store.delete(id).await.map_err(map_err)?;
    // Capture the SSH endpoint (if any) before `removed` is moved into the
    // orphan computation, so we can also drop the known-host trust record.
    let ssh_endpoint = match &removed {
        ServiceProfile::SshForward(p) => Some((p.ssh_host.clone(), p.ssh_port)),
        _ => None,
    };
    let orphans = store
        .orphaned_credentials(&[removed])
        .await
        .map_err(map_err)?;
    for cred_id in orphans {
        crate::commands::credentials::purge_credential(&credentials, &cred_id).await;
    }
    // Drop the known-host record for this service's SSH endpoint, if any, so a
    // later re-add does not silently trust a stale key (FR-004).
    if let Some((host, port)) = ssh_endpoint {
        store
            .delete_known_host(&host, port)
            .await
            .map_err(map_err)?;
    }
    Ok(())
}
