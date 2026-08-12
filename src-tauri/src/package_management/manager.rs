//! `PackageManagementManager` — the V3 orchestration core
//! (docs/requirements-v3.md §6–§14; design §7, §11, §13).
//!
//! Single source of truth for the package-management snapshot and the only
//! thing permitted to mutate it. It serializes install/update operations
//! (one at a time), guards against stale tasks overwriting newer state
//! (generation), computes UI permissions server-side, and delegates Pi Hub
//! activation/stop/restart to `LocalRuntimeManager` via a minimal port — it
//! never kills Pi Hub itself (V3-SR-002, design §19.1).
//!
//! On iOS the manager is constructed with no services, so every operation
//! returns `unsupported_platform` (requirements-v3 §4.2).

use crate::error::PackageManagementError;
use crate::local_runtime::detector::{DetectionHints, InstallationDetector, TokioCommandRunner};
use crate::local_runtime::manager::LocalRuntimeManager;
use crate::local_runtime::model::{
    InstallationSet, InstallationSource, LocalRuntimeState, NodeInstallation,
};
use crate::local_runtime::settings::LocalRuntimeSettingsStore;
use crate::package_management::installer::{InstallSpec, PackageInstaller};
use crate::package_management::managed_store::{ActiveEntry, ManagedPackageStore};
use crate::package_management::model::*;
use crate::package_management::npm_toolchain::{
    DefaultNpmToolchainDetector, NpmToolchain, NpmToolchainDetector,
};
use crate::package_management::operation::{OperationHandle, OperationLogBuffer, OperationLogSink};
use crate::package_management::release_client::ReleaseClient;
use crate::package_management::verifier::{PostInstallVerifier, VerifiedInstall};
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

/// Event names (design §14.2).
pub const STATUS_CHANGED_EVENT: &str = "package-management://status-changed";
pub const OPERATION_CHANGED_EVENT: &str = "package-management://operation-changed";

/// Bounded wait for in-flight op cancellation on app exit (design §13.2).
const EXIT_CANCEL_WAIT: Duration = Duration::from_secs(8);

/// Broadcasts non-sensitive snapshot/operation events to the App Shell only
/// (design §14.2). Never targets the Service WebView (V3-SR-006).
#[async_trait::async_trait]
pub trait PackageStatusBroadcaster: Send + Sync {
    async fn broadcast_status(&self, snapshot: &PackageManagementSnapshot);
    async fn broadcast_operation(&self, op: &PackageOperationDto);
}

/// No-op broadcaster for tests / headless construction.
pub struct NoopBroadcaster;
#[async_trait::async_trait]
impl PackageStatusBroadcaster for NoopBroadcaster {
    async fn broadcast_status(&self, _snapshot: &PackageManagementSnapshot) {}
    async fn broadcast_operation(&self, _op: &PackageOperationDto) {}
}

/// Tauri-backed broadcaster.
pub struct TauriBroadcaster {
    handle: tauri::AppHandle,
}
impl TauriBroadcaster {
    pub fn new(handle: tauri::AppHandle) -> Self {
        TauriBroadcaster { handle }
    }
}
#[async_trait::async_trait]
impl PackageStatusBroadcaster for TauriBroadcaster {
    async fn broadcast_status(&self, snapshot: &PackageManagementSnapshot) {
        use tauri::Emitter;
        let _ = self.handle.emit(STATUS_CHANGED_EVENT, snapshot.clone());
    }
    async fn broadcast_operation(&self, op: &PackageOperationDto) {
        use tauri::Emitter;
        let _ = self.handle.emit(OPERATION_CHANGED_EVENT, op.clone());
    }
}

/// Minimal port through which the package manager asks `LocalRuntimeManager`
/// to stop/start/refresh Pi Hub and repoint its settings to a managed install
/// (design §19.1). The package manager never holds a Pi Hub child handle.
#[async_trait::async_trait]
pub trait PiHubActivationPort: Send + Sync {
    async fn runtime_state(&self) -> LocalRuntimeState;
    async fn stop(&self) -> Result<(), PackageManagementError>;
    async fn start(&self) -> Result<(), PackageManagementError>;
    async fn refresh(&self) -> Result<(), PackageManagementError>;
    /// Repoint V2 local-runtime settings to the given managed Pi Hub install.
    async fn apply_pi_hub_paths(&self, entry: &ActiveEntry) -> Result<(), PackageManagementError>;
}

/// Default adapter wrapping the real `LocalRuntimeManager`.
pub struct LocalRuntimeActivationAdapter {
    manager: Arc<LocalRuntimeManager>,
}

impl LocalRuntimeActivationAdapter {
    pub fn new(manager: Arc<LocalRuntimeManager>) -> Self {
        LocalRuntimeActivationAdapter { manager }
    }
}

#[async_trait::async_trait]
impl PiHubActivationPort for LocalRuntimeActivationAdapter {
    async fn runtime_state(&self) -> LocalRuntimeState {
        self.manager.snapshot().await.runtime_state
    }
    async fn stop(&self) -> Result<(), PackageManagementError> {
        self.manager.stop().await.map_err(map_runtime_err)?;
        Ok(())
    }
    async fn start(&self) -> Result<(), PackageManagementError> {
        self.manager.start().await.map_err(map_runtime_err)?;
        Ok(())
    }
    async fn refresh(&self) -> Result<(), PackageManagementError> {
        self.manager.refresh().await.map_err(map_runtime_err)?;
        Ok(())
    }
    async fn apply_pi_hub_paths(&self, entry: &ActiveEntry) -> Result<(), PackageManagementError> {
        use crate::local_runtime::settings::LocalRuntimeSettingsUpdate;
        self.manager
            .update_settings(LocalRuntimeSettingsUpdate {
                node_executable: Some(entry.node_executable.clone()),
                pi_hub_entrypoint: Some(entry.entrypoint.clone()),
                pi_hub_package_root: Some(entry.package_root.clone()),
                ..Default::default()
            })
            .await
            .map_err(map_runtime_err)?;
        Ok(())
    }
}

fn map_runtime_err(e: crate::error::LocalRuntimeError) -> PackageManagementError {
    // Surface a non-sensitive summary; the LocalRuntime error's own DTO already
    // carries the actionable detail, but the package domain reports its own
    // stable codes.
    match e {
        crate::error::LocalRuntimeError::ProcessNotOwned
        | crate::error::LocalRuntimeError::UnsupportedPlatform => {
            PackageManagementError::ExternalRuntimeActive
        }
        crate::error::LocalRuntimeError::OperationInProgress => {
            PackageManagementError::ActivationFailed {
                product: "pi_hub".into(),
                reason: "local runtime busy".into(),
            }
        }
        other => PackageManagementError::ActivationFailed {
            product: "pi_hub".into(),
            reason: other.code().snake_case_name().to_string(),
        },
    }
}

/// The bundled domain services. `None` on iOS → unsupported operations.
pub struct Services {
    pub detector: Arc<dyn InstallationDetector>,
    pub runner: Arc<dyn crate::local_runtime::detector::CommandRunner>,
    pub release_client: Arc<dyn ReleaseClient>,
    pub npm_detector: Arc<dyn NpmToolchainDetector>,
    pub installer: Arc<dyn PackageInstaller>,
    pub verifier: Arc<dyn PostInstallVerifier>,
    pub store: Arc<ManagedPackageStore>,
    pub settings: Arc<LocalRuntimeSettingsStore>,
    pub pi_hub_port: Option<Arc<dyn PiHubActivationPort>>,
}

pub struct PackageManagementManager {
    snapshot: RwLock<PackageManagementSnapshot>,
    /// At most one in-flight operation; the handle is held for its lifetime.
    op: Mutex<Option<Arc<OperationHandle>>>,
    pending_releases: Mutex<HashMap<uuid::Uuid, ReleaseToken>>,
    /// Recent operation logs (incl. completed), keyed by op id.
    op_logs: Mutex<HashMap<uuid::Uuid, Arc<OperationLogBuffer>>>,
    services: Option<Arc<Services>>,
    broadcaster: Arc<dyn PackageStatusBroadcaster>,
    generation: AtomicU64,
}

impl PackageManagementManager {
    pub fn new(services: Option<Services>, broadcaster: Arc<dyn PackageStatusBroadcaster>) -> Self {
        PackageManagementManager {
            snapshot: RwLock::new(PackageManagementSnapshot::default()),
            op: Mutex::new(None),
            pending_releases: Mutex::new(HashMap::new()),
            op_logs: Mutex::new(HashMap::new()),
            services: services.map(Arc::new),
            broadcaster,
            generation: AtomicU64::new(0),
        }
    }

    /// Platform-default constructor: real services on macOS/Linux, `None` on
    /// iOS (requirements-v3 §4.2).
    pub fn platform_default(
        store: Arc<ManagedPackageStore>,
        settings: Arc<LocalRuntimeSettingsStore>,
        pi_hub_manager: Option<Arc<LocalRuntimeManager>>,
        broadcaster: Arc<dyn PackageStatusBroadcaster>,
    ) -> Self {
        #[cfg(not(target_os = "ios"))]
        {
            let runner: Arc<dyn crate::local_runtime::detector::CommandRunner> =
                Arc::new(TokioCommandRunner);
            let pi_hub_port: Option<Arc<dyn PiHubActivationPort>> = pi_hub_manager.map(|m| {
                Arc::new(LocalRuntimeActivationAdapter::new(m)) as Arc<dyn PiHubActivationPort>
            });
            let services = Services {
                detector: Arc::new(
                    crate::local_runtime::detector::DefaultInstallationDetector::with_default_runner(),
                ),
                runner: runner.clone(),
                release_client: Arc::new(
                    crate::package_management::release_client::NpmRegistryReleaseClient::new(),
                ),
                npm_detector: Arc::new(DefaultNpmToolchainDetector::with_default_runner()),
                installer: Arc::new(crate::package_management::installer::TokioPackageInstaller),
                verifier: Arc::new(
                    crate::package_management::verifier::DefaultPostInstallVerifier::with_default_runner(),
                ),
                store,
                settings,
                pi_hub_port,
            };
            Self::new(Some(services), broadcaster)
        }
        #[cfg(target_os = "ios")]
        {
            let _ = (store, settings, pi_hub_manager);
            Self::new(None, broadcaster)
        }
    }

    fn unsupported() -> PackageManagementError {
        PackageManagementError::UnsupportedPlatform
    }

    /// Current observable snapshot (cheap clone).
    pub async fn snapshot(&self) -> PackageManagementSnapshot {
        self.snapshot.read().await.clone()
    }

    async fn broadcast_status(&self, snap: &PackageManagementSnapshot) {
        self.broadcaster.broadcast_status(snap).await;
    }

    async fn set_op(&self, op: Option<Arc<OperationHandle>>) {
        let mut guard = self.op.lock().await;
        let prev = std::mem::replace(&mut *guard, op);
        // Keep the previous op's log in `op_logs` for history.
        if let Some(h) = &prev {
            self.op_logs
                .lock()
                .await
                .entry(h.id)
                .or_insert_with(|| h.log.clone());
            self.prune_op_logs().await;
        }
    }

    async fn prune_op_logs(&self) {
        let mut logs = self.op_logs.lock().await;
        if logs.len() > crate::package_management::operation::MAX_RECENT_OPERATIONS + 4 {
            // Drop the oldest beyond the cap (best-effort by insertion order via
            // started_at would need tracking; keep it bounded simply).
            let drop_n = logs.len() - crate::package_management::operation::MAX_RECENT_OPERATIONS;
            let to_drop: Vec<_> = logs.keys().take(drop_n).copied().collect();
            for k in to_drop {
                logs.remove(&k);
            }
        }
    }

    /// Refresh local detection + prerequisites, rebuild the snapshot (Phase 1).
    pub async fn scan(&self) -> Result<PackageManagementSnapshot, PackageManagementError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        self.bump_generation();
        let set = svc
            .detector
            .detect(&hints_from_settings(&svc.settings.get().await))
            .await
            .map_err(|e| {
                PackageManagementError::Internal(e.code().snake_case_name().to_string())
            })?;
        let prereqs = self.build_prereqs(&set).await;
        let snap = self.rebuild_snapshot(&set, &prereqs).await?;
        self.broadcast_status(&snap).await;
        Ok(snap)
    }

    /// Check stable releases for both products, mint opaque tokens, rebuild
    /// (design §9). `force` bypasses the cache TTL.
    pub async fn check_updates(
        &self,
        force: bool,
    ) -> Result<PackageManagementSnapshot, PackageManagementError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        // Mark checking.
        {
            let mut snap = self.snapshot.write().await;
            for p in snap.products.iter_mut() {
                p.update_status = UpdateStatus::Checking;
            }
            let cloned = snap.clone();
            drop(snap);
            self.broadcast_status(&cloned).await;
        }
        let now = Utc::now();
        for product in ProductId::all() {
            match svc.release_client.latest(*product, force).await {
                Ok(info) => {
                    let token = ReleaseToken::new(info);
                    let id = token.id;
                    self.pending_releases.lock().await.insert(id, token);
                    let _ = svc.store.set_last_update_check(now).await;
                }
                Err(_) => {
                    // Leave any prior token; offline keeps last success in the
                    // client cache. No snapshot mutation here.
                }
            }
        }
        let set = svc
            .detector
            .detect(&hints_from_settings(&svc.settings.get().await))
            .await
            .map_err(|e| {
                PackageManagementError::Internal(e.code().snake_case_name().to_string())
            })?;
        let prereqs = self.build_prereqs(&set).await;
        let snap = self.rebuild_snapshot(&set, &prereqs).await?;
        self.broadcast_status(&snap).await;
        Ok(snap)
    }

    /// Begin an install transaction (Phase 2). `release_token` is the opaque id
    /// minted by `check_updates`.
    pub async fn start_install(
        self: &Arc<Self>,
        product: ProductId,
        release_token: String,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        self.run_transaction(product, release_token, PackageOperationKind::Install, None)
            .await
    }

    /// Begin an update transaction (Phase 3). Reuses the install pipeline; for
    /// Pi Hub it coordinates activation/restart via the port.
    pub async fn start_update(
        self: &Arc<Self>,
        product: ProductId,
        release_token: String,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        self.run_transaction(product, release_token, PackageOperationKind::Update, None)
            .await
    }

    /// Shared install/update transaction core (design §11.1, §11.2).
    async fn run_transaction(
        self: &Arc<Self>,
        product: ProductId,
        release_token: String,
        kind: PackageOperationKind,
        _prev: Option<()>,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        let svc = svc.clone();

        // 1. Acquire the single operation slot.
        {
            let mut guard = self.op.lock().await;
            if guard.is_some() {
                return Err(PackageManagementError::OperationInProgress);
            }
            // For Pi Hub, refuse if the runtime is busy (design §13.1).
            if product == ProductId::PiHub {
                if let Some(port) = &svc.pi_hub_port {
                    let st = port.runtime_state().await;
                    if matches!(
                        st,
                        LocalRuntimeState::Starting
                            | LocalRuntimeState::Stopping
                            | LocalRuntimeState::Checking
                    ) {
                        return Err(PackageManagementError::ActivationFailed {
                            product: "pi_hub".into(),
                            reason: "runtime busy".into(),
                        });
                    }
                }
            }
            let from_version = self.current_version(product).await;
            let handle = Arc::new(OperationHandle::new(product, kind, from_version, None));
            *guard = Some(handle.clone());
            self.op_logs
                .lock()
                .await
                .insert(handle.id, handle.log.clone());
        }

        let result = self
            .transaction_inner(svc.clone(), product, kind, release_token)
            .await;

        // On terminal completion/failure/cancel, release the slot (unless the
        // op is parked in AwaitingRestartConfirmation).
        match result {
            Ok(dto) => Ok(dto),
            Err(e) => {
                self.fail_op(e.clone()).await;
                Err(e)
            }
        }
    }

    async fn transaction_inner(
        self: &Arc<Self>,
        svc: Arc<Services>,
        product: ProductId,
        _kind: PackageOperationKind,
        release_token: String,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        let handle = self
            .current_handle()
            .await
            .ok_or_else(|| PackageManagementError::Internal("operation handle missing".into()))?;

        // 2. Fetch + freeze the release.
        handle.set_stage(PackageOperationStage::FetchingMetadata);
        self.emit_op(&handle).await;
        let token = self.redeem_token(product, &release_token).await?;
        handle.set_target_version(Some(token.version.to_string()));

        // 3. Prerequisites.
        let (node, npm) = self.prereqs_pair(&svc).await?;
        let node_install = node.ok_or(PackageManagementError::NodeUnavailable)?;
        let _ = npm.ok_or(PackageManagementError::NpmUnavailable)?;

        // 4. Staging.
        let staging = svc.store.create_staging(product, handle.id).await?;
        handle.log.push(
            PackageOperationStage::Preparing,
            PackageLogLevel::Info,
            "staging created",
        );

        // 5. Install.
        handle.set_stage(PackageOperationStage::Installing);
        self.emit_op(&handle).await;
        let spec = InstallSpec {
            product,
            version: token.version.clone(),
            toolchain: NpmToolchain {
                node_executable: node_install.canonical_executable.clone(),
                npm_cli_js: self.resolve_npm_cli(&svc, &node_install).await?.npm_cli_js,
                npm_version: String::new(),
                source: node_install.source,
            },
            staging_dir: staging.clone(),
            cancel: handle.cancel.clone(),
            deadline: crate::package_management::installer::DEFAULT_INSTALL_DEADLINE,
        };
        if let Err(e) = svc.installer.install(spec, handle.log.clone()).await {
            svc.store.remove_staging(&staging).await;
            return Err(e);
        }

        // 6. Verify.
        handle.set_stage(PackageOperationStage::Verifying);
        self.emit_op(&handle).await;
        let verified: VerifiedInstall = match svc
            .verifier
            .verify(
                product,
                token.version.clone(),
                &staging,
                &node_install.canonical_executable,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                svc.store.remove_staging(&staging).await;
                return Err(e);
            }
        };

        // 7. Promote (activates manifest, retains previous for rollback).
        handle.set_stage(PackageOperationStage::Activating);
        self.emit_op(&handle).await;
        let entry = svc
            .store
            .promote(
                product,
                &staging,
                &token.version,
                verified.entrypoint.clone(),
                node_install.canonical_executable.clone(),
            )
            .await?;
        handle.log.push(
            PackageOperationStage::Activating,
            PackageLogLevel::Info,
            &format!("activated {} {}", entry.package_name, entry.version),
        );

        // 8. Product-specific activation policy.
        if product == ProductId::PiHub {
            match self.pihub_activation_policy(&svc, &handle, &entry).await? {
                ActivationPolicy::Parked => {
                    // Slot stays held; stage remains AwaitingRestartConfirmation.
                    let dto = handle.to_dto();
                    self.emit_op(&handle).await;
                    let _ = self.scan_internal_quiet(&svc).await;
                    return Ok(dto);
                }
                ActivationPolicy::AutoCompleted => {}
            }
        }

        // Done. Clean up old versions, release slot, mark completed.
        svc.store.cleanup_versions(product).await;
        handle.set_stage(PackageOperationStage::Completed);
        let dto = handle.to_dto();
        self.set_op(None).await;
        // Refresh detection so the snapshot reflects the new managed active.
        let _ = self.scan_internal_quiet(&svc).await;
        self.emit_op(&handle).await;
        Ok(dto)
    }

    /// Pi Hub activation policy after a successful promote (design §6.6).
    /// Returns whether the op auto-completed or is parked awaiting the user's
    /// restart confirmation.
    async fn pihub_activation_policy(
        self: &Arc<Self>,
        svc: &Arc<Services>,
        handle: &Arc<OperationHandle>,
        entry: &ActiveEntry,
    ) -> Result<ActivationPolicy, PackageManagementError> {
        let Some(port) = &svc.pi_hub_port else {
            return Ok(ActivationPolicy::AutoCompleted);
        };
        let state = port.runtime_state().await;
        match state {
            LocalRuntimeState::RunningManaged => {
                // Park until the user confirms "update and restart".
                handle.set_stage(PackageOperationStage::AwaitingRestartConfirmation);
                self.emit_op(handle).await;
                Ok(ActivationPolicy::Parked)
            }
            LocalRuntimeState::RunningExternal => {
                // Do not stop the external process. Promote is done; the user
                // can activate the managed copy later via `activate`.
                handle.log.push(
                    PackageOperationStage::Completed,
                    PackageLogLevel::Info,
                    "external Pi Hub running; managed copy ready, activation deferred",
                );
                Ok(ActivationPolicy::AutoCompleted)
            }
            LocalRuntimeState::Stopped
            | LocalRuntimeState::Failed
            | LocalRuntimeState::PortConflict
            | LocalRuntimeState::Unknown => {
                // Repoint settings to the managed install. Do not auto-start
                // (design §6.4: don't default-start Pi Hub after install).
                port.apply_pi_hub_paths(entry).await?;
                Ok(ActivationPolicy::AutoCompleted)
            }
            LocalRuntimeState::Starting
            | LocalRuntimeState::Stopping
            | LocalRuntimeState::Checking => Err(PackageManagementError::ActivationFailed {
                product: "pi_hub".into(),
                reason: "runtime became busy during activation".into(),
            }),
        }
    }

    /// Resume a parked Pi Hub update: stop current → repoint → start → verify
    /// (design §6.6 running_managed flow). Rolls back on start failure.
    pub async fn confirm_pi_hub_update_restart(
        self: &Arc<Self>,
        operation_id: uuid::Uuid,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        let svc = svc.clone();
        let handle = self
            .current_handle_for(operation_id)
            .await
            .ok_or_else(|| PackageManagementError::Internal("operation not found".into()))?;
        if handle.product != ProductId::PiHub
            || handle.stage() != PackageOperationStage::AwaitingRestartConfirmation
        {
            return Err(PackageManagementError::ActivationFailed {
                product: "pi_hub".into(),
                reason: "not awaiting restart".into(),
            });
        }
        let Some(port) = &svc.pi_hub_port else {
            return Err(PackageManagementError::Internal("no pi hub port".into()));
        };

        // Snapshot the previous entry for rollback before we touch anything.
        let previous = svc.store.previous_active(ProductId::PiHub).await;
        let new_entry = svc.store.active(ProductId::PiHub).await.ok_or_else(|| {
            PackageManagementError::ActivationFailed {
                product: "pi_hub".into(),
                reason: "no managed active to restart".into(),
            }
        })?;

        handle.set_stage(PackageOperationStage::Restarting);
        self.emit_op(&handle).await;

        // Stop current managed, repoint, start.
        if let Err(e) = port.stop().await {
            return self.rollback_on_failure(&svc, &handle, &previous, e).await;
        }
        if let Err(e) = port.apply_pi_hub_paths(&new_entry).await {
            return self.rollback_on_failure(&svc, &handle, &previous, e).await;
        }
        if let Err(e) = port.start().await {
            return self.rollback_on_failure(&svc, &handle, &previous, e).await;
        }

        handle.set_stage(PackageOperationStage::Completed);
        let dto = handle.to_dto();
        self.set_op(None).await;
        let _ = self.scan_internal_quiet(&svc).await;
        self.emit_op(&handle).await;
        Ok(dto)
    }

    async fn rollback_on_failure(
        self: &Arc<Self>,
        svc: &Arc<Services>,
        handle: &Arc<OperationHandle>,
        previous: &Option<ActiveEntry>,
        start_err: PackageManagementError,
    ) -> Result<PackageOperationDto, PackageManagementError> {
        handle.set_stage(PackageOperationStage::RollingBack);
        self.emit_op(handle).await;
        let rollback_result = match previous {
            Some(prev) => {
                let r = svc.store.restore_previous(ProductId::PiHub).await;
                // Repoint settings back to the previous entry, then attempt to
                // restart it so the user is not left without a service.
                if let Some(restored) = svc.store.active(ProductId::PiHub).await {
                    if let Some(port) = &svc.pi_hub_port {
                        let _ = port.apply_pi_hub_paths(&restored).await;
                        // Best-effort: ensure nothing of the failed-new is running.
                        let _ = port.stop().await;
                        let _ = port.start().await;
                    }
                }
                let _ = prev;
                r
            }
            None => Ok(None),
        };
        match rollback_result {
            Ok(_) => {
                let msg = format!(
                    "pi hub restart failed; rolled back: {}",
                    start_err.user_message_for_issue()
                );
                handle.log.push(
                    PackageOperationStage::RollingBack,
                    PackageLogLevel::Error,
                    &msg,
                );
                let final_err = PackageManagementError::ActivationFailed {
                    product: "pi_hub".into(),
                    reason: "restart failed, rolled back".into(),
                };
                self.fail_op(final_err.clone()).await;
                Err(final_err)
            }
            Err(e) => {
                let rb = PackageManagementError::RollbackFailed {
                    product: "pi_hub".into(),
                    reason: e.user_message_for_issue(),
                };
                self.fail_op(rb.clone()).await;
                Err(rb)
            }
        }
    }

    /// Cancel an in-flight operation (design §13.2). Only the app-owned npm
    /// child is ever signalled (via the installer's process-group kill).
    pub async fn cancel(&self, operation_id: uuid::Uuid) -> Result<(), PackageManagementError> {
        let guard = self.op.lock().await;
        if let Some(h) = guard.as_ref() {
            if h.id == operation_id && h.stage().cancellable() {
                h.cancel.cancel();
                return Ok(());
            }
        }
        Err(PackageManagementError::Internal(
            "operation not cancellable".into(),
        ))
    }

    /// Switch the current selection to the managed installation already on
    /// disk (design §15.5: does not delete other versions / external installs).
    pub async fn activate(
        &self,
        product: ProductId,
    ) -> Result<PackageManagementSnapshot, PackageManagementError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        // Only a managed active install can be activated by the frontend; the
        // manager is the source of truth for which managed install exists
        // (design §8.2). External installs are never guessed by id.
        let entry = svc.store.active(product).await.ok_or_else(|| {
            PackageManagementError::ActivationFailed {
                product: product.api_name().into(),
                reason: "no managed active".into(),
            }
        })?;
        if product == ProductId::PiHub {
            if let Some(port) = &svc.pi_hub_port {
                let st = port.runtime_state().await;
                if matches!(st, LocalRuntimeState::RunningExternal) {
                    return Err(PackageManagementError::ExternalRuntimeActive);
                }
                port.apply_pi_hub_paths(&entry).await?;
            }
        }
        let set = svc
            .detector
            .detect(&hints_from_settings(&svc.settings.get().await))
            .await
            .map_err(|e| {
                PackageManagementError::Internal(e.code().snake_case_name().to_string())
            })?;
        let prereqs = self.build_prereqs(&set).await;
        let snap = self.rebuild_snapshot(&set, &prereqs).await?;
        self.broadcast_status(&snap).await;
        Ok(snap)
    }

    /// Recent sanitized log lines for an operation.
    pub async fn operation_log(
        &self,
        operation_id: uuid::Uuid,
        limit: Option<u32>,
    ) -> Vec<crate::package_management::model::PackageOperationLogLine> {
        if let Some(h) = self.op.lock().await.as_ref() {
            if h.id == operation_id {
                return h.log.recent(limit.map(|n| n as usize));
            }
        }
        if let Some(buf) = self.op_logs.lock().await.get(&operation_id) {
            return buf.recent(limit.map(|n| n as usize));
        }
        Vec::new()
    }

    /// App-exit hook: cancel + bounded-wait any in-flight op, clean staging.
    pub async fn on_app_exit(&self) {
        let handle = self.op.lock().await.clone();
        if let Some(h) = &handle {
            if h.stage().cancellable() {
                h.cancel.cancel();
            }
        }
        if let Some(h) = &handle {
            // Bound the wait so a stuck npm can't hang exit.
            let _ = tokio::time::timeout(EXIT_CANCEL_WAIT, h.cancel.cancelled()).await;
        }
        if let Some(svc) = &self.services {
            svc.store.cleanup_stale_staging(Utc::now()).await;
        }
    }

    // ---- internals ----

    fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    async fn current_handle(&self) -> Option<Arc<OperationHandle>> {
        self.op.lock().await.clone()
    }

    async fn current_handle_for(&self, id: uuid::Uuid) -> Option<Arc<OperationHandle>> {
        self.op
            .lock()
            .await
            .as_ref()
            .and_then(|h| if h.id == id { Some(h.clone()) } else { None })
    }

    async fn current_version(&self, product: ProductId) -> Option<String> {
        self.snapshot
            .read()
            .await
            .products
            .iter()
            .find(|p| p.product == product)
            .and_then(|p| p.current.as_ref().and_then(|c| c.version.clone()))
    }

    async fn emit_op(&self, handle: &Arc<OperationHandle>) {
        self.broadcaster.broadcast_operation(&handle.to_dto()).await;
    }

    async fn fail_op(&self, err: PackageManagementError) {
        if let Some(h) = self.op.lock().await.as_ref() {
            h.set_stage(PackageOperationStage::Failed);
            h.log.push(
                PackageOperationStage::Failed,
                PackageLogLevel::Error,
                &err.user_message_for_issue(),
            );
            let mut dto = h.to_dto();
            dto.issue = Some(issue_from_error(&err));
            let _ = self.broadcaster.broadcast_operation(&dto).await;
        }
        self.set_op(None).await;
    }

    async fn redeem_token(
        &self,
        product: ProductId,
        token_id: &str,
    ) -> Result<ReleaseToken, PackageManagementError> {
        let id = uuid::Uuid::parse_str(token_id)
            .map_err(|_| PackageManagementError::ReleaseTokenExpired)?;
        let pending = self.pending_releases.lock().await;
        let token = pending
            .get(&id)
            .cloned()
            .ok_or(PackageManagementError::ReleaseTokenExpired)?;
        if token.product != product || token.is_expired(Utc::now()) {
            return Err(PackageManagementError::ReleaseTokenExpired);
        }
        Ok(token)
    }

    async fn prereqs_pair(
        &self,
        svc: &Services,
    ) -> Result<(Option<NodeInstallation>, Option<NpmToolchain>), PackageManagementError> {
        let set = svc
            .detector
            .detect(&hints_from_settings(&svc.settings.get().await))
            .await
            .map_err(|e| {
                PackageManagementError::Internal(e.code().snake_case_name().to_string())
            })?;
        let node = set.node.clone();
        let npm = if let Some(n) = &node {
            svc.npm_detector.detect(n).await.ok()
        } else {
            None
        };
        Ok((node, npm))
    }

    async fn resolve_npm_cli(
        &self,
        svc: &Services,
        node: &NodeInstallation,
    ) -> Result<NpmToolchain, PackageManagementError> {
        svc.npm_detector.detect(node).await
    }

    async fn build_prereqs(&self, set: &InstallationSet) -> PackagePrerequisites {
        let node = if let Some(n) = &set.node {
            ProductPrerequisite {
                name: "node".into(),
                satisfied: true,
                version: Some(n.version.clone()),
                location: Some(n.canonical_executable.clone()),
                issue: None,
            }
        } else {
            ProductPrerequisite {
                name: "node".into(),
                satisfied: false,
                version: None,
                location: None,
                issue: Some("未找到满足基线的 Node.js（>=22.19.0）。".into()),
            }
        };
        let npm = if let Some(n) = &set.node {
            if let Some(svc) = &self.services {
                match svc.npm_detector.detect(n).await {
                    Ok(tc) => ProductPrerequisite {
                        name: "npm".into(),
                        satisfied: true,
                        version: Some(tc.npm_version),
                        location: Some(tc.npm_cli_js),
                        issue: None,
                    },
                    Err(_) => ProductPrerequisite {
                        name: "npm".into(),
                        satisfied: false,
                        version: None,
                        location: None,
                        issue: Some("未找到与 Node 配套的 npm CLI。".into()),
                    },
                }
            } else {
                PackagePrerequisites::default().npm
            }
        } else {
            ProductPrerequisite {
                name: "npm".into(),
                satisfied: false,
                version: None,
                location: None,
                issue: Some("Node.js 不可用，npm 不可用。".into()),
            }
        };
        PackagePrerequisites { node, npm }
    }

    /// Rebuild the full snapshot from fresh detection facts.
    async fn rebuild_snapshot(
        &self,
        set: &InstallationSet,
        prereqs: &PackagePrerequisites,
    ) -> Result<PackageManagementSnapshot, PackageManagementError> {
        let mut products = Vec::new();
        for product in ProductId::all() {
            let status = self.build_product_status(*product, set, prereqs).await;
            products.push(status);
        }
        let active_op = self.op.lock().await.as_ref().map(|h| {
            let mut dto = h.to_dto();
            if h.stage() == PackageOperationStage::Failed {
                dto.issue = Some(PackageIssueDto {
                    code: "package_activation_failed".into(),
                    message: "operation failed".into(),
                });
            }
            dto
        });
        let last_check = match &self.services {
            Some(s) => s.store.last_update_check().await,
            None => None,
        };
        let snap = PackageManagementSnapshot {
            platform_supported: crate::package_management::platform_supported(),
            prerequisites: prereqs.clone(),
            products,
            active_operation: active_op,
            checked_at: Some(Utc::now()),
        };
        // Carry last_update_check_at into products via tokens already minted.
        let mut snap = snap;
        for p in snap.products.iter_mut() {
            p.last_update_check_at = last_check;
        }
        *self.snapshot.write().await = snap.clone();
        Ok(snap)
    }

    async fn build_product_status(
        &self,
        product: ProductId,
        set: &InstallationSet,
        prereqs: &PackagePrerequisites,
    ) -> ProductStatus {
        let managed = match &self.services {
            Some(s) => s.store.active(product).await,
            None => None,
        };

        let (install_state, current, alternatives) = match product {
            ProductId::Pi => self.build_pi_status(set, &managed).await,
            ProductId::PiHub => self.build_pihub_status(set, &managed).await,
        };

        // Update status from cached token, if any.
        let (update_status, latest_version, release_token) =
            self.update_state(product, current.as_ref()).await;

        let pi_hub_state = if product == ProductId::PiHub {
            self.services
                .as_ref()
                .and_then(|s| s.pi_hub_port.as_ref())
                .cloned()
        } else {
            None
        };
        let pi_hub_state = if let Some(port) = pi_hub_state {
            Some(port.runtime_state().await)
        } else {
            None
        };

        let allowed = compute_actions(
            product,
            install_state,
            update_status,
            release_token.is_some(),
            prereqs.node.satisfied && prereqs.npm.satisfied,
            managed.is_some(),
            current.as_ref().map(|c| c.ownership) == Some(InstallOwnership::External),
            pi_hub_state,
            self.op
                .lock()
                .await
                .as_ref()
                .map(|h| (h.product, h.stage())),
        );

        ProductStatus {
            product,
            install_state,
            current,
            alternatives,
            update_status,
            latest_version,
            last_update_check_at: None,
            release_token,
            allowed_actions: allowed,
            issue: None,
        }
    }

    async fn build_pi_status(
        &self,
        set: &InstallationSet,
        managed: &Option<ActiveEntry>,
    ) -> (
        ProductInstallState,
        Option<ProductInstallationDto>,
        Vec<ProductInstallationDto>,
    ) {
        let mut alts = Vec::new();
        let mut current: Option<ProductInstallationDto> = None;
        let mut state = ProductInstallState::NotInstalled;

        if let Some(m) = managed {
            current = Some(active_entry_to_dto(m));
            state = ProductInstallState::Installed;
        }
        if let Some(cli) = &set.pi_cli {
            let ext = strengthen_pi(cli);
            if current.is_none() {
                current = Some(ext);
                state = ProductInstallState::Installed;
            } else {
                alts.push(ext);
            }
        }
        (state, current, alts)
    }

    async fn build_pihub_status(
        &self,
        set: &InstallationSet,
        managed: &Option<ActiveEntry>,
    ) -> (
        ProductInstallState,
        Option<ProductInstallationDto>,
        Vec<ProductInstallationDto>,
    ) {
        let mut alts = Vec::new();
        let mut current: Option<ProductInstallationDto> = None;
        let mut state = ProductInstallState::NotInstalled;

        if let Some(m) = managed {
            current = Some(active_entry_to_dto(m));
            state = ProductInstallState::Installed;
        }
        if let Some(ph) = &set.pi_hub {
            let canon_entry = ph.entrypoint.clone();
            let ext = ProductInstallationDto {
                installation_id: installation_id_from_path(&canon_entry),
                package_name: "@jarome/pi-hub".into(),
                version: Some(ph.version.clone()),
                executable: None,
                package_root: Some(ph.package_root.clone()),
                entrypoint: Some(canon_entry),
                source: ph.source,
                ownership: InstallOwnership::External,
                kind: Some(PackageKind::Npm),
            };
            if current.is_none() {
                current = Some(ext);
                state = ProductInstallState::Installed;
            } else {
                alts.push(ext);
            }
        }
        (state, current, alts)
    }

    async fn update_state(
        &self,
        product: ProductId,
        current: Option<&ProductInstallationDto>,
    ) -> (UpdateStatus, Option<String>, Option<String>) {
        let pending = self.pending_releases.lock().await;
        let token = pending.values().find(|t| t.product == product).cloned();
        drop(pending);
        let Some(token) = token else {
            return (UpdateStatus::Unknown, None, None);
        };
        let latest = token.version.to_string();
        let token_id = token.id.to_string();
        match current.and_then(|c| c.version.as_deref()) {
            None => (UpdateStatus::Available, Some(latest), Some(token_id)),
            Some(v) => {
                let Ok(cur) = semver::Version::parse(v) else {
                    return (UpdateStatus::Unknown, Some(latest), Some(token_id));
                };
                match cur.cmp(&token.version) {
                    std::cmp::Ordering::Less => {
                        (UpdateStatus::Available, Some(latest), Some(token_id))
                    }
                    std::cmp::Ordering::Equal => {
                        (UpdateStatus::UpToDate, Some(latest), Some(token_id))
                    }
                    std::cmp::Ordering::Greater => {
                        (UpdateStatus::NewerThanLatest, Some(latest), Some(token_id))
                    }
                }
            }
        }
    }

    /// Refresh detection quietly (no broadcast storm) and update the snapshot.
    async fn scan_internal_quiet(&self, svc: &Arc<Services>) -> Result<(), PackageManagementError> {
        let set = svc
            .detector
            .detect(&hints_from_settings(&svc.settings.get().await))
            .await
            .map_err(|e| {
                PackageManagementError::Internal(e.code().snake_case_name().to_string())
            })?;
        let prereqs = self.build_prereqs(&set).await;
        let snap = self.rebuild_snapshot(&set, &prereqs).await?;
        self.broadcast_status(&snap).await;
        Ok(())
    }
}

/// Outcome of the Pi Hub activation policy.
enum ActivationPolicy {
    AutoCompleted,
    Parked,
}

/// Async helper to fetch a managed active entry off a service handle without
/// holding a borrow across an await boundary awkwardly.
#[allow(dead_code)]
async fn futures_lookup_active(svc: &Arc<Services>, product: ProductId) -> Option<ActiveEntry> {
    svc.store.active(product).await
}

/// Convert a PiCliInstallation into a strengthened ProductInstallationDto,
/// classifying npm vs standalone by locating an adjacent package.json
/// (design §8.3).
fn strengthen_pi(cli: &crate::local_runtime::model::PiCliInstallation) -> ProductInstallationDto {
    let exe = cli.executable.clone();
    let (pkg_root, kind) = locate_pi_package(&exe);
    let package_name = "@earendil-works/pi-coding-agent".to_string();
    let version = if let Some(root) = &pkg_root {
        read_pkg_version(root).or_else(|| cli.version.clone())
    } else {
        cli.version.clone()
    };
    ProductInstallationDto {
        installation_id: installation_id_from_path(&exe),
        package_name,
        version,
        executable: Some(exe.clone()),
        package_root: pkg_root,
        entrypoint: Some(exe),
        source: cli.source,
        ownership: InstallOwnership::External,
        kind: Some(kind),
    }
}

/// Walk up from a `pi` binary to find a package.json declaring the pi package.
fn locate_pi_package(exe: &Path) -> (Option<PathBuf>, PackageKind) {
    let Some(mut dir) = exe.parent().map(PathBuf::from) else {
        return (None, PackageKind::Standalone);
    };
    for _ in 0..8 {
        let pkg = dir.join("package.json");
        if let Ok(raw) = std::fs::read_to_string(&pkg) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if v.get("name").and_then(|n| n.as_str()) == Some("@earendil-works/pi-coding-agent")
                {
                    return (Some(dir.clone()), PackageKind::Npm);
                }
            }
        }
        dir = match dir.parent().map(PathBuf::from) {
            Some(p) => p,
            None => break,
        };
    }
    (None, PackageKind::Standalone)
}

fn read_pkg_version(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("version")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn active_entry_to_dto(entry: &ActiveEntry) -> ProductInstallationDto {
    ProductInstallationDto {
        installation_id: installation_id_from_path(&entry.entrypoint),
        package_name: entry.package_name.clone(),
        version: Some(entry.version.clone()),
        executable: None,
        package_root: Some(entry.package_root.clone()),
        entrypoint: Some(entry.entrypoint.clone()),
        source: InstallationSource::DesktopManaged,
        ownership: InstallOwnership::DesktopManaged,
        kind: Some(PackageKind::Npm),
    }
}

fn hints_from_settings(
    settings: &crate::local_runtime::settings::LocalRuntimeSettings,
) -> DetectionHints {
    DetectionHints {
        persisted_node: settings.node_executable.clone(),
        persisted_pi_hub_entrypoint: settings.pi_hub_entrypoint.clone(),
        persisted_pi_hub_package_root: settings.pi_hub_package_root.clone(),
        path_dirs: app_path_dirs(),
        home_override: None,
    }
}

fn app_path_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            out.push(dir);
        }
    }
    out
}

/// Compute the UI action allowlist (design §6.3, §12). Pure function so it is
/// unit-testable.
#[allow(clippy::too_many_arguments)]
pub fn compute_actions(
    product: ProductId,
    install_state: ProductInstallState,
    update_status: UpdateStatus,
    has_release_token: bool,
    prereqs_ok: bool,
    has_managed_active: bool,
    external_current: bool,
    pi_hub_state: Option<LocalRuntimeState>,
    in_flight: Option<(ProductId, PackageOperationStage)>,
) -> Vec<ProductAction> {
    use ProductAction::*;
    let mut out = vec![Scan];

    // Operation in flight governs the card.
    if let Some((p, stage)) = in_flight {
        if p == product {
            match stage {
                PackageOperationStage::AwaitingRestartConfirmation => {
                    out.push(ConfirmRestart);
                }
                s if s.cancellable() => out.push(Cancel),
                _ => {}
            }
            return out;
        }
    }

    out.push(CheckUpdates);

    let busy = matches!(
        pi_hub_state,
        Some(
            LocalRuntimeState::Starting | LocalRuntimeState::Stopping | LocalRuntimeState::Checking
        )
    );

    match install_state {
        ProductInstallState::NotInstalled => {
            if prereqs_ok && has_release_token && update_status == UpdateStatus::Available {
                out.push(Install);
            }
        }
        ProductInstallState::Installed => {
            if prereqs_ok && has_release_token && update_status == UpdateStatus::Available && !busy
            {
                out.push(Update);
            }
            // Switch to a managed copy if one exists and current isn't it.
            if has_managed_active && external_current && !busy {
                // External Pi Hub running: allow choosing managed, but it won't
                // stop the external process.
                out.push(Activate);
            }
        }
        ProductInstallState::Invalid => {
            out.push(Repair);
        }
        ProductInstallState::Incompatible => {
            if prereqs_ok && has_release_token && update_status == UpdateStatus::Available {
                out.push(Install);
            }
        }
        ProductInstallState::Unknown => {}
    }
    out
}

#[cfg(test)]
pub mod test_support {
    //! Fakes reused by command-level tests.

    use super::*;
    use crate::local_runtime::detector::CommandOutput;
    use std::path::Path;

    /// A release client fake returning a preset info.
    pub struct FakeRelease {
        pub info: Option<ReleaseInfo>,
    }
    #[async_trait::async_trait]
    impl ReleaseClient for FakeRelease {
        async fn latest(
            &self,
            _product: ProductId,
            _force: bool,
        ) -> Result<ReleaseInfo, PackageManagementError> {
            self.info
                .clone()
                .ok_or(PackageManagementError::ReleaseCheckFailed {
                    product: "pi".into(),
                })
        }
    }

    /// A verifier that approves anything present.
    pub struct OkVerifier;
    #[async_trait::async_trait]
    impl PostInstallVerifier for OkVerifier {
        async fn verify(
            &self,
            product: ProductId,
            expected_version: semver::Version,
            staging_dir: &Path,
            _node: &Path,
        ) -> Result<VerifiedInstall, PackageManagementError> {
            let pkg = package_name(product);
            let root = staging_dir.join("node_modules").join(pkg);
            Ok(VerifiedInstall {
                product,
                version: expected_version,
                package_root: root.clone(),
                entrypoint: root.join("bin").join(bin_name(product)),
            })
        }
    }

    /// A fake installer that materializes the expected node_modules layout.
    pub struct FakeInstaller;
    #[async_trait::async_trait]
    impl PackageInstaller for FakeInstaller {
        async fn install(
            &self,
            spec: InstallSpec,
            _log: Arc<dyn OperationLogSink>,
        ) -> Result<crate::package_management::installer::InstallOutcome, PackageManagementError>
        {
            let pkg = package_name(spec.product);
            let root = spec.staging_dir.join("node_modules").join(pkg);
            tokio::fs::create_dir_all(root.join("bin")).await.ok();
            tokio::fs::write(
                root.join("bin").join(bin_name(spec.product)),
                "#!/usr/bin/env node\n",
            )
            .await
            .ok();
            Ok(crate::package_management::installer::InstallOutcome {
                staging_dir: spec.staging_dir,
            })
        }
    }

    /// A fake npm detector returning a preset toolchain.
    pub struct FakeNpmDetector {
        pub ok: bool,
    }
    #[async_trait::async_trait]
    impl NpmToolchainDetector for FakeNpmDetector {
        async fn detect(
            &self,
            node: &NodeInstallation,
        ) -> Result<NpmToolchain, PackageManagementError> {
            if self.ok {
                Ok(NpmToolchain {
                    node_executable: node.canonical_executable.clone(),
                    npm_cli_js: PathBuf::from("/fake/npm-cli.js"),
                    npm_version: "10.8.2".into(),
                    source: node.source,
                })
            } else {
                Err(PackageManagementError::NpmUnavailable)
            }
        }
    }

    #[allow(dead_code)]
    pub fn ok_command_output() -> CommandOutput {
        CommandOutput {
            exit_code: Some(0),
            stdout: "".into(),
            stderr: "".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_not_installed_with_token_offer_install() {
        let a = compute_actions(
            ProductId::Pi,
            ProductInstallState::NotInstalled,
            UpdateStatus::Available,
            true,
            true,
            false,
            false,
            None,
            None,
        );
        assert!(a.contains(&ProductAction::Install));
    }

    #[test]
    fn actions_installed_available_offer_update() {
        let a = compute_actions(
            ProductId::Pi,
            ProductInstallState::Installed,
            UpdateStatus::Available,
            true,
            true,
            false,
            false,
            None,
            None,
        );
        assert!(a.contains(&ProductAction::Update));
    }

    #[test]
    fn actions_in_flight_shows_cancel() {
        let a = compute_actions(
            ProductId::Pi,
            ProductInstallState::Installed,
            UpdateStatus::Available,
            true,
            true,
            false,
            false,
            None,
            Some((ProductId::Pi, PackageOperationStage::Installing)),
        );
        assert_eq!(a, vec![ProductAction::Scan, ProductAction::Cancel]);
    }

    #[test]
    fn actions_awaiting_restart_shows_confirm() {
        let a = compute_actions(
            ProductId::PiHub,
            ProductInstallState::Installed,
            UpdateStatus::Available,
            true,
            true,
            true,
            false,
            Some(LocalRuntimeState::RunningManaged),
            Some((
                ProductId::PiHub,
                PackageOperationStage::AwaitingRestartConfirmation,
            )),
        );
        assert_eq!(a, vec![ProductAction::Scan, ProductAction::ConfirmRestart]);
    }

    #[test]
    fn actions_busy_pihub_no_update() {
        let a = compute_actions(
            ProductId::PiHub,
            ProductInstallState::Installed,
            UpdateStatus::Available,
            true,
            true,
            false,
            false,
            Some(LocalRuntimeState::Starting),
            None,
        );
        assert!(!a.contains(&ProductAction::Update));
    }

    #[test]
    fn strengthen_pi_classifies_npm_when_package_json_present() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_dir = dir.path().join("@earendil-works/pi-coding-agent");
        let bin_dir = pkg_dir.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name":"@earendil-works/pi-coding-agent","version":"0.84.0"}"#,
        )
        .unwrap();
        let exe = bin_dir.join("pi");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        let (root, kind) = locate_pi_package(&exe);
        assert_eq!(kind, PackageKind::Npm);
        assert!(root.is_some());
    }

    #[tokio::test]
    async fn install_transaction_pi_succeeds_with_fakes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ManagedPackageStore::new(dir.path().join("packages")));
        store.load().await.unwrap();
        store.ensure_layout().await.unwrap();
        let settings = Arc::new(LocalRuntimeSettingsStore::in_memory());

        // Seed a node installation on disk so the detector finds it.
        let node = dir.path().join("node");
        std::fs::write(&node, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&node).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&node, p).unwrap();
        }
        settings
            .update(crate::local_runtime::settings::LocalRuntimeSettingsUpdate {
                node_executable: Some(node.clone()),
                ..Default::default()
            })
            .await
            .unwrap();

        // A detector fake that returns the node.
        struct NodeDetector {
            node_path: PathBuf,
        }
        #[async_trait::async_trait]
        impl InstallationDetector for NodeDetector {
            async fn detect(
                &self,
                _hints: &DetectionHints,
            ) -> Result<InstallationSet, crate::error::LocalRuntimeError> {
                let canon = std::fs::canonicalize(&self.node_path).unwrap();
                Ok(InstallationSet {
                    node: Some(NodeInstallation {
                        executable: self.node_path.clone(),
                        canonical_executable: canon,
                        version: "24.19.0".into(),
                        source: InstallationSource::Manual,
                    }),
                    pi_hub: None,
                    pi_cli: None,
                })
            }
        }

        let services = Services {
            detector: Arc::new(NodeDetector {
                node_path: node.clone(),
            }),
            runner: Arc::new(crate::local_runtime::detector::TokioCommandRunner),
            release_client: Arc::new(test_support::FakeRelease {
                info: Some(ReleaseInfo {
                    product: ProductId::Pi,
                    version: semver::Version::new(0, 84, 0),
                    node_engine: None,
                    integrity: None,
                    published_at: None,
                }),
            }),
            npm_detector: Arc::new(test_support::FakeNpmDetector { ok: true }),
            installer: Arc::new(test_support::FakeInstaller),
            verifier: Arc::new(test_support::OkVerifier),
            store: store.clone(),
            settings,
            pi_hub_port: None,
        };
        let mgr = Arc::new(PackageManagementManager::new(
            Some(services),
            Arc::new(NoopBroadcaster),
        ));

        // Mint a token via check_updates.
        mgr.check_updates(false).await.unwrap();
        let snap = mgr.snapshot().await;
        let pi = snap
            .products
            .iter()
            .find(|p| p.product == ProductId::Pi)
            .unwrap();
        let token = pi.release_token.clone().expect("token minted");

        let dto = mgr.start_install(ProductId::Pi, token).await.unwrap();
        assert_eq!(dto.stage, PackageOperationStage::Completed);
        assert_eq!(dto.target_version.as_deref(), Some("0.84.0"));

        // The managed active is now set.
        let active = store.active(ProductId::Pi).await.unwrap();
        assert_eq!(active.version, "0.84.0");
        // No operation in flight anymore.
        assert!(mgr.op.lock().await.is_none());
    }
}
