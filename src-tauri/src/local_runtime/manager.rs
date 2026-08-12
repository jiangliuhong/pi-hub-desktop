//! `LocalRuntimeManager` — the V2 orchestration core (docs/design-v2.md §4.1,
//! §12–§14).
//!
//! It is the single source of truth for the local runtime snapshot and the
//! only thing permitted to mutate it. It serializes start/stop/restart,
//! guards against stale async tasks overwriting newer state (operation
//! generation), enforces ownership before stop, and applies crash-loop
//! protection to auto-start (design-v2 §12.2, §14.2).
//!
//! On iOS the manager is constructed with no services, so every operation
//! returns `unsupported_platform` (design-v2 §16.1) while still compiling and
//! exposing the same DTO surface.

use crate::credential::{CredentialId, CredentialKind, CredentialStore};
use crate::error::LocalRuntimeError;
use crate::local_runtime::detector::{DetectionHints, InstallationDetector};
use crate::local_runtime::doctor::{DoctorContext, PiEnvironmentDoctor};
use crate::local_runtime::health::{LocalServiceProbe, ProbeResult};
use crate::local_runtime::logs::RuntimeLogBuffer;
use crate::local_runtime::model::*;
use crate::local_runtime::process::{ManagedProcess, ProcessSecret, ProcessSupervisor, StartSpec};
use crate::local_runtime::settings::{LocalRuntimeSettings, LocalRuntimeSettingsStore};
use chrono::Utc;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

/// Crash-loop protection window and threshold (design-v2 §14.2).
pub const CRASH_LOOP_WINDOW_SECS: u64 = 300;
pub const CRASH_LOOP_THRESHOLD: usize = 3;

/// Ready-detection tuning (design-v2 §12.1).
const READY_POLL_INITIAL: Duration = Duration::from_millis(200);
const READY_POLL_MAX: Duration = Duration::from_secs(1);
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// Graceful stop window before SIGKILL (design-v2 §13.1, NFR-002).
const GRACEFUL_STOP: Duration = Duration::from_secs(5);
/// A child process can exit before Next.js has finished closing its listener.
/// Keep the card in `stopping` until the service is actually unreachable.
const STOP_RELEASE_TIMEOUT: Duration = Duration::from_secs(8);
const STOP_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Doctor result freshness before a start re-runs required checks (§8.5).
const DOCTOR_FRESH_SECS: i64 = 60;

/// Broadcasts non-sensitive snapshot events to the App Shell (design-v2 §17.2).
#[async_trait::async_trait]
pub trait StatusBroadcaster: Send + Sync {
    async fn broadcast(&self, snapshot: &LocalRuntimeSnapshot);
}

/// No-op broadcaster for tests / headless construction.
pub struct NoopBroadcaster;
#[async_trait::async_trait]
impl StatusBroadcaster for NoopBroadcaster {
    async fn broadcast(&self, _snapshot: &LocalRuntimeSnapshot) {}
}

/// Tauri-backed broadcaster. Emits `local-runtime://status-changed` to the
/// trusted App Shell only (design-v2 §17.2). Never targets the Service
/// WebView, which has no capability (V2-SR-005).
pub struct TauriBroadcaster {
    handle: tauri::AppHandle,
}

impl TauriBroadcaster {
    pub fn new(handle: tauri::AppHandle) -> Self {
        TauriBroadcaster { handle }
    }
}

#[async_trait::async_trait]
impl StatusBroadcaster for TauriBroadcaster {
    async fn broadcast(&self, snapshot: &LocalRuntimeSnapshot) {
        use tauri::Emitter;
        // Payload is the snapshot itself; it carries no secrets.
        let _ = self.handle.emit(STATUS_CHANGED_EVENT, snapshot.clone());
    }
}

/// Event names (design-v2 §17.2).
pub const STATUS_CHANGED_EVENT: &str = "local-runtime://status-changed";

/// The bundled domain services. `None` on iOS → unsupported operations.
pub struct Services {
    pub detector: Arc<dyn InstallationDetector>,
    pub doctor: Arc<dyn PiEnvironmentDoctor>,
    pub probe: Arc<dyn LocalServiceProbe>,
    pub supervisor: Arc<dyn ProcessSupervisor>,
    pub settings: Arc<LocalRuntimeSettingsStore>,
    pub logs: Arc<RuntimeLogBuffer>,
    pub credentials: Arc<dyn CredentialStore>,
}

/// The local runtime manager.
pub struct LocalRuntimeManager {
    snapshot: RwLock<LocalRuntimeSnapshot>,
    process: Mutex<Option<ManagedProcess>>,
    /// Serializes start/stop/restart so duplicate clicks reuse one operation.
    op_lock: Mutex<()>,
    generation: AtomicU64,
    services: Option<Arc<Services>>,
    broadcaster: Arc<dyn StatusBroadcaster>,
    /// Ready-detection deadline (design-v2 §12.1). Injectable so tests can
    /// exercise the startup-timeout path without waiting 30s (NFR-003).
    start_timeout: Duration,
}

impl LocalRuntimeManager {
    /// Construct with explicit services (DI for tests / macOS production).
    pub fn new(services: Option<Services>, broadcaster: Arc<dyn StatusBroadcaster>) -> Self {
        Self::with_start_timeout(services, broadcaster, START_TIMEOUT)
    }

    /// Construct with a custom ready-detection deadline. Exposed so the
    /// startup-timeout path can be tested quickly (NFR-003).
    pub(crate) fn with_start_timeout(
        services: Option<Services>,
        broadcaster: Arc<dyn StatusBroadcaster>,
        start_timeout: Duration,
    ) -> Self {
        LocalRuntimeManager {
            snapshot: RwLock::new(LocalRuntimeSnapshot::default()),
            process: Mutex::new(None),
            op_lock: Mutex::new(()),
            generation: AtomicU64::new(0),
            services: services.map(Arc::new),
            broadcaster,
            start_timeout,
        }
    }

    /// Platform-default constructor: real services on macOS/Linux, `None` on
    /// iOS (design-v2 §16.1).
    pub fn platform_default(
        settings: Arc<LocalRuntimeSettingsStore>,
        credentials: Arc<dyn CredentialStore>,
        broadcaster: Arc<dyn StatusBroadcaster>,
    ) -> Self {
        #[cfg(not(mobile))]
        {
            let services = Services {
                detector: Arc::new(
                    crate::local_runtime::detector::DefaultInstallationDetector::with_default_runner(),
                ),
                doctor: Arc::new(
                    crate::local_runtime::doctor::DefaultPiEnvironmentDoctor::with_default_runner(),
                ),
                probe: Arc::new(crate::local_runtime::health::HttpServiceProbe),
                supervisor: Arc::new(crate::local_runtime::process::TokioProcessSupervisor),
                settings,
                logs: Arc::new(RuntimeLogBuffer::default()),
                credentials,
            };
            Self::new(Some(services), broadcaster)
        }
        #[cfg(mobile)]
        {
            // No services on iOS: every operation returns unsupported_platform.
            let _ = (settings, credentials);
            Self::new(None, broadcaster)
        }
    }

    fn unsupported() -> LocalRuntimeError {
        LocalRuntimeError::UnsupportedPlatform
    }

    async fn broadcast(&self, snapshot: &LocalRuntimeSnapshot) {
        self.broadcaster.broadcast(snapshot).await;
    }

    /// Current observable snapshot (cheap clone).
    pub async fn snapshot(&self) -> LocalRuntimeSnapshot {
        self.snapshot.read().await.clone()
    }

    /// Validate a user-selected Node / Pi Hub pair without persisting it
    /// (V2-FR-003). Returns the discovered facts; the caller persists only on
    /// success.
    pub async fn validate_installation(
        &self,
        input: crate::commands::local_runtime::ValidateInstallationInput,
    ) -> Result<InstallationSet, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        let hints = DetectionHints {
            persisted_node: input.node_executable,
            persisted_pi_hub_entrypoint: input.pi_hub_entrypoint,
            persisted_pi_hub_package_root: input.pi_hub_package_root,
            path_dirs: app_path_dirs(),
            home_override: None,
        };
        svc.detector.detect(&hints).await
    }

    /// Snapshot the current local runtime settings.
    pub async fn settings(&self) -> LocalRuntimeSettings {
        match &self.services {
            Some(svc) => svc.settings.get().await,
            None => LocalRuntimeSettings::default(),
        }
    }

    /// Apply a settings update (validated + canonicalized + persisted).
    pub async fn update_settings(
        &self,
        input: crate::local_runtime::settings::LocalRuntimeSettingsUpdate,
    ) -> Result<LocalRuntimeSettings, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        // V2-FR-003 / V2-SR-001: when a *complete* Node + Pi Hub pair would be
        // saved, validate it through the detector before persisting. Partial
        // selections (only one of the two) are allowed so the user can build up
        // a pair incrementally. An invalid pair is rejected without touching
        // the store.
        let current = svc.settings.get().await;
        let would_node = input
            .node_executable
            .clone()
            .or_else(|| current.node_executable.clone());
        let would_entry = input
            .pi_hub_entrypoint
            .clone()
            .or_else(|| current.pi_hub_entrypoint.clone());
        if let (Some(node), Some(entry)) = (would_node.as_ref(), would_entry.as_ref()) {
            let hints = DetectionHints {
                persisted_node: Some(node.clone()),
                persisted_pi_hub_entrypoint: Some(entry.clone()),
                persisted_pi_hub_package_root: input
                    .pi_hub_package_root
                    .clone()
                    .or_else(|| current.pi_hub_package_root.clone()),
                path_dirs: app_path_dirs(),
                home_override: None,
            };
            let validated = svc.detector.detect(&hints).await?;
            if validated.node.is_none() || validated.pi_hub.is_none() {
                return Err(LocalRuntimeError::PiHubInstallationInvalid(
                    "所选 Node.js 或 Pi Hub 入口未能通过验证（包身份/版本/构建产物）".into(),
                ));
            }
        }
        let updated = svc.settings.update(input).await?;
        // A settings change invalidates the doctor cache (design-v2 §8.5) and
        // clears crash-loop protection (§14.2 recovery).
        self.patch_snapshot(|s| s.environment = EnvironmentReport::default())
            .await;
        let _ = svc.settings.clear_auto_start_failures().await;
        Ok(updated)
    }

    /// Recent redacted log lines.
    pub async fn logs(&self, limit: Option<u32>) -> Vec<LogLine> {
        match &self.services {
            Some(svc) => svc.logs.recent(limit.map(|n| n as usize)),
            None => Vec::new(),
        }
    }

    /// Clear the in-memory log buffer.
    pub async fn clear_logs(&self) {
        if let Some(svc) = &self.services {
            svc.logs.clear();
        }
    }

    /// Handle the App `ExitRequested`/shutdown path (design-v2 §14.3).
    pub async fn on_app_exit(&self) {
        self.shutdown_if_managed().await;
    }

    /// Spawn the async app-launch initialization (design-v2 §14.1).
    pub fn spawn_initialize(self: &std::sync::Arc<Self>) {
        let mgr = self.clone();
        tokio::spawn(async move {
            mgr.initialize().await;
        });
    }

    /// Transition the runtime state, validating legality (design-v2 §4.3).
    /// Currently the manager updates state inline via `patch_snapshot`; this
    /// helper is retained for explicit single-step transitions.
    #[allow(dead_code)]
    async fn set_state(&self, next: LocalRuntimeState) -> Result<(), LocalRuntimeError> {
        let mut snap = self.snapshot.write().await;
        let cur = snap.runtime_state;
        let next = cur.transition(next).map_err(|_| {
            LocalRuntimeError::Internal(format!("illegal transition {cur:?}->{next:?}"))
        })?;
        snap.runtime_state = next;
        snap.checked_at = Some(Utc::now());
        Ok(())
    }

    /// Update the snapshot fields without violating the state machine.
    async fn patch_snapshot(
        &self,
        f: impl FnOnce(&mut LocalRuntimeSnapshot),
    ) -> LocalRuntimeSnapshot {
        let mut snap = self.snapshot.write().await;
        f(&mut snap);
        snap.checked_at = Some(Utc::now());
        let cloned = snap.clone();
        drop(snap);
        cloned
    }

    /// Detect installations + probe the port, updating the snapshot. This is
    /// the read-only refresh (design-v2 Step 2).
    pub async fn refresh(&self) -> Result<LocalRuntimeSnapshot, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        self.patch_snapshot(|s| s.runtime_state = LocalRuntimeState::Checking)
            .await;
        let settings = svc.settings.get().await;

        // Detect installations.
        let installation = svc.detector.detect(&hints_from_settings(&settings)).await?;

        let installation_state = classify_installation(&installation, &settings);

        // Probe the port.
        let probe = svc
            .probe
            .probe(settings.port, Duration::from_secs(2))
            .await?;
        let has_managed = self.process.lock().await.is_some();

        let runtime_state = match (probe, has_managed) {
            (ProbeResult::PiHub { .. }, true) => LocalRuntimeState::RunningManaged,
            (
                ProbeResult::PiHub {
                    version,
                    protocol_version,
                },
                false,
            ) => {
                // Managed handle gone but service up → external.
                let _ = (version, protocol_version);
                LocalRuntimeState::RunningExternal
            }
            (ProbeResult::OtherService, _) => LocalRuntimeState::PortConflict,
            (ProbeResult::NotListening, _) => LocalRuntimeState::Stopped,
            (ProbeResult::TimedOut, _) => LocalRuntimeState::Unknown,
        };

        let effective_url = if runtime_state.is_running() {
            Some(format!("http://127.0.0.1:{}", settings.port))
        } else {
            None
        };

        let snap = self
            .patch_snapshot(|s| {
                s.installation_state = installation_state;
                // The probe result is authoritative for the runtime state;
                // refresh may move between any of these terminal-ish states.
                s.runtime_state = runtime_state;
                s.installation = Some(installation.clone());
                s.effective_url = effective_url;
            })
            .await;
        self.broadcast(&snap).await;
        Ok(snap)
    }

    /// Run (or re-run) the environment doctor (design-v2 §8).
    pub async fn run_doctor(&self, force: bool) -> Result<EnvironmentReport, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        if !force {
            let snap = self.snapshot.read().await;
            if let Some(checked) = snap.environment.generated_at {
                if (Utc::now() - checked).num_seconds() < DOCTOR_FRESH_SECS {
                    return Ok(snap.environment.clone());
                }
            }
        }
        let set = self.detect_validated().await?;
        let (node, entry, root) = match installation_triple(&set) {
            Some(t) => t,
            None => {
                // No usable installation → blocked report.
                let report = EnvironmentReport {
                    overall: EnvironmentStatus::Blocked,
                    generated_at: Some(Utc::now()),
                    checks: vec![blocked_no_installation()],
                };
                self.patch_snapshot(|s| s.environment = report.clone())
                    .await;
                return Ok(report);
            }
        };
        let settings = svc.settings.get().await;
        let ctx = DoctorContext {
            node_executable: node,
            pi_hub_entrypoint: entry,
            pi_hub_package_root: root,
            settings: settings.clone(),
        };
        let report = svc.doctor.diagnose(&ctx).await?;
        self.patch_snapshot(|s| {
            s.environment = report.clone();
            if report.overall != EnvironmentStatus::Blocked
                && matches!(
                    s.last_error.as_ref(),
                    Some(last)
                        if last.code == crate::error::ErrorCode::PiHubDoctorBlocked
                )
            {
                s.last_error = None;
            }
        })
        .await;
        Ok(report)
    }

    /// Start the managed Pi Hub (design-v2 §12).
    pub async fn start(&self) -> Result<LocalRuntimeSnapshot, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        let _op = self.op_lock.lock().await;
        let gen = self.bump_generation();

        // 1. Probe port first.
        let settings = svc.settings.get().await;
        let probe = svc
            .probe
            .probe(settings.port, Duration::from_secs(2))
            .await?;
        match probe {
            ProbeResult::PiHub { .. } => {
                let snap = self
                    .patch_snapshot(|s| {
                        let _ = s
                            .runtime_state
                            .transition(LocalRuntimeState::RunningExternal);
                        s.runtime_state = LocalRuntimeState::RunningExternal;
                    })
                    .await;
                self.broadcast(&snap).await;
                return Ok(snap);
            }
            ProbeResult::OtherService => {
                self.fail_with(LocalRuntimeError::PortConflict {
                    port: settings.port,
                })
                .await;
                return Err(LocalRuntimeError::PortConflict {
                    port: settings.port,
                });
            }
            _ => {}
        }

        // 2. Validate installation via the detector (V2-SR-001: only a
        //    detector-verified Node + Pi Hub pair may be launched — never a
        //    raw exists()-checked path).
        let set = self.detect_validated().await?;
        let (node, entry, root) = installation_triple(&set).ok_or_else(|| {
            LocalRuntimeError::PiHubInstallationInvalid(
                "no verified Node.js + Pi Hub installation; run a scan or set valid paths".into(),
            )
        })?;

        // 3. Required doctor checks (refresh if stale).
        let blocked = self
            .required_doctor_blocked(&settings, (node.clone(), entry.clone(), root.clone()))
            .await?;
        if let Some(report) = blocked {
            let err = LocalRuntimeError::DoctorBlocked(report.overall.api_name().into());
            // The structured report was persisted by `required_doctor_blocked`.
            // Keep Doctor failures out of `recentOutput`, which is reserved for
            // redacted child-process output from a process that actually ran.
            self.fail_with(err.clone()).await;
            return Err(err);
        }

        // 4. Resolve optional password from Keychain (V2-SR-003).
        let password = self.resolve_password(&settings).await;

        // 5. Launch.
        svc.logs.clear();
        // We hold the op_lock and have confirmed the port is free + the
        // installation is verified, so this transition is ours to make; set it
        // directly rather than via the state-machine guard (which would no-op
        // on e.g. Unknown and leave the UI showing a stale state).
        self.patch_snapshot(|s| {
            s.runtime_state = LocalRuntimeState::Starting;
        })
        .await;
        let spec = StartSpec {
            node_executable: node.clone(),
            pi_hub_entrypoint: entry.clone(),
            package_root: root.clone(),
            port: settings.port,
            pi_agent_dir: settings.pi_agent_dir.clone(),
            pi_hub_password: password.map(Arc::new),
        };
        let managed = match svc.supervisor.start(spec, svc.logs.clone()).await {
            Ok(m) => m,
            Err(e) => {
                self.fail_with(e.clone()).await;
                return Err(e);
            }
        };
        let pid = managed.pid;
        let started_at = managed.started_at;
        *self.process.lock().await = Some(managed);

        // 6. Wait for ready (poll /api/client-info) — design-v2 §12.1.
        let outcome = self.wait_for_ready(gen, settings.port).await;
        match outcome {
            ReadyOutcome::Ready {
                version,
                protocol_version,
            } => {
                let summary = ManagedProcessSummary {
                    pid,
                    started_at,
                    ready_at: Some(Utc::now()),
                    node_executable: node,
                    pi_hub_entrypoint: entry,
                    port: settings.port,
                };
                let snap = self
                    .patch_snapshot(|s| {
                        s.runtime_state = LocalRuntimeState::RunningManaged;
                        s.managed_process = Some(summary);
                        s.effective_url = Some(format!("http://127.0.0.1:{}", settings.port));
                        s.last_error = None;
                        let _ = (version, protocol_version);
                    })
                    .await;
                // A successful start clears crash-loop history.
                let _ = svc.settings.clear_auto_start_failures().await;
                self.broadcast(&snap).await;
                Ok(snap)
            }
            ReadyOutcome::ExitedEarly { exit_code } => {
                self.teardown_managed().await;
                let err = LocalRuntimeError::ProcessExitedEarly { exit_code };
                // Attach the most recent (already-redacted) child output so the
                // user can see why Pi Hub refused to stay up (design-v2 §15.3 —
                // lines are redacted before entering the buffer).
                let recent = self.recent_output_for_error().await;
                self.fail_with_rich(err.clone(), recent).await;
                Err(err)
            }
            ReadyOutcome::Timeout => {
                self.teardown_managed().await;
                let err = LocalRuntimeError::ProcessStartFailed("start timed out".into());
                self.fail_with(err.clone()).await;
                Err(err)
            }
            ReadyOutcome::Superseded => {
                // A newer start/stop superseded this attempt; the current
                // snapshot already reflects the newer operation's intent.
                self.teardown_managed().await;
                Ok(self.snapshot().await)
            }
        }
    }

    /// Stop the managed Pi Hub (design-v2 §13.1). Only legal on a process this
    /// app owns (RunningManaged).
    pub async fn stop(&self) -> Result<LocalRuntimeSnapshot, LocalRuntimeError> {
        let Some(svc) = &self.services else {
            return Err(Self::unsupported());
        };
        let _op = self.op_lock.lock().await;
        self.bump_generation();

        let mut guard = self.process.lock().await;
        let Some(mut managed) = guard.take() else {
            return Err(LocalRuntimeError::ProcessNotOwned);
        };
        drop(guard);

        self.patch_snapshot(|s| {
            if s.runtime_state
                .transition(LocalRuntimeState::Stopping)
                .is_ok()
            {
                s.runtime_state = LocalRuntimeState::Stopping;
            }
        })
        .await;

        let outcome = svc.supervisor.stop(&mut managed, GRACEFUL_STOP).await;
        match outcome {
            Ok(_) => {
                let settings = svc.settings.get().await;
                // The launcher process can exit slightly before Next.js closes
                // its socket. Do not sample only once: actively wait until the
                // service is genuinely unreachable (design-v2 §13.3).
                let released = self
                    .wait_for_port_release(settings.port, STOP_RELEASE_TIMEOUT)
                    .await;
                let snap = if released {
                    self.patch_snapshot(|s| {
                        s.runtime_state = LocalRuntimeState::Stopped;
                        s.managed_process = None;
                        s.effective_url = None;
                        s.last_error = None;
                    })
                    .await
                } else {
                    // The owned process group was signalled but the endpoint is
                    // still reachable. This is a stop failure, never a start
                    // failure; keep the typed error so the UI labels it clearly.
                    self.patch_snapshot(|s| {
                        s.runtime_state = LocalRuntimeState::Failed;
                        s.managed_process = None;
                        s.last_error = Some(
                            LocalRuntimeError::PortNotReleased {
                                port: settings.port,
                            }
                            .to_dto_with_details(),
                        );
                    })
                    .await
                };
                self.broadcast(&snap).await;
                Ok(snap)
            }
            Err(e) => {
                self.fail_with(e.clone()).await;
                Err(e)
            }
        }
    }

    /// Poll the identity endpoint until nothing is listening. Transient probe
    /// errors do not count as stopped: only an explicit connection refusal does.
    async fn wait_for_port_release(&self, port: u16, timeout: Duration) -> bool {
        let Some(svc) = &self.services else {
            return false;
        };
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match svc.probe.probe(port, Duration::from_millis(500)).await {
                Ok(ProbeResult::NotListening) => return true,
                Ok(
                    ProbeResult::PiHub { .. } | ProbeResult::OtherService | ProbeResult::TimedOut,
                )
                | Err(_) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(STOP_RELEASE_POLL_INTERVAL).await;
        }
    }

    /// Restart = stop (if managed) then start (design-v2 §13.3).
    pub async fn restart(&self) -> Result<LocalRuntimeSnapshot, LocalRuntimeError> {
        let snap = self.snapshot().await;
        if snap.runtime_state == LocalRuntimeState::RunningManaged {
            self.stop().await?;
        }
        self.start().await
    }

    /// App-launch initialization: load settings, refresh, and auto-start if
    /// enabled and not suppressed (design-v2 §14.1, §14.2). Never blocks the
    /// main window; failures only update the card.
    ///
    /// NOTE: As of the manual-detection model, this is no longer invoked at
    /// app launch. Detection and start/stop are fully user-driven from the
    /// "This Mac" card (`scan_local_installations`). The method is retained
    /// because it is covered by unit tests and is the natural hook should
    /// auto-start be reintroduced under a future requirements change.
    pub async fn initialize(&self) {
        let Some(svc) = &self.services else {
            return;
        };
        if svc.settings.load().await.is_err() {
            // Non-fatal: defaults are usable.
        }
        // Refresh observable state first (card can show "checking").
        let _ = self.refresh().await;

        let settings = svc.settings.get().await;
        if !settings.auto_start_on_app_launch {
            return;
        }
        if self.auto_start_suppressed(&settings).await {
            let snap = self
                .patch_snapshot(|s| {
                    s.last_error =
                        Some(LocalRuntimeError::AutoStartSuppressed.to_dto_with_details());
                })
                .await;
            self.broadcast(&snap).await;
            return;
        }
        let snap = self.snapshot().await;
        // Already running → nothing to do.
        if snap.runtime_state.is_running() {
            return;
        }
        if snap.runtime_state == LocalRuntimeState::PortConflict {
            return;
        }
        match self.start().await {
            Ok(_) => {}
            Err(_) => {
                // Record failure for crash-loop protection (design-v2 §14.2).
                let _ = svc.settings.record_auto_start_failure(Utc::now()).await;
            }
        }
    }

    /// Whether auto-start is currently suppressed by the crash-loop window.
    pub async fn auto_start_suppressed(&self, settings: &LocalRuntimeSettings) -> bool {
        let now = Utc::now();
        let window = chrono::Duration::from_std(Duration::from_secs(CRASH_LOOP_WINDOW_SECS))
            .unwrap_or(chrono::Duration::minutes(5));
        settings
            .auto_start_failures
            .iter()
            .filter(|t| **t > now - window)
            .count()
            >= CRASH_LOOP_THRESHOLD
    }

    /// Best-effort stop of the managed process on app exit (design-v2 §14.3).
    /// External processes are never touched.
    pub async fn shutdown_if_managed(&self) {
        let settings = match &self.services {
            Some(svc) => svc.settings.get().await,
            None => return,
        };
        if !settings.stop_managed_on_app_exit {
            return;
        }
        let snap = self.snapshot().await;
        if snap.runtime_state != LocalRuntimeState::RunningManaged {
            return;
        }
        // Bound shutdown so a hung child can't block exit indefinitely.
        let _ = tokio::time::timeout(Duration::from_secs(8), self.stop()).await;
    }

    // ---- internals ----

    fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Re-run the detector with the persisted paths as hints and return the
    /// *validated* installation set. This is the single chokepoint that turns
    /// user-supplied paths into verified absolute binaries (V2-SR-001, design-v2
    /// §6.4/§16): no caller may launch from raw `exists()`-checked paths.
    async fn detect_validated(&self) -> Result<InstallationSet, LocalRuntimeError> {
        let svc = self
            .services
            .as_ref()
            .ok_or(LocalRuntimeError::UnsupportedPlatform)?;
        let settings = svc.settings.get().await;
        svc.detector.detect(&hints_from_settings(&settings)).await
    }

    async fn resolve_password(&self, settings: &LocalRuntimeSettings) -> Option<ProcessSecret> {
        let svc = self.services.as_ref()?;
        let id = settings.pi_hub_credential_id.as_ref()?;
        let cred_id = CredentialId(id.clone());
        let value = svc
            .credentials
            .get(&cred_id, CredentialKind::PiHubPassword)
            .await
            .ok()?;
        Some(ProcessSecret::new(value.into_secret()))
    }

    async fn required_doctor_blocked(
        &self,
        settings: &LocalRuntimeSettings,
        triple: (PathBuf, PathBuf, PathBuf),
    ) -> Result<Option<EnvironmentReport>, LocalRuntimeError> {
        let svc = self.services.as_ref().expect("services present");
        let snap = self.snapshot.read().await;
        let fresh = snap
            .environment
            .generated_at
            .map(|t| (Utc::now() - t).num_seconds() < DOCTOR_FRESH_SECS)
            .unwrap_or(false);
        drop(snap);
        let report = if fresh {
            self.snapshot.read().await.environment.clone()
        } else {
            let ctx = DoctorContext {
                node_executable: triple.0,
                pi_hub_entrypoint: triple.1,
                pi_hub_package_root: triple.2,
                settings: settings.clone(),
            };
            let r = svc.doctor.diagnose(&ctx).await?;
            // Persist the fresh report so the card's environment row and the
            // environment page reflect why start was blocked (otherwise the UI
            // keeps showing "未检查" with no clue about the failure).
            self.patch_snapshot(|s| s.environment = r.clone()).await;
            r
        };
        if report.overall == EnvironmentStatus::Blocked {
            Ok(Some(report))
        } else {
            Ok(None)
        }
    }

    async fn wait_for_ready(&self, gen: u64, port: u16) -> ReadyOutcome {
        let Some(svc) = &self.services else {
            return ReadyOutcome::Timeout;
        };
        let mut delay = READY_POLL_INITIAL;
        let deadline = tokio::time::Instant::now() + self.start_timeout;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return ReadyOutcome::Timeout;
            }
            // If our child died, fail fast — and capture why.
            {
                let mut guard = self.process.lock().await;
                if let Some(m) = guard.as_mut() {
                    if let Some(status) = m.exit_status_if_finished() {
                        return ReadyOutcome::ExitedEarly {
                            exit_code: status.code(),
                        };
                    }
                }
            }
            match svc.probe.probe(port, Duration::from_millis(800)).await {
                Ok(ProbeResult::PiHub {
                    version,
                    protocol_version,
                }) => {
                    if self.current_generation() != gen {
                        return ReadyOutcome::Superseded;
                    }
                    return ReadyOutcome::Ready {
                        version,
                        protocol_version,
                    };
                }
                Ok(ProbeResult::OtherService) => {
                    // The owned child may bind the socket before its identity
                    // route is ready (or temporarily return a 5xx). Do not call
                    // that an external port conflict while our child is still
                    // alive. A real bind collision makes the child exit and is
                    // reported by the next `exit_status_if_finished` check.
                }
                _ => {}
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(READY_POLL_MAX);
        }
    }

    async fn teardown_managed(&self) {
        let mut guard = self.process.lock().await;
        if let Some(mut managed) = guard.take() {
            if let Some(svc) = &self.services {
                let _ = svc.supervisor.stop(&mut managed, GRACEFUL_STOP).await;
            }
        }
    }

    async fn fail_with(&self, err: LocalRuntimeError) {
        self.fail_with_rich(err, None).await;
    }

    /// Like `fail_with`, but optionally attaches already-redacted recent child
    /// output to the error DTO's `details["recentOutput"]` so the UI can show
    /// *why* Pi Hub failed (e.g. exited early). The output comes from
    /// `RuntimeLogBuffer`, which redacts secrets on ingest (design-v2 §15.3).
    async fn fail_with_rich(&self, err: LocalRuntimeError, recent_output: Option<String>) {
        let snap = self
            .patch_snapshot(|s| {
                s.runtime_state = LocalRuntimeState::Failed;
                let mut dto = err.to_dto_with_details();
                if let Some(out) = &recent_output {
                    if !out.is_empty() {
                        dto.details.insert("recentOutput".into(), out.clone());
                    }
                }
                s.last_error = Some(dto);
                s.managed_process = None;
            })
            .await;
        self.broadcast(&snap).await;
    }

    /// Collect the most recent child output lines (stdout + stderr, newest
    /// last) as a single string for attachment to a start-failure error.
    async fn recent_output_for_error(&self) -> Option<String> {
        let Some(svc) = &self.services else {
            return None;
        };
        let lines = svc.logs.recent(Some(20));
        if lines.is_empty() {
            return None;
        }
        Some(
            lines
                .iter()
                .map(|l| {
                    let stream = match l.stream {
                        LogStream::Stdout => "stdout",
                        LogStream::Stderr => "stderr",
                    };
                    format!("[{stream}] {}", l.text)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

enum ReadyOutcome {
    Ready {
        version: String,
        protocol_version: u32,
    },
    ExitedEarly {
        exit_code: Option<i32>,
    },
    Timeout,
    Superseded,
}

// ---- free helpers ----

/// Classify the installation set into a coarse state (design-v2 §7.1).
fn classify_installation(
    installation: &InstallationSet,
    settings: &LocalRuntimeSettings,
) -> InstallationState {
    match (&installation.node, &installation.pi_hub) {
        (Some(_), Some(_)) => InstallationState::Ready,
        (Some(_), None) => {
            // Node found but no Pi Hub — invalid unless user is mid-selection.
            if settings.pi_hub_entrypoint.is_some() {
                InstallationState::Invalid
            } else {
                InstallationState::NotFound
            }
        }
        (None, _) => InstallationState::NotFound,
    }
}

/// Build detection hints from the persisted settings (persisted paths act
/// as the highest-priority candidates; the detector still validates them).
fn hints_from_settings(settings: &LocalRuntimeSettings) -> DetectionHints {
    DetectionHints {
        persisted_node: settings.node_executable.clone(),
        persisted_pi_hub_entrypoint: settings.pi_hub_entrypoint.clone(),
        persisted_pi_hub_package_root: settings.pi_hub_package_root.clone(),
        path_dirs: app_path_dirs(),
        home_override: None,
    }
}

/// Extract the validated (canonical node, entrypoint, package root) triple
/// from a detector-returned set. Only present when the detector confirmed a
/// usable Node + Pi Hub pair (so callers can never receive an unverified
/// arbitrary path here).
fn installation_triple(set: &InstallationSet) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let node = set.node.as_ref()?;
    let pi_hub = set.pi_hub.as_ref()?;
    Some((
        node.canonical_executable.clone(),
        pi_hub.entrypoint.clone(),
        pi_hub.package_root.clone(),
    ))
}

fn blocked_no_installation() -> CheckResult {
    CheckResult {
        id: "DEP-INSTALL-000".into(),
        category: CheckCategory::Runtime,
        severity: CheckSeverity::Required,
        status: CheckStatus::Fail,
        code: Some("no_usable_installation".into()),
        message: Some("未发现可用的 Node.js + Pi Hub 安装组合。".into()),
        remediation: Some("请在设置中手动选择 Node.js 与 Pi Hub 入口路径。".into()),
        details: BTreeMap::new(),
    }
}

/// Directories derived from the current process PATH (Finder-launched apps do
/// not inherit the interactive shell PATH — design-v2 §6.1).
fn app_path_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            out.push(dir);
        }
    }
    out
}

#[cfg(test)]
pub mod test_support {
    //! Reusable fakes for manager-level tests.

    use super::*;
    use crate::local_runtime::detector::CommandOutput;
    use crate::local_runtime::process::StopOutcome;
    use async_trait::async_trait;

    /// A configurable probe fake backed by a shared mutable cell, so a test
    /// can flip the result between phases (e.g. NotListening before start,
    /// PiHub after, NotListening again after stop).
    /// A configurable probe fake. With no `starts` tracker it always returns
    /// `base`. When paired with a `FakeSupervisor` via a shared counter, it
    /// returns `ready` once a managed process is alive and `base` again once
    /// it is stopped — so start/stop flows can be exercised without a real
    /// Pi Hub.
    pub struct FakeProbe {
        pub base: ProbeResult,
        pub ready: ProbeResult,
        pub starts: Option<Arc<std::sync::atomic::AtomicU32>>,
    }
    impl FakeProbe {
        pub fn fixed(result: ProbeResult) -> Self {
            FakeProbe {
                base: result.clone(),
                ready: result,
                starts: None,
            }
        }
    }
    #[async_trait]
    impl LocalServiceProbe for FakeProbe {
        async fn probe(
            &self,
            _port: u16,
            _timeout: Duration,
        ) -> Result<ProbeResult, LocalRuntimeError> {
            if let Some(starts) = &self.starts {
                if starts.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                    return Ok(self.ready.clone());
                }
            }
            Ok(self.base.clone())
        }
    }

    /// A detector fake. When both `persisted_node` and `persisted_pi_hub_entrypoint`
    /// are present in the hints, it echoes them as a *validated* pair (mirrors
    /// the real detector's contract after a successful validation), so the
    /// manager's start path can exercise the full flow without a real Node/Pi
    /// Hub on the host. Otherwise it returns its preset `set`.
    pub struct FakeDetector {
        pub set: InstallationSet,
    }
    #[async_trait]
    impl InstallationDetector for FakeDetector {
        async fn detect(
            &self,
            hints: &DetectionHints,
        ) -> Result<InstallationSet, LocalRuntimeError> {
            if let (Some(node), Some(entry)) =
                (&hints.persisted_node, &hints.persisted_pi_hub_entrypoint)
            {
                return Ok(InstallationSet {
                    node: Some(NodeInstallation {
                        executable: node.clone(),
                        canonical_executable: node.clone(),
                        version: "24.19.0".into(),
                        source: InstallationSource::Manual,
                    }),
                    pi_hub: Some(PiHubInstallation {
                        package_root: hints
                            .persisted_pi_hub_package_root
                            .clone()
                            .or_else(|| entry.parent().map(PathBuf::from))
                            .unwrap_or_else(|| PathBuf::from(".")),
                        entrypoint: entry.clone(),
                        version: "0.0.42".into(),
                        node_requirement: ">=22.19.0".into(),
                        source: InstallationSource::Manual,
                    }),
                    pi_cli: None,
                });
            }
            Ok(self.set.clone())
        }
    }

    /// A detector that always reports no installation, used to assert the
    /// manager rejects unverified paths.
    pub struct EmptyDetector;
    #[async_trait]
    impl InstallationDetector for EmptyDetector {
        async fn detect(
            &self,
            _hints: &DetectionHints,
        ) -> Result<InstallationSet, LocalRuntimeError> {
            Ok(InstallationSet::default())
        }
    }

    /// A doctor fake returning a preset report.
    pub struct FakeDoctor {
        pub report: EnvironmentReport,
    }
    #[async_trait]
    impl PiEnvironmentDoctor for FakeDoctor {
        async fn diagnose(
            &self,
            _ctx: &DoctorContext,
        ) -> Result<EnvironmentReport, LocalRuntimeError> {
            Ok(self.report.clone())
        }
    }

    /// A supervisor fake that tracks start/stop without depending on a real
    /// Pi Hub. It spawns a real `sleep` child purely so `ManagedProcess` has
    /// a genuine `Child` handle. It shares a `starts` counter with `FakeProbe`
    /// so the probe can report readiness once a child exists.
    pub struct FakeSupervisor {
        pub starts: Arc<std::sync::atomic::AtomicU32>,
        pub stops: Arc<std::sync::atomic::AtomicU32>,
        /// When true, the spawned child exits immediately (exercises the
        /// early-exit path in `wait_for_ready`).
        pub exit_fast: bool,
        /// Optional stderr lines pushed to the log buffer on start (simulates a
        /// real child that prints an error before exiting).
        pub stderr_lines: Vec<String>,
    }
    impl Default for FakeSupervisor {
        fn default() -> Self {
            FakeSupervisor::new()
        }
    }
    impl FakeSupervisor {
        pub fn new() -> Self {
            FakeSupervisor {
                starts: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                stops: Arc::new(std::sync::atomic::AtomicU32::new(0)),
                exit_fast: false,
                stderr_lines: Vec::new(),
            }
        }
        pub fn new_exiting() -> Self {
            FakeSupervisor {
                exit_fast: true,
                ..FakeSupervisor::new()
            }
        }
    }
    #[async_trait]
    impl ProcessSupervisor for FakeSupervisor {
        async fn start(
            &self,
            _spec: StartSpec,
            logs: Arc<RuntimeLogBuffer>,
        ) -> Result<ManagedProcess, LocalRuntimeError> {
            self.starts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Simulate a child that emits stderr before the manager polls it.
            for line in &self.stderr_lines {
                logs.push_raw(LogStream::Stderr, line);
            }
            #[cfg(unix)]
            {
                let mut cmd = tokio::process::Command::new("sleep");
                cmd.arg(if self.exit_fast { "0" } else { "60" });
                cmd.process_group(0);
                cmd.stdin(std::process::Stdio::null());
                cmd.stdout(std::process::Stdio::null());
                cmd.stderr(std::process::Stdio::null());
                let child = cmd.spawn().map_err(|e| {
                    LocalRuntimeError::ProcessStartFailed(format!("fake spawn: {e}"))
                })?;
                let pid = child.id().unwrap_or(0);
                Ok(ManagedProcess {
                    pid,
                    started_at: chrono::Utc::now(),
                    child: Some(child),
                    readers: Vec::new(),
                })
            }
            #[cfg(not(unix))]
            {
                Err(LocalRuntimeError::ProcessStartFailed(
                    "fake supervisor needs unix".into(),
                ))
            }
        }
        async fn stop(
            &self,
            process: &mut ManagedProcess,
            _graceful_timeout: Duration,
        ) -> Result<StopOutcome, LocalRuntimeError> {
            // Decrement the live-starts counter so the paired probe reports
            // NotListening again (mirrors a real port release).
            let _ = self.starts.fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |v| {
                    if v > 0 {
                        Some(v - 1)
                    } else {
                        None
                    }
                },
            );
            self.stops
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Actually reap the fake child so no zombie lingers.
            if let Some(child) = process.child.as_mut() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            Ok(StopOutcome::Graceful)
        }
    }

    #[allow(dead_code)]
    pub fn ready_report() -> EnvironmentReport {
        EnvironmentReport {
            overall: EnvironmentStatus::Ready,
            generated_at: Some(Utc::now()),
            checks: vec![],
        }
    }

    #[allow(dead_code)]
    pub fn command_output_ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: "".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::in_memory::InMemoryCredentialStore;
    use crate::error::ErrorCode;
    use crate::local_runtime::settings::LocalRuntimeSettingsStore;
    use test_support::*;

    struct TransientIdentityProbe {
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LocalServiceProbe for TransientIdentityProbe {
        async fn probe(
            &self,
            _port: u16,
            _timeout: Duration,
        ) -> Result<ProbeResult, LocalRuntimeError> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(match call {
                0 => ProbeResult::NotListening,
                1 => ProbeResult::OtherService,
                _ => ProbeResult::PiHub {
                    version: "0.0.42".into(),
                    protocol_version: 1,
                },
            })
        }
    }

    /// Start identifies immediately, while stop observes the Pi Hub listener
    /// for two more polls before it is finally released.
    struct DelayedStopReleaseProbe {
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl LocalServiceProbe for DelayedStopReleaseProbe {
        async fn probe(
            &self,
            _port: u16,
            _timeout: Duration,
        ) -> Result<ProbeResult, LocalRuntimeError> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(match call {
                0 => ProbeResult::NotListening,
                1..=3 => ProbeResult::PiHub {
                    version: "0.0.42".into(),
                    protocol_version: 1,
                },
                _ => ProbeResult::NotListening,
            })
        }
    }

    fn blocked_report() -> EnvironmentReport {
        EnvironmentReport {
            overall: EnvironmentStatus::Blocked,
            generated_at: Some(Utc::now()),
            checks: vec![CheckResult {
                id: "DEP-NODE-001".into(),
                category: CheckCategory::Runtime,
                severity: CheckSeverity::Required,
                status: CheckStatus::Fail,
                code: None,
                message: None,
                remediation: None,
                details: BTreeMap::new(),
            }],
        }
    }

    fn manager_with(
        probe: Arc<dyn LocalServiceProbe>,
        doctor: Arc<dyn PiEnvironmentDoctor>,
    ) -> LocalRuntimeManager {
        manager_with_supervisor(probe, doctor, Arc::new(FakeSupervisor::new()))
    }

    fn manager_with_supervisor(
        probe: Arc<dyn LocalServiceProbe>,
        doctor: Arc<dyn PiEnvironmentDoctor>,
        supervisor: Arc<FakeSupervisor>,
    ) -> LocalRuntimeManager {
        manager_with_detector(
            probe,
            doctor,
            supervisor,
            Arc::new(FakeDetector {
                set: InstallationSet::default(),
            }),
        )
    }

    fn manager_with_detector(
        probe: Arc<dyn LocalServiceProbe>,
        doctor: Arc<dyn PiEnvironmentDoctor>,
        supervisor: Arc<FakeSupervisor>,
        detector: Arc<dyn InstallationDetector>,
    ) -> LocalRuntimeManager {
        let settings = Arc::new(LocalRuntimeSettingsStore::in_memory());
        let services = Services {
            detector,
            doctor,
            probe,
            supervisor,
            settings,
            logs: Arc::new(RuntimeLogBuffer::default()),
            credentials: Arc::new(InMemoryCredentialStore::new()),
        };
        LocalRuntimeManager::new(Some(services), Arc::new(NoopBroadcaster))
    }

    /// Like `manager_with_detector` but with a custom start deadline (for the
    /// startup-timeout path).
    fn manager_with_start_timeout(
        probe: Arc<dyn LocalServiceProbe>,
        doctor: Arc<dyn PiEnvironmentDoctor>,
        supervisor: Arc<FakeSupervisor>,
        start_timeout: Duration,
    ) -> LocalRuntimeManager {
        let settings = Arc::new(LocalRuntimeSettingsStore::in_memory());
        let services = Services {
            detector: Arc::new(FakeDetector {
                set: InstallationSet::default(),
            }),
            doctor,
            probe,
            supervisor,
            settings,
            logs: Arc::new(RuntimeLogBuffer::default()),
            credentials: Arc::new(InMemoryCredentialStore::new()),
        };
        LocalRuntimeManager::with_start_timeout(
            Some(services),
            Arc::new(NoopBroadcaster),
            start_timeout,
        )
    }

    fn probe(initial: ProbeResult) -> FakeProbe {
        FakeProbe::fixed(initial)
    }

    /// Build a probe that reports `ready` once the paired supervisor has a
    /// live child, else `base`.
    fn paired_probe(sup: &FakeSupervisor, base: ProbeResult, ready: ProbeResult) -> FakeProbe {
        FakeProbe {
            base,
            ready,
            starts: Some(sup.starts.clone()),
        }
    }

    /// Persist a fake installation so `start` can resolve the triple.
    async fn persist_fake_installation(mgr: &LocalRuntimeManager) {
        let exe = std::env::current_exe().unwrap();
        let svc = mgr.services.as_ref().unwrap();
        svc.settings
            .update(crate::local_runtime::settings::LocalRuntimeSettingsUpdate {
                node_executable: Some(exe.clone()),
                pi_hub_entrypoint: Some(exe.clone()),
                pi_hub_package_root: Some(
                    exe.parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .to_path_buf(),
                ),
                ..Default::default()
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn refresh_reports_stopped_when_port_closed() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        let snap = mgr.refresh().await.unwrap();
        assert_eq!(snap.runtime_state, LocalRuntimeState::Stopped);
    }

    #[tokio::test]
    async fn refresh_reports_running_external_without_handle() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::PiHub {
                version: "0.0.1".into(),
                protocol_version: 1,
            })),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        let snap = mgr.refresh().await.unwrap();
        assert_eq!(snap.runtime_state, LocalRuntimeState::RunningExternal);
        assert!(snap.effective_url.unwrap().contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn refresh_reports_port_conflict() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::OtherService)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        let snap = mgr.refresh().await.unwrap();
        assert_eq!(snap.runtime_state, LocalRuntimeState::PortConflict);
    }

    #[tokio::test]
    async fn start_rejects_unverified_installation() {
        // C1: with no persisted paths the detector returns no pair, so start
        // must refuse to launch anything (no exists()-only shortcut).
        let supervisor = Arc::new(FakeSupervisor::new());
        let p = paired_probe(
            &supervisor,
            ProbeResult::NotListening,
            ProbeResult::PiHub {
                version: "0.0.42".into(),
                protocol_version: 1,
            },
        );
        let mgr = manager_with_supervisor(
            Arc::new(p),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        // No persist_fake_installation → detector reports no installation.
        let res = mgr.start().await;
        assert!(matches!(
            res,
            Err(LocalRuntimeError::PiHubInstallationInvalid(_))
        ));
        // The supervisor must never have been asked to spawn.
        assert_eq!(
            supervisor.starts.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[tokio::test]
    async fn update_settings_rejects_invalid_pair() {
        // C2: a complete Node + Pi Hub pair that the detector cannot validate
        // must be rejected *before* it is persisted.
        let supervisor = Arc::new(FakeSupervisor::new());
        let p = paired_probe(
            &supervisor,
            ProbeResult::NotListening,
            ProbeResult::NotListening,
        );
        let mgr = manager_with_detector(
            Arc::new(p),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
            Arc::new(EmptyDetector),
        );
        let res = mgr
            .update_settings(crate::local_runtime::settings::LocalRuntimeSettingsUpdate {
                node_executable: Some(PathBuf::from("/bin/echo")),
                pi_hub_entrypoint: Some(PathBuf::from("/etc/passwd")),
                ..Default::default()
            })
            .await;
        assert!(matches!(
            res,
            Err(LocalRuntimeError::PiHubInstallationInvalid(_))
        ));
        // Nothing was persisted.
        let svc = mgr.services.as_ref().unwrap();
        assert!(svc.settings.get().await.node_executable.is_none());
        assert!(svc.settings.get().await.pi_hub_entrypoint.is_none());
    }

    #[tokio::test]
    async fn update_settings_allows_partial_selection() {
        // Saving only the Node path (no Pi Hub yet) is allowed — the pair is
        // validated only once both halves are present.
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        let updated = mgr
            .update_settings(crate::local_runtime::settings::LocalRuntimeSettingsUpdate {
                node_executable: Some(PathBuf::from("/usr/local/bin/node")),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            updated.node_executable.as_deref(),
            Some(std::path::Path::new("/usr/local/bin/node"))
        );
    }

    #[tokio::test]
    async fn update_settings_invalidates_doctor_cache() {
        // M1: changing install paths must drop a cached doctor report so a
        // subsequent non-forced run re-checks against the new paths.
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        // Seed a cached environment report.
        mgr.patch_snapshot(|s| {
            s.environment = EnvironmentReport {
                overall: EnvironmentStatus::Blocked,
                generated_at: Some(Utc::now()),
                checks: vec![],
            };
        })
        .await;
        assert_eq!(
            mgr.snapshot().await.environment.overall,
            EnvironmentStatus::Blocked
        );
        // A port-only change still invalidates the cache.
        mgr.update_settings(crate::local_runtime::settings::LocalRuntimeSettingsUpdate {
            port: Some(30200),
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(
            mgr.snapshot().await.environment.overall,
            EnvironmentStatus::Unknown
        );
    }

    #[tokio::test]
    async fn start_blocked_when_doctor_blocked() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: blocked_report(),
            }),
        );
        persist_fake_installation(&mgr).await;
        let res = mgr.start().await;
        assert!(matches!(res, Err(LocalRuntimeError::DoctorBlocked(_))));
        let snap = mgr.snapshot().await;
        assert_eq!(snap.runtime_state, LocalRuntimeState::Failed);
        assert_eq!(snap.environment.overall, EnvironmentStatus::Blocked);
        assert_eq!(snap.environment.checks.len(), 1);
        assert_eq!(snap.environment.checks[0].id, "DEP-NODE-001");
        let last = snap
            .last_error
            .expect("last_error set when start is blocked");
        assert_eq!(last.code, ErrorCode::PiHubDoctorBlocked);
        assert!(!last.details.contains_key("recentOutput"));
    }

    #[tokio::test]
    async fn successful_doctor_recheck_clears_stale_blocking_error() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        persist_fake_installation(&mgr).await;
        mgr.patch_snapshot(|s| {
            s.environment = blocked_report();
            s.last_error =
                Some(LocalRuntimeError::DoctorBlocked("blocked".into()).to_dto_with_details());
        })
        .await;

        let report = mgr.run_doctor(true).await.expect("recheck succeeds");
        assert_eq!(report.overall, EnvironmentStatus::Ready);
        let snap = mgr.snapshot().await;
        assert_eq!(snap.environment.overall, EnvironmentStatus::Ready);
        assert!(snap.last_error.is_none());
    }

    #[tokio::test]
    async fn start_succeeds_when_port_becomes_ready() {
        let supervisor = Arc::new(FakeSupervisor::new());
        let p = paired_probe(
            &supervisor,
            ProbeResult::NotListening,
            ProbeResult::PiHub {
                version: "0.0.42".into(),
                protocol_version: 1,
            },
        );
        let mgr = manager_with_supervisor(
            Arc::new(p),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        persist_fake_installation(&mgr).await;

        let snap = mgr.start().await.expect("start succeeds");
        assert_eq!(snap.runtime_state, LocalRuntimeState::RunningManaged);
        assert_eq!(
            supervisor.starts.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert!(snap.managed_process.is_some());

        // A successful start clears crash-loop history.
        let s = mgr.services.as_ref().unwrap().settings.get().await;
        assert!(s.auto_start_failures.is_empty());
    }

    #[tokio::test]
    async fn stop_releases_managed_process() {
        let supervisor = Arc::new(FakeSupervisor::new());
        let p = paired_probe(
            &supervisor,
            ProbeResult::NotListening,
            ProbeResult::PiHub {
                version: "0.0.42".into(),
                protocol_version: 1,
            },
        );
        let mgr = manager_with_supervisor(
            Arc::new(p),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        persist_fake_installation(&mgr).await;
        mgr.start().await.unwrap();
        assert_eq!(
            supervisor.starts.load(std::sync::atomic::Ordering::Relaxed),
            1
        );

        let snap = mgr.stop().await.expect("stop succeeds");
        assert_eq!(snap.runtime_state, LocalRuntimeState::Stopped);
        assert!(snap.managed_process.is_none());
        assert_eq!(
            supervisor.stops.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn stop_polls_until_listener_is_really_released() {
        let supervisor = Arc::new(FakeSupervisor::new());
        let probe = Arc::new(DelayedStopReleaseProbe {
            calls: std::sync::atomic::AtomicU32::new(0),
        });
        let mgr = manager_with_supervisor(
            probe.clone(),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor,
        );
        persist_fake_installation(&mgr).await;
        mgr.start().await.expect("start succeeds");

        let snap = mgr
            .stop()
            .await
            .expect("stop succeeds after delayed release");
        assert_eq!(snap.runtime_state, LocalRuntimeState::Stopped);
        assert!(snap.effective_url.is_none());
        assert!(snap.last_error.is_none());
        // preflight + ready + at least three stop checks
        assert!(probe.calls.load(Ordering::Relaxed) >= 5);
    }

    #[tokio::test]
    async fn start_reports_exited_early_when_child_dies() {
        // The child (`sleep 0`) exits immediately; the probe never reports
        // ready, so wait_for_ready must detect the dead handle and report
        // ProcessExitedEarly with the real exit code, then tear it down.
        let supervisor = Arc::new(FakeSupervisor::new_exiting());
        let mgr = manager_with_supervisor(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        persist_fake_installation(&mgr).await;

        let res = mgr.start().await;
        // `sleep 0` exits with code 0 — we must capture it (not None).
        assert!(matches!(
            res,
            Err(LocalRuntimeError::ProcessExitedEarly { exit_code: Some(0) })
        ));
        let snap = mgr.snapshot().await;
        assert_eq!(snap.runtime_state, LocalRuntimeState::Failed);
        assert!(snap.managed_process.is_none());
        // The persisted error must carry the exit code + a recentOutput hint.
        let last = snap.last_error.expect("last_error set on failure");
        assert_eq!(last.details.get("exitCode").map(String::as_str), Some("0"));
    }

    #[tokio::test]
    async fn start_failure_attaches_recent_output() {
        // When Pi Hub exits early, the persisted error should carry the most
        // recent (redacted) child output so the user can see *why* it died.
        let supervisor = Arc::new(FakeSupervisor {
            stderr_lines: vec![
                "Error: Cannot find module '@jarome/pi-hub'".into(),
                "    at Function.run (node:internal/modules/cjs/loader:nnn)".into(),
            ],
            ..FakeSupervisor::new_exiting()
        });
        let mgr = manager_with_supervisor(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        persist_fake_installation(&mgr).await;

        let res = mgr.start().await;
        assert!(matches!(
            res,
            Err(LocalRuntimeError::ProcessExitedEarly { exit_code: Some(0) })
        ));
        let snap = mgr.snapshot().await;
        let last = snap.last_error.expect("last_error set");
        let recent = last
            .details
            .get("recentOutput")
            .expect("recentOutput attached");
        assert!(recent.contains("Cannot find module"));
        assert!(recent.contains("[stderr]"));
    }

    #[tokio::test]
    async fn start_retries_transient_unidentified_response() {
        // A managed Next.js child can bind before /api/client-info is ready.
        // One unidentified/5xx response must not be called an external port
        // conflict; keep polling until the owned service identifies itself.
        let supervisor = Arc::new(FakeSupervisor::new());
        let mgr = manager_with_supervisor(
            Arc::new(TransientIdentityProbe {
                calls: std::sync::atomic::AtomicU32::new(0),
            }),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
        );
        persist_fake_installation(&mgr).await;

        let snap = mgr
            .start()
            .await
            .expect("transient identity failure recovers");
        assert_eq!(snap.runtime_state, LocalRuntimeState::RunningManaged);
        assert_eq!(supervisor.stops.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn start_reports_timeout_when_never_becomes_ready() {
        // Port never comes up and the child stays alive → bounded timeout fires.
        let supervisor = Arc::new(FakeSupervisor::new());
        let p = paired_probe(
            &supervisor,
            ProbeResult::NotListening,
            ProbeResult::NotListening,
        );
        let mgr = manager_with_start_timeout(
            Arc::new(p),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
            supervisor.clone(),
            Duration::from_millis(150),
        );
        persist_fake_installation(&mgr).await;

        let res = mgr.start().await;
        assert!(matches!(res, Err(LocalRuntimeError::ProcessStartFailed(_))));
        let snap = mgr.snapshot().await;
        assert_eq!(snap.runtime_state, LocalRuntimeState::Failed);
    }

    #[tokio::test]
    async fn stop_refused_when_not_owned() {
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::PiHub {
                version: "0.0.1".into(),
                protocol_version: 1,
            })),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        mgr.refresh().await.unwrap(); // RunningExternal, no handle.
        let res = mgr.stop().await;
        assert!(matches!(res, Err(LocalRuntimeError::ProcessNotOwned)));
    }

    #[tokio::test]
    async fn auto_start_suppressed_after_threshold_failures() {
        let settings = LocalRuntimeSettings {
            auto_start_failures: vec![Utc::now(), Utc::now(), Utc::now()],
            ..LocalRuntimeSettings::default()
        };
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        assert!(mgr.auto_start_suppressed(&settings).await);
    }

    #[tokio::test]
    async fn auto_start_not_suppressed_with_old_failures() {
        let old = Utc::now() - chrono::Duration::minutes(10);
        let settings = LocalRuntimeSettings {
            auto_start_failures: vec![old, old, old],
            ..LocalRuntimeSettings::default()
        };
        let mgr = manager_with(
            Arc::new(probe(ProbeResult::NotListening)),
            Arc::new(FakeDoctor {
                report: ready_report(),
            }),
        );
        assert!(!mgr.auto_start_suppressed(&settings).await);
    }
}
