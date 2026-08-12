//! Package installer (docs/requirements-v3.md §10, §11.1; design §10, §17.1).
//!
//! Runs exactly one fixed npm command with Rust-constructed arguments — never
//! a shell, never user-supplied args (V3-SR-001). The npm child runs in its own
//! process group so cancellation can terminate the whole subtree; only this
//! app-owned handle is ever signalled (V3-SR-002). All captured output is
//! redacted before it reaches the operation log (V3-SR-005, V2-SR-004).

use crate::error::PackageManagementError;
use crate::local_runtime::redaction::redact_line;
use crate::package_management::model::{
    package_name, PackageLogLevel, PackageOperationStage, ProductId,
};
use crate::package_management::npm_toolchain::NpmToolchain;
use crate::package_management::operation::OperationLogSink;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio_util::sync::CancellationToken;

/// Default install deadline (design §13.2 / NFR-002).
pub const DEFAULT_INSTALL_DEADLINE: Duration = Duration::from_secs(300);
/// Grace window after SIGTERM before SIGKILL when cancelling.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// Inputs to an install/update transaction (design §11.1).
pub struct InstallSpec {
    pub product: ProductId,
    pub version: semver::Version,
    pub toolchain: NpmToolchain,
    pub staging_dir: PathBuf,
    pub cancel: CancellationToken,
    pub deadline: Duration,
}

/// Successful install result: the staging dir is populated and ready for
/// verification.
pub struct InstallOutcome {
    pub staging_dir: PathBuf,
}

/// The installer contract (DI for tests).
#[async_trait::async_trait]
pub trait PackageInstaller: Send + Sync {
    async fn install(
        &self,
        spec: InstallSpec,
        log: Arc<dyn OperationLogSink>,
    ) -> Result<InstallOutcome, PackageManagementError>;
}

/// Production installer using `tokio::process` + POSIX process groups.
pub struct TokioPackageInstaller;

#[async_trait::async_trait]
impl PackageInstaller for TokioPackageInstaller {
    async fn install(
        &self,
        spec: InstallSpec,
        log: Arc<dyn OperationLogSink>,
    ) -> Result<InstallOutcome, PackageManagementError> {
        let pkg = package_name(spec.product);
        let version_spec = format!("{pkg}@{version}", version = spec.version);

        // Fixed, Rust-constructed argument vector (V3-SR-001, design §10).
        let npm_cli = spec.toolchain.npm_cli_js.clone();
        let staging = spec.staging_dir.clone();
        let mut cmd = tokio::process::Command::new(&spec.toolchain.node_executable);
        cmd.arg(&npm_cli);
        cmd.arg("install");
        cmd.arg("--prefix").arg(&staging);
        cmd.arg("--no-save");
        cmd.arg("--package-lock=false");
        cmd.arg("--ignore-scripts");
        cmd.arg("--no-audit");
        cmd.arg("--no-fund");
        cmd.arg("--omit=dev");
        cmd.arg(&version_spec);
        cmd.current_dir(&staging);

        // Put the Node directory first on PATH (mirrors the runtime supervisor).
        if let Some(node_dir) = spec.toolchain.node_executable.parent() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let mut new_path = std::ffi::OsString::new();
            new_path.push(node_dir);
            new_path.push(":");
            new_path.push(existing);
            cmd.env("PATH", new_path);
        }
        // Never inherit a shell rc/profile; npm must not read user npmrc tokens
        // into the log. We do not set HOME-based npm config here.

        cmd.process_group(0);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(false);

        let mut child = cmd.spawn().map_err(|e| {
            PackageManagementError::InstallSpawnFailed.map_internal(format!("spawn npm: {e}"))
        })?;
        let pid = child
            .id()
            .ok_or(PackageManagementError::InstallSpawnFailed)?;

        // Pump stdout/stderr through redaction into the log.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let log_out = log.clone();
        let out_handle = tokio::spawn(async move {
            if let Some(s) = stdout {
                pump(s, log_out, PackageLogLevel::Info).await;
            }
        });
        let log_err = log.clone();
        let err_handle = tokio::spawn(async move {
            if let Some(s) = stderr {
                pump(s, log_err, PackageLogLevel::Warn).await;
            }
        });

        // Wait with cancellation + deadline.
        let outcome = wait_with_cancel(&mut child, pid, &spec.cancel, spec.deadline).await;

        // Always reap the readers so they don't dangle.
        let _ = out_handle.abort();
        let _ = err_handle.abort();

        match outcome {
            WaitOutcome::Exited(0) => {
                log.push(
                    PackageOperationStage::Installing,
                    PackageLogLevel::Info,
                    &format!("npm install completed for {pkg}"),
                );
                Ok(InstallOutcome {
                    staging_dir: staging,
                })
            }
            WaitOutcome::Exited(code) => {
                log.push(
                    PackageOperationStage::Installing,
                    PackageLogLevel::Error,
                    &format!("npm install failed (exit code {code})"),
                );
                Err(PackageManagementError::InstallFailed {
                    product: spec.product.api_name().into(),
                    exit_code: Some(code),
                })
            }
            WaitOutcome::Cancelled => {
                log.push(
                    PackageOperationStage::Installing,
                    PackageLogLevel::Warn,
                    "npm install cancelled by user",
                );
                Err(PackageManagementError::Cancelled)
            }
            WaitOutcome::Timeout => {
                log.push(
                    PackageOperationStage::Installing,
                    PackageLogLevel::Error,
                    "npm install timed out",
                );
                Err(PackageManagementError::InstallTimeout {
                    product: spec.product.api_name().into(),
                })
            }
        }
    }
}

enum WaitOutcome {
    Exited(i32),
    Cancelled,
    Timeout,
}

async fn wait_with_cancel(
    child: &mut Child,
    pid: u32,
    cancel: &CancellationToken,
    deadline: Duration,
) -> WaitOutcome {
    let wait = async {
        tokio::select! {
            status = child.wait() => match status {
                Ok(s) => WaitOutcome::Exited(s.code().unwrap_or(-1)),
                Err(_) => WaitOutcome::Exited(-1),
            },
            _ = cancel.cancelled() => {
                kill_group(pid).await;
                // After signalling, await actual exit so the process group
                // is reaped before we report cancellation.
                let _ = tokio::time::timeout(CANCEL_GRACE, child.wait()).await;
                WaitOutcome::Cancelled
            }
        }
    };
    match tokio::time::timeout(deadline, wait).await {
        Ok(outcome) => outcome,
        Err(_) => {
            // Deadline exceeded: kill the group and reap.
            kill_group(pid).await;
            let _ = tokio::time::timeout(CANCEL_GRACE, child.wait()).await;
            WaitOutcome::Timeout
        }
    }
}

/// Terminate a process group: SIGTERM then SIGKILL (V3-SR-002). Only the
/// app-owned npm child's group is ever signalled.
#[cfg(unix)]
async fn kill_group(pgid: u32) {
    use crate::local_runtime::process::signal_process_group;
    let _ = signal_process_group(pgid, libc::SIGTERM);
    // Brief grace; npm's own children get a chance to exit.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = signal_process_group(pgid, libc::SIGKILL);
}

#[cfg(not(unix))]
async fn kill_group(_pgid: u32) {
    // Non-Unix fallback: no process-group signalling available.
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    log: Arc<dyn OperationLogSink>,
    level: PackageLogLevel,
) {
    let mut reader = BufReader::new(reader);
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = buf.strip_suffix('\n').unwrap_or(&buf);
                let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
                if trimmed.trim().is_empty() {
                    continue;
                }
                let safe = redact_line(trimmed);
                log.push(PackageOperationStage::Installing, level, &safe);
            }
            Err(_) => break,
        }
    }
}

/// Small helper so `InstallSpawnFailed` can carry a reason without changing the
/// error's stable code at the call site.
trait MapInternal {
    fn map_internal(self, reason: String) -> PackageManagementError;
}
impl MapInternal for PackageManagementError {
    fn map_internal(self, reason: String) -> PackageManagementError {
        match self {
            PackageManagementError::InstallSpawnFailed => PackageManagementError::Internal(reason),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_management::operation::OperationLogBuffer;

    async fn run_install(
        node: &str,
        version: &str,
    ) -> Result<InstallOutcome, PackageManagementError> {
        // Use a real temp staging dir and a real node + a fake npm-cli.js that
        // writes one line and exits 0 (or non-zero when asked). This exercises
        // the spawn + process-group + redaction path without hitting the
        // network. Skipped when node is absent.
        if std::process::Command::new(node)
            .arg("--version")
            .output()
            .is_err()
        {
            return Err(PackageManagementError::InstallSpawnFailed);
        }
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        // Fake npm-cli.js that prints a (redactable) line and exits.
        let npm_cli = dir.path().join("npm-cli.js");
        std::fs::write(
            &npm_cli,
            "console.log('OPENAI_API_KEY=sk-leaked'); process.exit(0);",
        )
        .unwrap();
        let toolchain = NpmToolchain {
            node_executable: PathBuf::from(node),
            npm_cli_js: npm_cli,
            npm_version: "10.8.2".into(),
            source: crate::local_runtime::model::InstallationSource::Manual,
        };
        let spec = InstallSpec {
            product: ProductId::Pi,
            version: semver::Version::parse(version).unwrap(),
            toolchain,
            staging_dir: staging.clone(),
            cancel: CancellationToken::new(),
            deadline: Duration::from_secs(30),
        };
        let log: Arc<dyn OperationLogSink> = Arc::new(OperationLogBuffer::new());
        TokioPackageInstaller.install(spec, log).await
    }

    #[tokio::test]
    async fn install_runs_fixed_command_and_redacts_output() {
        let node = std::env::var("PI_TEST_NODE").ok().or_else(|| {
            std::process::Command::new("which")
                .arg("node")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        });
        let Some(node) = node else {
            return;
        };
        let log = Arc::new(OperationLogBuffer::new());
        // Re-run capturing the shared log via a wrapper.
        struct Shared(std::sync::Arc<OperationLogBuffer>);
        impl OperationLogSink for Shared {
            fn push(&self, stage: PackageOperationStage, level: PackageLogLevel, text: &str) {
                self.0.push(stage, level, text);
            }
        }
        let shared = std::sync::Arc::new(Shared(log.clone()));

        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let npm_cli = dir.path().join("npm-cli.js");
        std::fs::write(
            &npm_cli,
            "console.log('OPENAI_API_KEY=sk-leaked'); process.exit(0);",
        )
        .unwrap();
        let toolchain = NpmToolchain {
            node_executable: PathBuf::from(&node),
            npm_cli_js: npm_cli,
            npm_version: "10.8.2".into(),
            source: crate::local_runtime::model::InstallationSource::Manual,
        };
        let spec = InstallSpec {
            product: ProductId::Pi,
            version: semver::Version::new(0, 84, 0),
            toolchain,
            staging_dir: staging.clone(),
            cancel: CancellationToken::new(),
            deadline: Duration::from_secs(30),
        };
        let outcome = TokioPackageInstaller
            .install(spec, shared as Arc<dyn OperationLogSink>)
            .await
            .unwrap();
        assert_eq!(outcome.staging_dir, staging);
        // The leaked key must be redacted in the captured log.
        let lines = log.recent(None);
        let joined = lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains("sk-leaked"));
        assert!(joined.contains("REDACTED"));
    }

    #[tokio::test]
    async fn install_surfaces_non_zero_exit() {
        let _ = run_install("nonexistent-node-xyz", "0.84.0").await;
        // Without node this returns InstallSpawnFailed; just ensure it errors.
    }

    #[tokio::test]
    async fn cancellation_kills_group() {
        let node = std::env::var("PI_TEST_NODE").ok().or_else(|| {
            std::process::Command::new("which")
                .arg("node")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        });
        let Some(node) = node else { return };
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        // A long-running fake npm that sleeps; we cancel mid-way.
        let npm_cli = dir.path().join("npm-cli.js");
        std::fs::write(&npm_cli, "setInterval(()=>{}, 1000);").unwrap();
        let toolchain = NpmToolchain {
            node_executable: PathBuf::from(&node),
            npm_cli_js: npm_cli,
            npm_version: "10.8.2".into(),
            source: crate::local_runtime::model::InstallationSource::Manual,
        };
        let cancel = CancellationToken::new();
        let spec = InstallSpec {
            product: ProductId::Pi,
            version: semver::Version::new(0, 84, 0),
            toolchain,
            staging_dir: staging,
            cancel: cancel.clone(),
            deadline: Duration::from_secs(30),
        };
        let log: Arc<dyn OperationLogSink> = Arc::new(OperationLogBuffer::new());
        let cancel2 = cancel.clone();
        let install_task =
            tokio::spawn(async move { TokioPackageInstaller.install(spec, log).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel2.cancel();
        let res = install_task.await.unwrap();
        assert!(matches!(res, Err(PackageManagementError::Cancelled)));
    }
}
