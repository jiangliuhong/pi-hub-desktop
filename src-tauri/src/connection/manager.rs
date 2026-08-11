//! Connection manager (docs/design-v1.md §12).
//!
//! Owns the per-service connection lifecycle as the single source of truth
//! (AGENTS.md §5.3). Dedupes one active connection per service, drives the
//! state machine, holds resources via RAII + cancellation, and orchestrates
//! the host-key confirmation round-trip without ever auto-accepting a key.

use crate::connection::diagnostics::ConnectionDiagnostics;
use crate::connection::direct::DirectUrlProvider;
use crate::connection::provider::{
    ConnectContext, ConnectOutcome, ConnectionProvider, ConnectionResources,
};
use crate::connection::ssh_forward::SshForwardProvider;
use crate::connection::state::{ConnectionState, StateError};
use crate::credential::CredentialStore;
use crate::error::{AppError, ErrorCode, ProfileError, SshError};
use crate::profile::model::{ServiceProfile, SshForwardProfile};
use crate::profile::repository::ProfileStore;
use crate::ssh::client::PresentedHostKey;
use crate::ssh::host_key::{HostKeyFacts, KnownHostRecord};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Stable id for a connection attempt (distinct from the service id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConnectionId(pub Uuid);

impl ConnectionId {
    pub fn new() -> Self {
        ConnectionId(Uuid::new_v4())
    }
}

/// A pending host-key confirmation (FR-007). Carries only non-secret facts.
#[derive(Debug, Clone)]
pub struct HostKeyChallenge {
    pub challenge_id: Uuid,
    pub connection_id: ConnectionId,
    pub service_id: Uuid,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub algorithm: String,
    pub sha256_fingerprint: String,
}

/// Snapshot of a connection's current observable state for the UI.
#[derive(Debug, Clone)]
pub struct ConnectionSnapshot {
    pub id: ConnectionId,
    pub service_id: Uuid,
    pub state: ConnectionState,
    pub effective_url: Option<String>,
    pub diagnostics: ConnectionDiagnostics,
}

struct ManagedConnection {
    id: ConnectionId,
    service_id: Uuid,
    state: ConnectionState,
    /// Held so a hard cancel can be triggered independently of the graceful
    /// `ConnectionResources::shutdown` path (used by reconnect/backoff, which
    /// lands in the lifecycle phase).
    #[allow(dead_code)]
    cancellation: CancellationToken,
    resources: Option<ConnectionResources>,
    diagnostics: ConnectionDiagnostics,
    effective_url: Option<String>,
    /// Retry attempt counter for backoff (design §12.4). Surfaced in
    /// diagnostics; incremented by the reconnect task.
    #[allow(dead_code)]
    retry_count: u32,
}

impl ManagedConnection {
    fn set_state(&mut self, next: ConnectionState) -> Result<(), StateError> {
        self.state = self.state.transition(next)?;
        Ok(())
    }

    fn snapshot(&self) -> ConnectionSnapshot {
        ConnectionSnapshot {
            id: self.id,
            service_id: self.service_id,
            state: self.state,
            effective_url: self.effective_url.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

struct ManagerInner {
    connections: HashMap<ConnectionId, ManagedConnection>,
    by_service: HashMap<Uuid, ConnectionId>,
    challenges: HashMap<Uuid, PendingChallenge>,
}

/// Data retained while waiting for the user to confirm a host key.
struct PendingChallenge {
    connection_id: ConnectionId,
    service_id: Uuid,
    ssh_host: String,
    ssh_port: u16,
    presented: PresentedHostKey,
    cancellation: CancellationToken,
}

/// The connection manager. Cheap to clone (all state behind shared handles).
#[derive(Clone)]
pub struct ConnectionManager {
    inner: Arc<Mutex<ManagerInner>>,
    profiles: Arc<ProfileStore>,
    credentials: Arc<dyn CredentialStore>,
    direct: Arc<DirectUrlProvider>,
    ssh: Arc<SshForwardProvider>,
}

impl ConnectionManager {
    pub fn new(profiles: Arc<ProfileStore>, credentials: Arc<dyn CredentialStore>) -> Self {
        ConnectionManager {
            inner: Arc::new(Mutex::new(ManagerInner {
                connections: HashMap::new(),
                by_service: HashMap::new(),
                challenges: HashMap::new(),
            })),
            profiles,
            credentials,
            direct: Arc::new(DirectUrlProvider),
            ssh: Arc::new(SshForwardProvider),
        }
    }

    /// Resolve a profile by service id.
    async fn profile(&self, service_id: Uuid) -> Result<ServiceProfile, AppError> {
        self.profiles.get(service_id).await.map_err(AppError::from)
    }

    fn pick_provider(&self, profile: &ServiceProfile) -> Arc<dyn ConnectionProvider> {
        match profile {
            ServiceProfile::DirectUrl(_) => self.direct.clone() as Arc<dyn ConnectionProvider>,
            ServiceProfile::SshForward(_) => self.ssh.clone() as Arc<dyn ConnectionProvider>,
        }
    }

    /// Begin (or resume) a connection for `service_id`. Returns the effective
    /// URL on success, or a host-key challenge to confirm.
    pub async fn connect(&self, service_id: Uuid) -> Result<ConnectFlavor, AppError> {
        // Dedup: if this service already has an active connection, reuse it.
        {
            let inner = self.inner.lock().await;
            if let Some(cid) = inner.by_service.get(&service_id) {
                if let Some(conn) = inner.connections.get(cid) {
                    if conn.state == ConnectionState::Connected {
                        return Ok(ConnectFlavor::Connected {
                            effective_url: conn.effective_url.clone().unwrap_or_default(),
                        });
                    }
                }
            }
        }

        let profile = self.profile(service_id).await?;
        profile.validate()?;

        let cancellation = CancellationToken::new();
        let diagnostics = Arc::new(Mutex::new(ConnectionDiagnostics::new()));
        let known_host = self.known_host_for(&profile).await?;

        let context = ConnectContext {
            cancellation: cancellation.clone(),
            known_host,
            credentials: self.credentials.clone(),
            diagnostics: diagnostics.clone(),
        };

        let conn_id = ConnectionId::new();
        {
            let mut inner = self.inner.lock().await;
            inner.connections.insert(
                conn_id,
                ManagedConnection {
                    id: conn_id,
                    service_id,
                    state: ConnectionState::Idle,
                    cancellation: cancellation.clone(),
                    resources: None,
                    diagnostics: ConnectionDiagnostics::new(),
                    effective_url: None,
                    retry_count: 0,
                },
            );
            inner.by_service.insert(service_id, conn_id);
        }

        let provider = self.pick_provider(&profile);
        match provider.connect(&profile, &context).await {
            Ok(ConnectOutcome::Established(est)) => {
                let mut inner = self.inner.lock().await;
                let conn = inner
                    .connections
                    .get_mut(&conn_id)
                    .expect("connection just inserted");
                conn.set_state(ConnectionState::Validating)
                    .map_err(AppError::from)?;
                conn.set_state(ConnectionState::CheckingService)
                    .map_err(AppError::from)?;
                conn.set_state(ConnectionState::Connected)
                    .map_err(AppError::from)?;
                conn.effective_url = Some(est.effective_url.to_string());
                conn.resources = Some(est.resources);
                conn.diagnostics = diagnostics.lock().await.clone();
                Ok(ConnectFlavor::Connected {
                    effective_url: est.effective_url.to_string(),
                })
            }
            Ok(ConnectOutcome::NeedsHostKeyConfirmation {
                presented,
                ssh_host,
                ssh_port,
            }) => {
                let challenge_id = Uuid::new_v4();
                let challenge = HostKeyChallenge {
                    challenge_id,
                    connection_id: conn_id,
                    service_id,
                    ssh_host: ssh_host.clone(),
                    ssh_port,
                    algorithm: presented.algorithm.clone(),
                    sha256_fingerprint: presented.sha256_fingerprint.clone(),
                };
                let mut inner = self.inner.lock().await;
                inner.challenges.insert(
                    challenge_id,
                    PendingChallenge {
                        connection_id: conn_id,
                        service_id,
                        ssh_host,
                        ssh_port,
                        presented,
                        cancellation: cancellation.clone(),
                    },
                );
                if let Some(conn) = inner.connections.get_mut(&conn_id) {
                    let _ = conn.set_state(ConnectionState::VerifyingHostKey);
                    conn.diagnostics = diagnostics.lock().await.clone();
                    conn.diagnostics
                        .set_error(ErrorCode::HostKeyUnknown.snake_case_name());
                }
                Ok(ConnectFlavor::HostKeyChallenge(challenge))
            }
            Err(e) => {
                let mut inner = self.inner.lock().await;
                if let Some(conn) = inner.connections.get_mut(&conn_id) {
                    let _ = conn.set_state(ConnectionState::Error);
                    conn.diagnostics = diagnostics.lock().await.clone();
                    conn.diagnostics.set_error(e.code().snake_case_name());
                }
                Err(e)
            }
        }
    }

    /// User confirmed a host-key challenge. Persist the record and retry.
    pub async fn confirm_host_key(
        &self,
        challenge_id: Uuid,
        accept: bool,
    ) -> Result<ConnectFlavor, AppError> {
        let pending = {
            let mut inner = self.inner.lock().await;
            inner
                .challenges
                .remove(&challenge_id)
                .ok_or_else(|| AppError::from(ProfileError::NotFound))?
        };
        if !accept {
            // Rejected: cancel any in-flight tasks then tear down the connection.
            pending.cancellation.cancel();
            self.teardown(pending.connection_id).await;
            return Err(SshError::HostKeyUnknown {
                host: pending.ssh_host,
                port: pending.ssh_port,
            }
            .into());
        }

        // Persist the trusted key bound to (host, port) (FR-007).
        let facts = HostKeyFacts {
            algorithm: pending.presented.algorithm.clone(),
            sha256_fingerprint: pending.presented.sha256_fingerprint.clone(),
        };
        let record = KnownHostRecord {
            host: pending.ssh_host.clone(),
            port: pending.ssh_port,
            algorithm: facts.algorithm,
            public_key: pending.presented.public_key_bytes.clone(),
            sha256_fingerprint: facts.sha256_fingerprint,
            trusted_at: Utc::now(),
        };
        self.profiles.upsert_known_host(record).await?;

        // Retry the connect with the now-trusted key.
        self.connect(pending.service_id).await
    }

    /// Explicitly replace a known host after the doubly-confirmed "changed key"
    /// flow (FR-008). Returns the next connect flavor.
    pub async fn replace_known_host_and_connect(
        &self,
        service_id: Uuid,
    ) -> Result<ConnectFlavor, AppError> {
        // Delete the old record; the next connect will surface a fresh
        // confirmation with both old+new fingerprints visible.
        if let Some((host, port)) = self.ssh_endpoint(service_id).await? {
            self.profiles.delete_known_host(&host, port).await?;
        }
        self.connect(service_id).await
    }

    async fn ssh_endpoint(&self, service_id: Uuid) -> Result<Option<(String, u16)>, AppError> {
        match self.profile(service_id).await? {
            ServiceProfile::SshForward(SshForwardProfile {
                ssh_host, ssh_port, ..
            }) => Ok(Some((ssh_host, ssh_port))),
            _ => Ok(None),
        }
    }

    /// Disconnect a connection, releasing all resources (FR-010).
    pub async fn disconnect(&self, connection_id: ConnectionId) -> Result<(), AppError> {
        self.teardown(connection_id).await;
        Ok(())
    }

    /// Disconnect whatever connection is active for a service (used by delete).
    pub async fn disconnect_service(&self, service_id: Uuid) -> Result<(), AppError> {
        let cid = self.inner.lock().await.by_service.get(&service_id).copied();
        if let Some(cid) = cid {
            self.teardown(cid).await;
        }
        Ok(())
    }

    async fn teardown(&self, connection_id: ConnectionId) {
        let removed = {
            let mut inner = self.inner.lock().await;
            inner.connections.remove(&connection_id).map(|mut c| {
                // Drop the service mapping only if it still points here.
                if inner.by_service.get(&c.service_id) == Some(&connection_id) {
                    inner.by_service.remove(&c.service_id);
                }
                let _ = c.set_state(ConnectionState::Disconnecting);
                let _ = c.set_state(ConnectionState::Disconnected);
                c
            })
        };
        if let Some(mut conn) = removed {
            if let Some(res) = conn.resources.take() {
                res.shutdown().await;
            }
        }
    }

    /// Snapshot the current connection for a service (None if none active).
    pub async fn status_for_service(
        &self,
        service_id: Uuid,
    ) -> Result<Option<ConnectionSnapshot>, AppError> {
        let inner = self.inner.lock().await;
        let Some(cid) = inner.by_service.get(&service_id) else {
            return Ok(None);
        };
        Ok(inner.connections.get(cid).map(|c| c.snapshot()))
    }

    pub async fn status(
        &self,
        connection_id: ConnectionId,
    ) -> Result<Option<ConnectionSnapshot>, AppError> {
        Ok(self
            .inner
            .lock()
            .await
            .connections
            .get(&connection_id)
            .map(|c| c.snapshot()))
    }

    async fn known_host_for(
        &self,
        profile: &ServiceProfile,
    ) -> Result<Option<KnownHostRecord>, AppError> {
        if let ServiceProfile::SshForward(p) = profile {
            Ok(self
                .profiles
                .find_known_host(&p.ssh_host, p.ssh_port)
                .await?)
        } else {
            Ok(None)
        }
    }
}

/// Result of a connect attempt surfaced to the UI.
#[derive(Debug, Clone)]
pub enum ConnectFlavor {
    /// Connection ready; open the Service View at this URL.
    Connected { effective_url: String },
    /// SSH host key needs first-time confirmation (FR-007).
    HostKeyChallenge(HostKeyChallenge),
}

/// Map a state-machine violation onto `AppError`. A violation is a
/// programmer error; it is surfaced via the existing `ProfileError::Storage`
/// path, which maps to the internal error code (design §19).
impl From<StateError> for AppError {
    fn from(e: StateError) -> Self {
        AppError::Profile(ProfileError::Storage(format!(
            "connection state error: {e}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::in_memory::InMemoryCredentialStore;
    use crate::profile::model::ProfileInput;

    fn manager() -> ConnectionManager {
        ConnectionManager::new(
            Arc::new(ProfileStore::in_memory()),
            Arc::new(InMemoryCredentialStore::new()),
        )
    }

    #[tokio::test]
    async fn direct_connect_returns_effective_url() {
        let mgr = manager();
        let p = mgr.profiles.clone();
        let created = p
            .create(ProfileInput::DirectUrl {
                name: "Cloud".into(),
                base_url: url::Url::parse("https://pi.example.com").unwrap(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();

        match mgr.connect(created.id()).await.unwrap() {
            ConnectFlavor::Connected { effective_url } => {
                assert!(effective_url.starts_with("https://pi.example.com"));
            }
            _ => panic!("expected connected"),
        }

        let snap = mgr.status_for_service(created.id()).await.unwrap().unwrap();
        assert_eq!(snap.state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn second_connect_reuses_active_connection() {
        let mgr = manager();
        let p = mgr.profiles.clone();
        let created = p
            .create(ProfileInput::DirectUrl {
                name: "Cloud".into(),
                base_url: url::Url::parse("https://pi.example.com").unwrap(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();

        mgr.connect(created.id()).await.unwrap();
        // Second connect must not create a new connection (dedup, NFR-001).
        mgr.connect(created.id()).await.unwrap();
        let snap = mgr.status_for_service(created.id()).await.unwrap().unwrap();
        assert_eq!(snap.state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn disconnect_releases_connection() {
        let mgr = manager();
        let p = mgr.profiles.clone();
        let created = p
            .create(ProfileInput::DirectUrl {
                name: "Cloud".into(),
                base_url: url::Url::parse("https://pi.example.com").unwrap(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();
        mgr.connect(created.id()).await.unwrap();
        mgr.disconnect_service(created.id()).await.unwrap();
        // After disconnect the service has no active connection.
        assert!(mgr
            .status_for_service(created.id())
            .await
            .unwrap()
            .is_none());
    }
}
