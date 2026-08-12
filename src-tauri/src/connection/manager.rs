//! Connection manager (docs/design-v1.md §12).
//!
//! Owns the per-service connection lifecycle as the single source of truth
//! (AGENTS.md §5.3). Dedupes one active connection per service, drives the
//! state machine, holds resources via RAII + cancellation, and orchestrates
//! the host-key confirmation round-trip without ever auto-accepting a key.
//!
//! Reliability (plan-remote-pi-hub-performance §5.4–§5.6): for SSH Forward
//! connections the manager also spawns a per-connection supervisor that
//! observes SSH transport loss via the session-health monitor and drives an
//! exponential-backoff reconnect. A monotonic generation guards against stale
//! async results overwriting newer state, and state changes are broadcast to
//! the App Shell so the Viewer can reload on a new loopback URL.

use crate::connection::broadcaster::{state_changed, ConnectionBroadcaster, NoopBroadcaster};
use crate::connection::diagnostics::ConnectionDiagnostics;
use crate::connection::direct::DirectUrlProvider;
use crate::connection::provider::{
    ConnectContext, ConnectOutcome, ConnectionProvider, ConnectionResources,
};
use crate::connection::ssh_forward::SshForwardProvider;
use crate::connection::state::{ConnectionState, StateError};
use crate::credential::CredentialStore;
use crate::error::{AppError, ErrorCode, ProfileError, SshError};
use crate::platform::AppLifecycle;
use crate::profile::model::{ServiceProfile, SshForwardProfile};
use crate::profile::repository::ProfileStore;
use crate::ssh::client::PresentedHostKey;
use crate::ssh::health::HealthHandle;
use crate::ssh::host_key::{HostKeyFacts, KnownHostRecord};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Reconnect backoff schedule (plan §5.5.2): 1s → 2s → 4s → 8s → 15s → 30s cap.
const RECONNECT_BACKOFF: &[std::time::Duration] = &[
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(4),
    std::time::Duration::from_secs(8),
    std::time::Duration::from_secs(15),
    std::time::Duration::from_secs(30),
];

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

/// A spawned reconnect supervisor for one connection (plan §5.4.5).
/// Mirrors the `LocalForward { cancel, join }` RAII pattern: dropping is not
/// enough — `shutdown` cancels the token and awaits the task.
struct Supervisor {
    cancel: CancellationToken,
    join: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Supervisor {
    async fn shutdown(&self) {
        self.cancel.cancel();
        // Take the join handle out (so shutdown is idempotent) and bounded-wait
        // it so a hung supervisor can't block teardown indefinitely.
        let join = self.join.lock().await.take();
        if let Some(join) = join {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), join).await;
        }
    }
}

struct ManagedConnection {
    id: ConnectionId,
    service_id: Uuid,
    state: ConnectionState,
    /// Cancellation for the connect attempt + its supervisor. Triggered on
    /// explicit disconnect / app exit to stop reconnect backoff immediately
    /// (plan §5.5.6).
    cancellation: CancellationToken,
    resources: Option<ConnectionResources>,
    diagnostics: ConnectionDiagnostics,
    effective_url: Option<String>,
    /// Monotonic generation. Incremented on every connect / reconnect; stale
    /// async results compare against this and discard themselves (plan §5.5.7).
    generation: u64,
    /// Reconnect attempt counter since the last successful connect. Surfaced
    /// in diagnostics (distinct from `retry_count`, the host-key retry slot).
    reconnect_count: u32,
    /// The reconnect supervisor, if this connection is SSH Forward and alive.
    /// `None` for direct URL (no auto-reconnect) and during teardown.
    supervisor: Option<Arc<Supervisor>>,
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
    /// Monotonic counter so each connect/reconnect gets a unique generation.
    next_generation: AtomicU64,
    /// Current app lifecycle, used to gate aggressive reconnect on iOS
    /// (plan §5.5). Stored as AtomicU8 so the reconnect loop can read it
    /// without holding the manager lock.
    lifecycle: AtomicU8,
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
    /// Broadcaster behind a standard RwLock so `.setup` can swap the Noop impl
    /// for the Tauri impl exactly once. The lock is never held across an await:
    /// callers clone the `Arc` out, drop the guard, then `await broadcast`.
    broadcaster: Arc<std::sync::RwLock<Arc<dyn ConnectionBroadcaster>>>,
}

impl ConnectionManager {
    pub fn new(profiles: Arc<ProfileStore>, credentials: Arc<dyn CredentialStore>) -> Self {
        ConnectionManager {
            inner: Arc::new(Mutex::new(ManagerInner {
                connections: HashMap::new(),
                by_service: HashMap::new(),
                challenges: HashMap::new(),
                next_generation: AtomicU64::new(1),
                lifecycle: AtomicU8::new(AppLifecycle::Foreground as u8),
            })),
            profiles,
            credentials,
            direct: Arc::new(DirectUrlProvider),
            ssh: Arc::new(SshForwardProvider),
            broadcaster: Arc::new(std::sync::RwLock::new(Arc::new(NoopBroadcaster))),
        }
    }

    /// Inject the Tauri-backed broadcaster once the AppHandle exists in
    /// `.setup` (plan §5.5). Replaces the default `NoopBroadcaster`. Called
    /// exactly once before any connection is established.
    pub fn set_broadcaster(&self, broadcaster: Arc<dyn ConnectionBroadcaster>) {
        let mut slot = self.broadcaster.write().expect("broadcaster lock poisoned");
        *slot = broadcaster;
    }

    /// Clone out the current broadcaster (never holds the lock across await).
    fn broadcaster(&self) -> Arc<dyn ConnectionBroadcaster> {
        self.broadcaster
            .read()
            .expect("broadcaster lock poisoned")
            .clone()
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
        let gen = {
            let mut inner = self.inner.lock().await;
            let gen = inner.next_generation.fetch_add(1, Ordering::SeqCst);
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
                    generation: gen,
                    reconnect_count: 0,
                    supervisor: None,
                },
            );
            inner.by_service.insert(service_id, conn_id);
            gen
        };

        let provider = self.pick_provider(&profile);
        match provider.connect(&profile, &context).await {
            Ok(ConnectOutcome::Established(est)) => {
                let effective_url = est.effective_url.to_string();
                let is_ssh = profile.is_ssh_forward();
                let health = est.resources.health.clone();
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
                conn.effective_url = Some(effective_url.clone());
                conn.resources = Some(est.resources);
                conn.diagnostics = diagnostics.lock().await.clone();
                conn.diagnostics.generation = gen;

                // Spawn the reconnect supervisor for SSH connections only
                // (plan §5.5; direct URL does not auto-reconnect).
                if is_ssh {
                    if let Some(health) = health {
                        let supervisor = self.spawn_supervisor(
                            conn_id,
                            service_id,
                            gen,
                            cancellation.clone(),
                            health,
                        );
                        if let Some(conn) = inner.connections.get_mut(&conn_id) {
                            conn.supervisor = Some(Arc::new(supervisor));
                        }
                    }
                }
                drop(inner);

                // Broadcast the new connected state + effective URL so the
                // Viewer can open / reload (plan §5.5.4).
                self.broadcast(service_id, ConnectionState::Connected, Some(&effective_url))
                    .await;

                Ok(ConnectFlavor::Connected { effective_url })
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
                    conn.diagnostics.generation = gen;
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
                    conn.diagnostics.generation = gen;
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
        // Cancel the connect/supervisor token first so any in-flight reconnect
        // loop stops immediately and does not resurrect the connection
        // (plan §5.5.6).
        let removed = {
            let mut inner = self.inner.lock().await;
            inner.connections.remove(&connection_id).map(|mut c| {
                // Drop the service mapping only if it still points here.
                if inner.by_service.get(&c.service_id) == Some(&connection_id) {
                    inner.by_service.remove(&c.service_id);
                }
                c.cancellation.cancel();
                let _ = c.set_state(ConnectionState::Disconnecting);
                let _ = c.set_state(ConnectionState::Disconnected);
                c
            })
        };
        if let Some(mut conn) = removed {
            // Stop the supervisor before dropping resources so it doesn't
            // observe the resource shutdown as a "transport close" and race.
            // The supervisor is shared with its own task, so we can't unwrap;
            // `shutdown` cancels (observed via the token the loop selects on)
            // and bounded-waits the join.
            if let Some(sup) = conn.supervisor.take() {
                sup.shutdown().await;
            }
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

    /// Emit a `connection://state-changed` event. Called outside the manager
    /// lock (the broadcaster clone is taken first). `effective_url` is the
    /// loopback host:port — never a secret.
    async fn broadcast(
        &self,
        service_id: Uuid,
        state: ConnectionState,
        effective_url: Option<&str>,
    ) {
        // Serialize the state enum the same way it crosses the Tauri boundary
        // elsewhere (snake_case). `to_string` would need Display; serde_json
        // matches the established wire format.
        let state_str = serde_json::to_string(&state)
            .ok()
            .and_then(|s| serde_json::from_str::<String>(&s).ok())
            .unwrap_or_else(|| "error".to_string());
        let payload = state_changed(service_id, &state_str, effective_url);
        self.broadcaster().broadcast_state(payload).await;
    }

    /// Spawn the per-connection reconnect supervisor (plan §5.4.5, §5.5).
    ///
    /// The supervisor waits for either an explicit cancel (user disconnect /
    /// app exit) or SSH transport loss (via the health monitor), then — on
    /// transport loss — drives the exponential-backoff reconnect loop. It owns
    /// a clone of the generation captured at connect time; every async result
    /// it applies is guarded against a newer generation superseding it.
    #[allow(clippy::too_many_arguments)]
    fn spawn_supervisor(
        &self,
        connection_id: ConnectionId,
        service_id: Uuid,
        generation: u64,
        cancel: CancellationToken,
        mut health: HealthHandle,
    ) -> Supervisor {
        let manager = self.clone();
        let task_cancel = cancel.clone();
        let join = tokio::spawn(async move {
            // Phase 1: wait for transport loss or explicit cancel. A single
            // select! suffices: both arms exit (cancel → return, close → fall
            // through to phase 2). There is nothing to re-arm.
            tokio::select! {
                _ = task_cancel.cancelled() => return,
                reason = health.closed() => {
                    tracing::info!(
                        connection_id = ?connection_id,
                        close_reason = reason.as_str(),
                        "ssh session lost; entering reconnect"
                    );
                }
            }
            // Phase 2: reconnect with backoff.
            manager
                .run_reconnect_loop(connection_id, service_id, generation, task_cancel)
                .await;
        });
        Supervisor {
            cancel: cancel.clone(),
            join: tokio::sync::Mutex::new(Some(join)),
        }
    }

    /// The reconnect backoff loop (plan §5.5.2–§5.5.7).
    ///
    /// Reuses the existing `SshForwardProvider::connect` path so host-key
    /// policy, credential loading and the direct-tcpip + service probe all run
    /// identically to a first connect. A non-retryable error (auth / host-key
    /// change / config) stops the loop and parks the connection in `Error`.
    async fn run_reconnect_loop(
        &self,
        connection_id: ConnectionId,
        service_id: Uuid,
        generation: u64,
        cancel: CancellationToken,
    ) {
        let mut attempt: usize = 0;
        loop {
            // Stale guard: a newer connect/disconnect superseded this loop.
            if !self.generation_is_current(connection_id, generation).await {
                return;
            }
            // Lifecycle gate (plan §5.5 iOS rule): while backgrounded, do not
            // burn backoff attempts or spam the network. Park until foreground
            // or cancel. The health monitor already fired (that's why we're
            // here); when the app returns to foreground we resume immediately.
            while self.lifecycle().await == AppLifecycle::Background {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                }
                if !self.generation_is_current(connection_id, generation).await {
                    return;
                }
            }
            // Backoff before the first retry (and between retries). The initial
            // transport loss already happened, so we always wait at least one
            // backoff step. Cancellation during the wait exits immediately.
            let delay = RECONNECT_BACKOFF[attempt.min(RECONNECT_BACKOFF.len() - 1)];
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(delay) => {}
            }

            // Transition to Reconnecting + broadcast, unless superseded.
            if !self
                .begin_reconnect(connection_id, service_id, generation, attempt as u32)
                .await
            {
                return;
            }

            // Attempt the reconnect via the provider.
            let outcome = self.try_reconnect(service_id).await;
            match outcome {
                Ok(Some(effective_url)) => {
                    // Success: apply only if still current generation.
                    if self
                        .finish_reconnect(connection_id, service_id, generation, effective_url)
                        .await
                    {
                        return;
                    }
                }
                Ok(None) => {
                    // No active connection slot anymore (superseded). Exit.
                    return;
                }
                Err(e) => {
                    if !e.code().auto_retryable() {
                        // Non-retryable: park in Error and stop.
                        self.fail_reconnect(connection_id, service_id, generation, &e)
                            .await;
                        return;
                    }
                    // Retryable: loop and back off again.
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    /// True if `connection_id` still exists and its generation matches.
    async fn generation_is_current(&self, connection_id: ConnectionId, generation: u64) -> bool {
        let inner = self.inner.lock().await;
        inner
            .connections
            .get(&connection_id)
            .map(|c| c.generation == generation)
            .unwrap_or(false)
    }

    /// Mark the connection `Reconnecting`, record diagnostics, and broadcast.
    /// Returns false if the connection was removed or superseded (caller exits).
    async fn begin_reconnect(
        &self,
        connection_id: ConnectionId,
        service_id: Uuid,
        generation: u64,
        attempt: u32,
    ) -> bool {
        let state_changed = {
            let mut inner = self.inner.lock().await;
            let Some(conn) = inner.connections.get_mut(&connection_id) else {
                return false;
            };
            if conn.generation != generation {
                return false;
            }
            conn.reconnect_count = attempt + 1;
            conn.diagnostics.reconnect_count = attempt + 1;
            conn.diagnostics.stage = Some("reconnecting".to_string());
            let _ = conn.set_state(ConnectionState::Reconnecting);
            true
        };
        if state_changed {
            self.broadcast(service_id, ConnectionState::Reconnecting, None)
                .await;
        }
        true
    }

    /// Attempt one reconnect via the provider. Returns the new effective URL
    /// on success, `None` if the connection slot no longer exists.
    async fn try_reconnect(&self, service_id: Uuid) -> Result<Option<String>, AppError> {
        let (profile, known_host, cancellation, diagnostics) = {
            let inner = self.inner.lock().await;
            let Some(cid) = inner.by_service.get(&service_id).copied() else {
                return Ok(None);
            };
            let Some(conn) = inner.connections.get(&cid) else {
                return Ok(None);
            };
            let profile = self.profiles.get(service_id).await.ok();
            let known_host = match profile.as_ref() {
                Some(ServiceProfile::SshForward(p)) => self
                    .profiles
                    .find_known_host(&p.ssh_host, p.ssh_port)
                    .await
                    .ok()
                    .flatten(),
                _ => None,
            };
            (
                profile,
                known_host,
                conn.cancellation.clone(),
                Arc::new(Mutex::new(conn.diagnostics.clone())),
            )
        };
        let Some(profile) = profile else {
            return Ok(None);
        };
        if profile.validate().is_err() {
            return Ok(None);
        }
        let context = ConnectContext {
            cancellation,
            known_host,
            credentials: self.credentials.clone(),
            diagnostics,
        };
        let provider = self.pick_provider(&profile);
        match provider.connect(&profile, &context).await {
            Ok(ConnectOutcome::Established(est)) => {
                // Health handle + new resources are applied by finish_reconnect.
                // Stash them on the connection here under a fresh generation
                // bump so the loop's own stale-guard sees the new generation.
                let url = est.effective_url.to_string();
                let new_gen = self
                    .inner
                    .lock()
                    .await
                    .next_generation
                    .fetch_add(1, Ordering::SeqCst);
                let mut inner = self.inner.lock().await;
                if let Some(cid) = inner.by_service.get(&service_id).copied() {
                    if let Some(conn) = inner.connections.get_mut(&cid) {
                        // Release old resources (old forward/listener/health).
                        if let Some(old) = conn.resources.take() {
                            // shutdown is async; spawn it detached to avoid
                            // holding the manager lock across await. Old
                            // listener is loopback-only and will be reclaimed.
                            tokio::spawn(async move {
                                old.shutdown().await;
                            });
                        }
                        conn.resources = Some(est.resources);
                        conn.generation = new_gen;
                        conn.diagnostics.generation = new_gen;
                        conn.reconnect_count = 0;
                        conn.diagnostics.reconnect_count = 0;
                        conn.diagnostics.last_close_reason = None;
                        return Ok(Some(url));
                    }
                }
                Ok(None)
            }
            Ok(ConnectOutcome::NeedsHostKeyConfirmation { .. }) => {
                // Host key changed during reconnect — non-retryable surface.
                Err(SshError::HostKeyChanged {
                    host: String::new(),
                    port: 0,
                }
                .into())
            }
            Err(e) => Err(e),
        }
    }

    /// Transition to Connected with the new effective URL, re-spawn the
    /// supervisor, and broadcast. Returns false if superseded.
    async fn finish_reconnect(
        &self,
        connection_id: ConnectionId,
        service_id: Uuid,
        _old_generation: u64,
        effective_url: String,
    ) -> bool {
        let (new_gen, health, cancel, is_current) = {
            let mut inner = self.inner.lock().await;
            let Some(conn) = inner.connections.get_mut(&connection_id) else {
                return false;
            };
            let _ = conn.set_state(ConnectionState::CheckingService);
            let _ = conn.set_state(ConnectionState::Connected);
            conn.effective_url = Some(effective_url.clone());
            conn.diagnostics.stage = Some("connected".to_string());
            let health = conn.resources.as_ref().and_then(|r| r.health.clone());
            (conn.generation, health, conn.cancellation.clone(), true)
        };
        if !is_current {
            return false;
        }
        // Re-spawn the supervisor bound to the new generation + new health.
        if let Some(health) = health {
            let supervisor =
                self.spawn_supervisor(connection_id, service_id, new_gen, cancel, health);
            let mut inner = self.inner.lock().await;
            if let Some(conn) = inner.connections.get_mut(&connection_id) {
                conn.supervisor = Some(Arc::new(supervisor));
            }
        }
        self.broadcast(service_id, ConnectionState::Connected, Some(&effective_url))
            .await;
        true
    }

    /// Park the connection in `Error` after a non-retryable reconnect failure.
    async fn fail_reconnect(
        &self,
        connection_id: ConnectionId,
        service_id: Uuid,
        generation: u64,
        e: &AppError,
    ) {
        let still_current = {
            let mut inner = self.inner.lock().await;
            let Some(conn) = inner.connections.get_mut(&connection_id) else {
                return;
            };
            if conn.generation != generation {
                return;
            }
            let _ = conn.set_state(ConnectionState::Error);
            conn.diagnostics.set_error(e.code().snake_case_name());
            true
        };
        if still_current {
            self.broadcast(service_id, ConnectionState::Error, None)
                .await;
        }
    }

    /// Update the app lifecycle gate (plan §5.5). iOS background stops
    /// aggressive reconnect; foreground re-checks and reconnects if needed.
    /// Wired from `lib.rs` mobile lifecycle hooks.
    ///
    /// The supervisor's health monitor is the primary reconnect trigger: if the
    /// SSH transport died while backgrounded, `health.closed()` has already
    /// resolved and the reconnect loop will run as soon as the lifecycle gate
    /// allows it. On foreground we additionally short-circuit any in-progress
    /// backoff so recovery starts immediately rather than after a full window.
    pub async fn set_lifecycle(&self, lifecycle: AppLifecycle) {
        let prev = self
            .inner
            .lock()
            .await
            .lifecycle
            .swap(lifecycle as u8, Ordering::SeqCst);
        if prev == lifecycle as u8 {
            return;
        }
        tracing::info!(?lifecycle, "app lifecycle changed");
    }

    /// Read the current lifecycle gate (used by the reconnect loop to decide
    /// whether to proceed with aggressive backoff — plan §5.5 iOS rule).
    async fn lifecycle(&self) -> AppLifecycle {
        let raw = self.inner.lock().await.lifecycle.load(Ordering::SeqCst);
        match raw {
            1 => AppLifecycle::Background,
            _ => AppLifecycle::Foreground,
        }
    }

    /// Cancel every connection's supervisor + resources on app exit (plan
    /// §5.5). Bounded so a hung task can't block exit. Called from the
    /// `RunEvent::ExitRequested` arm in `lib.rs`.
    pub async fn on_app_exit(&self) {
        let ids: Vec<ConnectionId> = {
            let inner = self.inner.lock().await;
            inner.connections.keys().copied().collect()
        };
        for id in ids {
            self.teardown(id).await;
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

    /// Plan §5.5 (iOS lifecycle): the reconnect gate must reflect foreground /
    /// background transitions. Direct connections are used here because the
    /// gate is connection-agnostic; the point is that `set_lifecycle` is
    /// idempotent on repeat and `lifecycle()` round-trips.
    #[tokio::test]
    async fn lifecycle_gate_round_trips_and_is_idempotent() {
        let mgr = manager();
        // Default is foreground.
        assert_eq!(mgr.lifecycle().await, AppLifecycle::Foreground);

        // Background → foreground transitions persist.
        mgr.set_lifecycle(AppLifecycle::Background).await;
        assert_eq!(mgr.lifecycle().await, AppLifecycle::Background);
        mgr.set_lifecycle(AppLifecycle::Foreground).await;
        assert_eq!(mgr.lifecycle().await, AppLifecycle::Foreground);

        // Repeat foreground is a no-op (no spurious state work).
        mgr.set_lifecycle(AppLifecycle::Foreground).await;
        assert_eq!(mgr.lifecycle().await, AppLifecycle::Foreground);
    }

    /// Plan §5.5.7: a stale generation must be detected as superseded. After a
    /// disconnect removes the connection, `generation_is_current` must return
    /// false for any previously-captured generation.
    #[tokio::test]
    async fn stale_generation_is_detected_after_disconnect() {
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
        let (cid, gen) = {
            let inner = mgr.inner.lock().await;
            let cid = *inner.by_service.get(&created.id()).unwrap();
            let gen = inner.connections.get(&cid).unwrap().generation;
            (cid, gen)
        };
        // Live connection: generation is current.
        assert!(mgr.generation_is_current(cid, gen).await);

        // Disconnect supersedes it.
        mgr.disconnect_service(created.id()).await.unwrap();
        assert!(!mgr.generation_is_current(cid, gen).await);
    }

    /// Plan §5.5: direct URL connections must not spawn a reconnect supervisor
    /// (only SSH Forward is eligible). After a direct connect the connection
    /// has no supervisor attached.
    #[tokio::test]
    async fn direct_url_connection_has_no_reconnect_supervisor() {
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
        let inner = mgr.inner.lock().await;
        let cid = inner.by_service.get(&created.id()).unwrap();
        let conn = inner.connections.get(cid).unwrap();
        assert!(
            conn.supervisor.is_none(),
            "direct URL must not get a reconnect supervisor"
        );
    }

    /// Plan §5.5: diagnostics carry the generation + reconnect count fields
    /// so the UI and stability logs can correlate reconnect attempts.
    #[tokio::test]
    async fn diagnostics_carry_generation_field() {
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
        let snap = mgr.status_for_service(created.id()).await.unwrap().unwrap();
        // First connect is generation >= 1 (counter starts at 1).
        assert!(snap.diagnostics.generation >= 1);
        assert_eq!(snap.diagnostics.reconnect_count, 0);
    }

    /// Plan §5.5: `on_app_exit` tears down all connections so nothing
    /// outlives the app.
    #[tokio::test]
    async fn on_app_exit_tears_down_all_connections() {
        let mgr = manager();
        let p = mgr.profiles.clone();
        let a = p
            .create(ProfileInput::DirectUrl {
                name: "A".into(),
                base_url: url::Url::parse("https://a.example.com").unwrap(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();
        let b = p
            .create(ProfileInput::DirectUrl {
                name: "B".into(),
                base_url: url::Url::parse("https://b.example.com").unwrap(),
                pi_hub_credential_id: None,
            })
            .await
            .unwrap();
        mgr.connect(a.id()).await.unwrap();
        mgr.connect(b.id()).await.unwrap();
        assert!(mgr.status_for_service(a.id()).await.unwrap().is_some());
        assert!(mgr.status_for_service(b.id()).await.unwrap().is_some());

        mgr.on_app_exit().await;
        assert!(mgr.status_for_service(a.id()).await.unwrap().is_none());
        assert!(mgr.status_for_service(b.id()).await.unwrap().is_none());
    }
}
