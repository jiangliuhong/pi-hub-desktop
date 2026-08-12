//! Typed application errors and the stable DTO serialized across the Tauri
//! boundary (docs/design-v1.md §19, AGENTS.md §9).
//!
//! Rules:
//! - Recoverable errors use a typed enum; `unwrap()`/`expect()` and
//!   undocumented panics are forbidden in business paths (AGENTS.md §9).
//! - The Tauri boundary returns a stable, allowlist-serialized error-code DTO.
//! - `details` must be allowlist serialized: only non-sensitive fields such as
//!   host/port/stage are ever exposed. Secrets, Authorization, cookies and
//!   raw error strings never leak (AGENTS.md §6.1, SR-005).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Stable error codes surfaced to the UI (AGENTS.md §9, design §19).
///
/// Keep this list in sync with `features/connection/model.ts` on the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidProfile,
    DnsFailed,
    SshConnectTimeout,
    HostKeyUnknown,
    HostKeyChanged,
    AuthenticationFailed,
    PrivateKeyInvalid,
    PrivateKeyPassphraseRequired,
    TargetUnreachable,
    LocalListenerFailed,
    ServiceHttpError,
    TlsError,
    Cancelled,
    UnsupportedPlatform,
    NotFound,
    Io,
    Internal,
    // --- V1 connection reliability codes (plan-remote-pi-hub-performance §5.6) ---
    SshKeepaliveTimeout,
    SshTransportClosed,
    SshChannelOpenFailed,
    NetworkPathChanged,
    ForegroundSessionInvalid,
    ViewerReloadFailed,
    // --- V2 local runtime codes (docs/design-v2.md §18) ---
    LocalRuntimeUnsupportedPlatform,
    LocalRuntimeOperationInProgress,
    NodeNotFound,
    NodeVersionIncompatible,
    NodeExecutionFailed,
    PiHubNotFound,
    PiHubInstallationInvalid,
    PiHubVersionIncompatible,
    PiHubDoctorInvalidOutput,
    PiHubDoctorBlocked,
    PiAgentDirUnavailable,
    PiSessionDirUnavailable,
    PiAuthNotConfigured,
    PiModelNotAvailable,
    LocalPortConflict,
    LocalServiceProbeTimeout,
    LocalServiceProtocolIncompatible,
    LocalProcessStartFailed,
    LocalProcessExitedEarly,
    LocalProcessNotOwned,
    LocalProcessStopTimeout,
    LocalPortNotReleased,
    AutoStartSuppressed,
    LocalRuntimeCancelled,
    // --- V3 package management codes (docs/requirements-v3.md §14) ---
    PackagePlatformUnsupported,
    PackageOperationInProgress,
    PackageNodeUnavailable,
    PackageNpmUnavailable,
    PackageReleaseCheckFailed,
    PackageReleaseInvalid,
    PackageReleaseTokenExpired,
    PackageInstallSpawnFailed,
    PackageInstallFailed,
    PackageInstallTimeout,
    PackageVerificationFailed,
    PackageActivationFailed,
    PackageUpdateRequiresRestart,
    PackageExternalRuntimeActive,
    PackageRollbackFailed,
    PackageCancelled,
    PackageDiskSpaceInsufficient,
}

impl ErrorCode {
    /// Whether the user-facing flow may automatically retry this error
    /// (design §12.4). Authentication failures, host-key changes and invalid
    /// configuration must never auto-retry.
    pub fn auto_retryable(self) -> bool {
        match self {
            ErrorCode::DnsFailed
            | ErrorCode::SshConnectTimeout
            | ErrorCode::TargetUnreachable
            | ErrorCode::ServiceHttpError
            | ErrorCode::Io
            // Transport-side failures observed mid-session may auto-retry
            // (plan §5.4 / §5.6). They reflect a dead/aged SSH transport or a
            // foreground session that went stale, not a config or credential
            // problem.
            | ErrorCode::SshKeepaliveTimeout
            | ErrorCode::SshTransportClosed
            | ErrorCode::NetworkPathChanged
            | ErrorCode::ForegroundSessionInvalid => true,
            // Profile/credential/host-key/TLS/cancel errors are never retried
            // automatically; they require explicit user action.
            ErrorCode::InvalidProfile
            | ErrorCode::HostKeyUnknown
            | ErrorCode::HostKeyChanged
            | ErrorCode::AuthenticationFailed
            | ErrorCode::PrivateKeyInvalid
            | ErrorCode::PrivateKeyPassphraseRequired
            | ErrorCode::LocalListenerFailed
            | ErrorCode::TlsError
            | ErrorCode::Cancelled
            | ErrorCode::UnsupportedPlatform
            | ErrorCode::NotFound
            | ErrorCode::Internal
            // Channel-level failures are handled per-channel, not as a session
            // reconnect trigger; Viewer reload failure is surfaced for the user
            // to retry navigation, not auto-retried by the backoff path.
            | ErrorCode::SshChannelOpenFailed
            | ErrorCode::ViewerReloadFailed
            // V2 local runtime errors are never auto-retried by the V1
            // connection backoff path; the manager owns its own polling and
            // crash-loop protection (design-v2 §12.2, §14.2).
            | ErrorCode::LocalRuntimeUnsupportedPlatform
            | ErrorCode::LocalRuntimeOperationInProgress
            | ErrorCode::NodeNotFound
            | ErrorCode::NodeVersionIncompatible
            | ErrorCode::NodeExecutionFailed
            | ErrorCode::PiHubNotFound
            | ErrorCode::PiHubInstallationInvalid
            | ErrorCode::PiHubVersionIncompatible
            | ErrorCode::PiHubDoctorInvalidOutput
            | ErrorCode::PiHubDoctorBlocked
            | ErrorCode::PiAgentDirUnavailable
            | ErrorCode::PiSessionDirUnavailable
            | ErrorCode::PiAuthNotConfigured
            | ErrorCode::PiModelNotAvailable
            | ErrorCode::LocalPortConflict
            | ErrorCode::LocalServiceProbeTimeout
            | ErrorCode::LocalServiceProtocolIncompatible
            | ErrorCode::LocalProcessStartFailed
            | ErrorCode::LocalProcessExitedEarly
            | ErrorCode::LocalProcessNotOwned
            | ErrorCode::LocalProcessStopTimeout
            | ErrorCode::LocalPortNotReleased
            | ErrorCode::AutoStartSuppressed
            | ErrorCode::LocalRuntimeCancelled
            // V3 package management errors are never auto-retried by the V1
            // connection backoff path; the manager owns operation state and
            // exposes explicit retry semantics.
            | ErrorCode::PackagePlatformUnsupported
            | ErrorCode::PackageOperationInProgress
            | ErrorCode::PackageNodeUnavailable
            | ErrorCode::PackageNpmUnavailable
            | ErrorCode::PackageReleaseCheckFailed
            | ErrorCode::PackageReleaseInvalid
            | ErrorCode::PackageReleaseTokenExpired
            | ErrorCode::PackageInstallSpawnFailed
            | ErrorCode::PackageInstallFailed
            | ErrorCode::PackageInstallTimeout
            | ErrorCode::PackageVerificationFailed
            | ErrorCode::PackageActivationFailed
            | ErrorCode::PackageUpdateRequiresRestart
            | ErrorCode::PackageExternalRuntimeActive
            | ErrorCode::PackageRollbackFailed
            | ErrorCode::PackageCancelled
            | ErrorCode::PackageDiskSpaceInsufficient => false,
        }
    }

    /// Stable snake_case wire name, matching the serde rename. Used to surface
    /// a string code in non-sensitive diagnostics (FR-016) without pulling in
    /// serde at the call site.
    pub fn snake_case_name(self) -> &'static str {
        match self {
            ErrorCode::InvalidProfile => "invalid_profile",
            ErrorCode::DnsFailed => "dns_failed",
            ErrorCode::SshConnectTimeout => "ssh_connect_timeout",
            ErrorCode::HostKeyUnknown => "host_key_unknown",
            ErrorCode::HostKeyChanged => "host_key_changed",
            ErrorCode::AuthenticationFailed => "authentication_failed",
            ErrorCode::PrivateKeyInvalid => "private_key_invalid",
            ErrorCode::PrivateKeyPassphraseRequired => "private_key_passphrase_required",
            ErrorCode::TargetUnreachable => "target_unreachable",
            ErrorCode::LocalListenerFailed => "local_listener_failed",
            ErrorCode::ServiceHttpError => "service_http_error",
            ErrorCode::TlsError => "tls_error",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::UnsupportedPlatform => "unsupported_platform",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Io => "io",
            ErrorCode::Internal => "internal",
            ErrorCode::SshKeepaliveTimeout => "ssh_keepalive_timeout",
            ErrorCode::SshTransportClosed => "ssh_transport_closed",
            ErrorCode::SshChannelOpenFailed => "ssh_channel_open_failed",
            ErrorCode::NetworkPathChanged => "network_path_changed",
            ErrorCode::ForegroundSessionInvalid => "foreground_session_invalid",
            ErrorCode::ViewerReloadFailed => "viewer_reload_failed",
            ErrorCode::LocalRuntimeUnsupportedPlatform => "local_runtime_unsupported_platform",
            ErrorCode::LocalRuntimeOperationInProgress => "local_runtime_operation_in_progress",
            ErrorCode::NodeNotFound => "node_not_found",
            ErrorCode::NodeVersionIncompatible => "node_version_incompatible",
            ErrorCode::NodeExecutionFailed => "node_execution_failed",
            ErrorCode::PiHubNotFound => "pi_hub_not_found",
            ErrorCode::PiHubInstallationInvalid => "pi_hub_installation_invalid",
            ErrorCode::PiHubVersionIncompatible => "pi_hub_version_incompatible",
            ErrorCode::PiHubDoctorInvalidOutput => "pi_hub_doctor_invalid_output",
            ErrorCode::PiHubDoctorBlocked => "pi_hub_doctor_blocked",
            ErrorCode::PiAgentDirUnavailable => "pi_agent_dir_unavailable",
            ErrorCode::PiSessionDirUnavailable => "pi_session_dir_unavailable",
            ErrorCode::PiAuthNotConfigured => "pi_auth_not_configured",
            ErrorCode::PiModelNotAvailable => "pi_model_not_available",
            ErrorCode::LocalPortConflict => "local_port_conflict",
            ErrorCode::LocalServiceProbeTimeout => "local_service_probe_timeout",
            ErrorCode::LocalServiceProtocolIncompatible => "local_service_protocol_incompatible",
            ErrorCode::LocalProcessStartFailed => "local_process_start_failed",
            ErrorCode::LocalProcessExitedEarly => "local_process_exited_early",
            ErrorCode::LocalProcessNotOwned => "local_process_not_owned",
            ErrorCode::LocalProcessStopTimeout => "local_process_stop_timeout",
            ErrorCode::LocalPortNotReleased => "local_port_not_released",
            ErrorCode::AutoStartSuppressed => "auto_start_suppressed",
            ErrorCode::LocalRuntimeCancelled => "local_runtime_cancelled",
            ErrorCode::PackagePlatformUnsupported => "package_platform_unsupported",
            ErrorCode::PackageOperationInProgress => "package_operation_in_progress",
            ErrorCode::PackageNodeUnavailable => "package_node_unavailable",
            ErrorCode::PackageNpmUnavailable => "package_npm_unavailable",
            ErrorCode::PackageReleaseCheckFailed => "package_release_check_failed",
            ErrorCode::PackageReleaseInvalid => "package_release_invalid",
            ErrorCode::PackageReleaseTokenExpired => "package_release_token_expired",
            ErrorCode::PackageInstallSpawnFailed => "package_install_spawn_failed",
            ErrorCode::PackageInstallFailed => "package_install_failed",
            ErrorCode::PackageInstallTimeout => "package_install_timeout",
            ErrorCode::PackageVerificationFailed => "package_verification_failed",
            ErrorCode::PackageActivationFailed => "package_activation_failed",
            ErrorCode::PackageUpdateRequiresRestart => "package_update_requires_restart",
            ErrorCode::PackageExternalRuntimeActive => "package_external_runtime_active",
            ErrorCode::PackageRollbackFailed => "package_rollback_failed",
            ErrorCode::PackageCancelled => "package_cancelled",
            ErrorCode::PackageDiskSpaceInsufficient => "package_disk_space_insufficient",
        }
    }
}

/// Top-level typed error for the Rust core (design §19).
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid service profile: {0}")]
    Profile(#[from] ProfileError),

    #[error("credential error: {0}")]
    Credential(#[from] CredentialError),

    #[error("ssh error: {0}")]
    Ssh(#[from] SshError),

    #[error("forward error: {0}")]
    Forward(#[from] ForwardError),

    #[error("service error: {0}")]
    Service(#[from] ServiceError),

    #[error("viewer error: {0}")]
    Viewer(#[from] ViewerError),

    #[error("platform error: {0}")]
    Platform(#[from] PlatformError),

    #[error("local runtime error: {0}")]
    LocalRuntime(#[from] LocalRuntimeError),

    #[error("package management error: {0}")]
    PackageManagement(#[from] PackageManagementError),

    #[error("operation cancelled")]
    Cancelled,
}

impl AppError {
    /// Map any app error to its stable code (design §19, AGENTS.md §9).
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Profile(e) => e.code(),
            AppError::Credential(e) => e.code(),
            AppError::Ssh(e) => e.code(),
            AppError::Forward(e) => e.code(),
            AppError::Service(e) => e.code(),
            AppError::Viewer(e) => e.code(),
            AppError::Platform(e) => e.code(),
            AppError::LocalRuntime(e) => e.code(),
            AppError::PackageManagement(e) => e.code(),
            AppError::Cancelled => ErrorCode::Cancelled,
        }
    }

    /// Map to a serializable DTO. `details` is filled only with allowlisted,
    /// non-sensitive context (design §19). Never expose raw source error
    /// strings or secret-bearing values.
    pub fn to_dto(&self) -> ErrorDto {
        let code = self.code();
        ErrorDto {
            code,
            message: self.user_message(),
            retryable: code.auto_retryable(),
            stage: None,
            details: BTreeMap::new(),
        }
    }

    /// Build a DTO with an extra allowlisted detail + stage. Callers are
    /// responsible for only inserting non-sensitive keys.
    pub fn to_dto_with(
        &self,
        stage: Option<String>,
        details: BTreeMap<String, String>,
    ) -> ErrorDto {
        let mut dto = self.to_dto();
        dto.stage = stage;
        dto.details = details;
        dto
    }

    fn user_message(&self) -> String {
        match self {
            AppError::Profile(e) => e.user_message(),
            AppError::Credential(e) => e.user_message(),
            AppError::Ssh(e) => e.user_message(),
            AppError::Forward(e) => e.user_message(),
            AppError::Service(e) => e.user_message(),
            AppError::Viewer(e) => e.user_message(),
            AppError::Platform(e) => e.user_message(),
            AppError::LocalRuntime(e) => e.user_message(),
            AppError::PackageManagement(e) => e.user_message(),
            AppError::Cancelled => "操作已取消。".to_string(),
        }
    }
}

/// Service profile validation / persistence errors.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("profile not found")]
    NotFound,
    #[error("invalid profile: {0}")]
    Invalid(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("migration error: {0}")]
    Migration(String),
}

impl ProfileError {
    fn code(&self) -> ErrorCode {
        match self {
            ProfileError::NotFound => ErrorCode::NotFound,
            ProfileError::Invalid(_) => ErrorCode::InvalidProfile,
            ProfileError::Storage(_) | ProfileError::Migration(_) => ErrorCode::Internal,
        }
    }

    fn user_message(&self) -> String {
        match self {
            ProfileError::NotFound => "找不到该服务配置。".to_string(),
            ProfileError::Invalid(_) => "服务配置无效，请检查必填字段。".to_string(),
            ProfileError::Storage(_) => "无法读写服务配置文件。".to_string(),
            ProfileError::Migration(_) => "服务配置版本不兼容，需要迁移。".to_string(),
        }
    }
}

/// Credential store errors.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential not found")]
    NotFound,
    #[error("keychain error: {0}")]
    Backend(String),
}

impl CredentialError {
    fn code(&self) -> ErrorCode {
        match self {
            CredentialError::NotFound => ErrorCode::NotFound,
            CredentialError::Backend(_) => ErrorCode::Internal,
        }
    }

    fn user_message(&self) -> String {
        match self {
            CredentialError::NotFound => "找不到对应的凭据，请重新输入。".to_string(),
            CredentialError::Backend(_) => "系统钥匙串访问失败。".to_string(),
        }
    }
}

/// SSH transport / authentication errors.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("dns resolution failed for {host}")]
    DnsFailed { host: String },
    #[error("ssh connect timeout to {host}:{port}")]
    ConnectTimeout { host: String, port: u16 },
    #[error("host key unknown for {host}:{port}")]
    HostKeyUnknown { host: String, port: u16 },
    #[error("host key changed for {host}:{port}")]
    HostKeyChanged { host: String, port: u16 },
    #[error("authentication failed for user {user}")]
    AuthenticationFailed { user: String },
    #[error("private key invalid")]
    PrivateKeyInvalid,
    #[error("private key passphrase required")]
    PrivateKeyPassphraseRequired,
    #[error("ssh transport error: {0}")]
    Transport(String),
    /// The SSH transport closed mid-session (plan §5.4 / §5.6). Classified by
    /// [`SessionCloseReason`] so the reconnect path can decide retryability
    /// without re-inspecting raw russh errors. Produced by the session health
    /// monitor, never by the initial connect/auth path.
    #[error("ssh session closed: {reason}")]
    SessionClosed { reason: SessionCloseReason },
}

/// Non-sensitive classification of why an established SSH session ended
/// (plan §5.4 / §5.6). Carries no secrets, host/port beyond what diagnostics
/// already record, or business data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCloseReason {
    /// The remote sent an explicit SSH disconnect message.
    RemoteDisconnect,
    /// Keepalive probes went unanswered past the configured threshold.
    KeepaliveTimeout,
    /// The underlying socket closed unexpectedly (TCP RST, NAT reclaim, etc.).
    NetworkError,
    /// Close source could not be classified (treated as retryable).
    Unknown,
}

impl SessionCloseReason {
    /// Stable wire name used in diagnostics (non-sensitive).
    pub fn as_str(self) -> &'static str {
        match self {
            SessionCloseReason::RemoteDisconnect => "remote_disconnect",
            SessionCloseReason::KeepaliveTimeout => "keepalive_timeout",
            SessionCloseReason::NetworkError => "network_error",
            SessionCloseReason::Unknown => "unknown",
        }
    }
}

impl fmt::Display for SessionCloseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SshError {
    fn code(&self) -> ErrorCode {
        match self {
            SshError::DnsFailed { .. } => ErrorCode::DnsFailed,
            SshError::ConnectTimeout { .. } => ErrorCode::SshConnectTimeout,
            SshError::HostKeyUnknown { .. } => ErrorCode::HostKeyUnknown,
            SshError::HostKeyChanged { .. } => ErrorCode::HostKeyChanged,
            SshError::AuthenticationFailed { .. } => ErrorCode::AuthenticationFailed,
            SshError::PrivateKeyInvalid => ErrorCode::PrivateKeyInvalid,
            SshError::PrivateKeyPassphraseRequired => ErrorCode::PrivateKeyPassphraseRequired,
            // Initial-handshake transport errors are unexpected and not
            // auto-retried as a session drop (plan §5.6).
            SshError::Transport(_) => ErrorCode::Internal,
            // Mid-session closes map to the new retryable codes. Keepalive
            // timeout gets its own code so diagnostics can distinguish it;
            // every other classified close surfaces as a transport close.
            SshError::SessionClosed {
                reason: SessionCloseReason::KeepaliveTimeout,
            } => ErrorCode::SshKeepaliveTimeout,
            SshError::SessionClosed { .. } => ErrorCode::SshTransportClosed,
        }
    }

    fn user_message(&self) -> String {
        match self {
            SshError::DnsFailed { host } => {
                format!("无法解析主机 {host}，请检查地址或网络。")
            }
            SshError::ConnectTimeout { host, port } => {
                format!("连接 {host}:{port} 超时，请确认 SSH 服务是否可达。")
            }
            SshError::HostKeyUnknown { .. } => "首次连接需要确认 SSH Host Key。".to_string(),
            SshError::HostKeyChanged { .. } => {
                "SSH Host Key 已变化，连接已阻止。请到服务安全设置中确认替换。".to_string()
            }
            SshError::AuthenticationFailed { user } => {
                format!("SSH 用户 {user} 认证失败，请检查凭据。")
            }
            SshError::PrivateKeyInvalid => "私钥无效或格式不受支持。".to_string(),
            SshError::PrivateKeyPassphraseRequired => "私钥需要 Passphrase 才能解密。".to_string(),
            SshError::Transport(_) => "SSH 通信异常。".to_string(),
            SshError::SessionClosed { reason } => match reason {
                SessionCloseReason::KeepaliveTimeout => {
                    "SSH 连接因长时间无响应被关闭，正在尝试重新连接。".to_string()
                }
                _ => "SSH 连接已断开，正在尝试重新连接。".to_string(),
            },
        }
    }
}

/// Local forward (listener / direct-tcpip) errors.
#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    #[error("local listener bind failed on 127.0.0.1: {0}")]
    ListenerFailed(String),
    #[error("target unreachable: {0}")]
    TargetUnreachable(String),
    #[error("forward io error: {0}")]
    Io(String),
}

impl ForwardError {
    fn code(&self) -> ErrorCode {
        match self {
            ForwardError::ListenerFailed(_) => ErrorCode::LocalListenerFailed,
            ForwardError::TargetUnreachable(_) => ErrorCode::TargetUnreachable,
            ForwardError::Io(_) => ErrorCode::Io,
        }
    }

    fn user_message(&self) -> String {
        match self {
            ForwardError::ListenerFailed(_) => "无法在本地分配端口。".to_string(),
            ForwardError::TargetUnreachable(_) => {
                "SSH 隧道建立成功，但目标服务不可达。".to_string()
            }
            ForwardError::Io(_) => "转发数据时发生 IO 错误。".to_string(),
        }
    }
}

/// Service reachability errors (design §17).
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("http error: {0}")]
    Http(String),
    #[error("tls error: {0}")]
    Tls(String),
}

impl ServiceError {
    fn code(&self) -> ErrorCode {
        match self {
            ServiceError::Http(_) => ErrorCode::ServiceHttpError,
            ServiceError::Tls(_) => ErrorCode::TlsError,
        }
    }

    fn user_message(&self) -> String {
        match self {
            ServiceError::Http(_) => "Pi Hub 服务返回 HTTP 错误。".to_string(),
            ServiceError::Tls(_) => "TLS 证书校验失败，V1 不允许忽略证书错误。".to_string(),
        }
    }
}

/// Viewer (Service WebView) lifecycle errors.
#[derive(Debug, thiserror::Error)]
pub enum ViewerError {
    #[error("viewer error: {0}")]
    Other(String),
}

impl ViewerError {
    fn code(&self) -> ErrorCode {
        ErrorCode::Internal
    }

    fn user_message(&self) -> String {
        "Service WebView 发生错误。".to_string()
    }
}

/// Platform adaptation errors.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("unsupported platform")]
    Unsupported,
    #[error("platform error: {0}")]
    Other(String),
}

impl PlatformError {
    fn code(&self) -> ErrorCode {
        match self {
            PlatformError::Unsupported => ErrorCode::UnsupportedPlatform,
            PlatformError::Other(_) => ErrorCode::Internal,
        }
    }

    fn user_message(&self) -> String {
        match self {
            PlatformError::Unsupported => "当前平台不受支持。".to_string(),
            PlatformError::Other(_) => "平台适配层发生错误。".to_string(),
        }
    }
}

/// V2 local runtime domain errors (docs/design-v2.md §18).
///
/// Each variant carries enough non-sensitive context to build an actionable
/// `ErrorDto`. Secrets, full environment dumps and raw stdout are never stored
/// here (AGENTS.md §6.1, V2-SR-003/004).
#[derive(Debug, Clone, thiserror::Error)]
pub enum LocalRuntimeError {
    #[error("local runtime is not supported on this platform")]
    UnsupportedPlatform,
    #[error("another local runtime operation is already in progress")]
    OperationInProgress,
    #[error("node.js was not found")]
    NodeNotFound,
    #[error("node.js version {version} does not satisfy {required_version}")]
    NodeVersionIncompatible {
        version: String,
        required_version: String,
    },
    #[error("node.js execution failed: {0}")]
    NodeExecutionFailed(String),
    #[error("pi hub was not found")]
    PiHubNotFound,
    #[error("pi hub installation is invalid: {0}")]
    PiHubInstallationInvalid(String),
    #[error("pi hub version {version} does not satisfy {required_version}")]
    PiHubVersionIncompatible {
        version: String,
        required_version: String,
    },
    #[error("pi hub doctor output could not be parsed: {0}")]
    DoctorInvalidOutput(String),
    #[error("pi hub environment check is blocked: {0}")]
    DoctorBlocked(String),
    #[error("pi agent directory is unavailable: {0}")]
    PiAgentDirUnavailable(String),
    #[error("pi session directory is unavailable: {0}")]
    PiSessionDirUnavailable(String),
    #[error("pi authentication is not configured")]
    PiAuthNotConfigured,
    #[error("no pi model is available")]
    PiModelNotAvailable,
    #[error("local port {port} is already in use")]
    PortConflict { port: u16 },
    #[error("local service probe timed out")]
    ServiceProbeTimeout,
    #[error("local service protocol is incompatible (got protocol {got}, supported {min}-{max})")]
    ServiceProtocolIncompatible { got: u32, min: u32, max: u32 },
    #[error("local process failed to start: {0}")]
    ProcessStartFailed(String),
    #[error("local process exited early (exit code {exit_code:?})")]
    ProcessExitedEarly { exit_code: Option<i32> },
    #[error("local process is not owned by this app")]
    ProcessNotOwned,
    #[error("local process did not stop within the graceful period")]
    ProcessStopTimeout,
    #[error("local port {port} was not released after stop")]
    PortNotReleased { port: u16 },
    #[error("automatic start is suppressed due to repeated failures")]
    AutoStartSuppressed,
    #[error("local runtime operation cancelled")]
    Cancelled,
    #[error("local runtime internal error: {0}")]
    Internal(String),
}

impl LocalRuntimeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            LocalRuntimeError::UnsupportedPlatform => ErrorCode::LocalRuntimeUnsupportedPlatform,
            LocalRuntimeError::OperationInProgress => ErrorCode::LocalRuntimeOperationInProgress,
            LocalRuntimeError::NodeNotFound => ErrorCode::NodeNotFound,
            LocalRuntimeError::NodeVersionIncompatible { .. } => ErrorCode::NodeVersionIncompatible,
            LocalRuntimeError::NodeExecutionFailed(_) => ErrorCode::NodeExecutionFailed,
            LocalRuntimeError::PiHubNotFound => ErrorCode::PiHubNotFound,
            LocalRuntimeError::PiHubInstallationInvalid(_) => ErrorCode::PiHubInstallationInvalid,
            LocalRuntimeError::PiHubVersionIncompatible { .. } => {
                ErrorCode::PiHubVersionIncompatible
            }
            LocalRuntimeError::DoctorInvalidOutput(_) => ErrorCode::PiHubDoctorInvalidOutput,
            LocalRuntimeError::DoctorBlocked(_) => ErrorCode::PiHubDoctorBlocked,
            LocalRuntimeError::PiAgentDirUnavailable(_) => ErrorCode::PiAgentDirUnavailable,
            LocalRuntimeError::PiSessionDirUnavailable(_) => ErrorCode::PiSessionDirUnavailable,
            LocalRuntimeError::PiAuthNotConfigured => ErrorCode::PiAuthNotConfigured,
            LocalRuntimeError::PiModelNotAvailable => ErrorCode::PiModelNotAvailable,
            LocalRuntimeError::PortConflict { .. } => ErrorCode::LocalPortConflict,
            LocalRuntimeError::ServiceProbeTimeout => ErrorCode::LocalServiceProbeTimeout,
            LocalRuntimeError::ServiceProtocolIncompatible { .. } => {
                ErrorCode::LocalServiceProtocolIncompatible
            }
            LocalRuntimeError::ProcessStartFailed(_) => ErrorCode::LocalProcessStartFailed,
            LocalRuntimeError::ProcessExitedEarly { .. } => ErrorCode::LocalProcessExitedEarly,
            LocalRuntimeError::ProcessNotOwned => ErrorCode::LocalProcessNotOwned,
            LocalRuntimeError::ProcessStopTimeout => ErrorCode::LocalProcessStopTimeout,
            LocalRuntimeError::PortNotReleased { .. } => ErrorCode::LocalPortNotReleased,
            LocalRuntimeError::AutoStartSuppressed => ErrorCode::AutoStartSuppressed,
            LocalRuntimeError::Cancelled => ErrorCode::LocalRuntimeCancelled,
            LocalRuntimeError::Internal(_) => ErrorCode::Internal,
        }
    }

    fn user_message(&self) -> String {
        match self {
            LocalRuntimeError::UnsupportedPlatform => {
                "当前平台不支持本机 Pi Hub 管理。".to_string()
            }
            LocalRuntimeError::OperationInProgress => {
                "本机 Pi Hub 正在执行其他操作，请稍候。".to_string()
            }
            LocalRuntimeError::NodeNotFound => {
                "未找到 Node.js，请在设置中手动选择 Node.js 安装路径。".to_string()
            }
            LocalRuntimeError::NodeVersionIncompatible {
                version,
                required_version,
            } => format!(
                "Node.js 版本 {version} 不满足要求（需 {required_version}），请升级 Node.js。"
            ),
            LocalRuntimeError::NodeExecutionFailed(_) => {
                "执行 Node.js 失败，请检查安装路径是否有效。".to_string()
            }
            LocalRuntimeError::PiHubNotFound => {
                "未找到 Pi Hub 安装，请在设置中选择 Pi Hub 入口。".to_string()
            }
            LocalRuntimeError::PiHubInstallationInvalid(_) => {
                "Pi Hub 安装无效，包身份或构建产物校验失败。".to_string()
            }
            LocalRuntimeError::PiHubVersionIncompatible {
                version,
                required_version,
            } => format!(
                "Pi Hub 版本 {version} 不满足要求（需 {required_version}），请升级 Pi Hub。"
            ),
            LocalRuntimeError::DoctorInvalidOutput(_) => {
                "Pi Hub 环境检查输出无法解析，请确认 Pi Hub 版本与协议兼容。".to_string()
            }
            LocalRuntimeError::DoctorBlocked(_) => {
                "存在阻断性问题，无法启动本机 Pi Hub。请先查看环境检查结果。".to_string()
            }
            LocalRuntimeError::PiAgentDirUnavailable(_) => {
                "Pi Agent 数据目录不可用或无法创建，请检查路径与权限。".to_string()
            }
            LocalRuntimeError::PiSessionDirUnavailable(_) => {
                "Pi Session 目录不可用，请检查路径与权限。".to_string()
            }
            LocalRuntimeError::PiAuthNotConfigured => {
                "尚未配置任何 Provider 认证，Agent 任务可能无法完成。".to_string()
            }
            LocalRuntimeError::PiModelNotAvailable => {
                "未找到可用模型，请检查模型配置。".to_string()
            }
            LocalRuntimeError::PortConflict { port } => {
                format!("本地端口 {port} 已被占用，且该服务不是 Pi Hub。请在设置中更换端口。")
            }
            LocalRuntimeError::ServiceProbeTimeout => "探测本机服务超时，请稍后重试。".to_string(),
            LocalRuntimeError::ServiceProtocolIncompatible { got, min, max } => {
                format!("本机服务协议版本 {got} 与本客户端（支持 {min}-{max}）不兼容。")
            }
            LocalRuntimeError::ProcessStartFailed(detail) => {
                format!("启动本机 Pi Hub 进程失败：{detail}。")
            }
            LocalRuntimeError::ProcessExitedEarly { exit_code: Some(code) } => {
                format!("本机 Pi Hub 启动后立即退出（退出码 {code}）。请查看「日志」了解详情。")
            }
            LocalRuntimeError::ProcessExitedEarly { exit_code: None } => {
                "本机 Pi Hub 启动后立即退出（未获取到退出码，可能被信号终止）。请查看「日志」了解详情。".to_string()
            }
            LocalRuntimeError::ProcessNotOwned => {
                "该 Pi Hub 由其他程序启动，本客户端无权停止。".to_string()
            }
            LocalRuntimeError::ProcessStopTimeout => {
                "本机 Pi Hub 未在限定时间内退出，已强制终止。".to_string()
            }
            LocalRuntimeError::PortNotReleased { port } => {
                format!("停止后本地端口 {port} 仍未释放，请检查是否有残留进程。")
            }
            LocalRuntimeError::AutoStartSuppressed => {
                "自动启动因多次失败被暂时抑制，请手动启动或调整设置。".to_string()
            }
            LocalRuntimeError::Cancelled => "本机 Pi Hub 操作已取消。".to_string(),
            LocalRuntimeError::Internal(_) => "本机运行时发生内部错误。".to_string(),
        }
    }

    /// Build a serializable DTO, attaching allowlisted non-sensitive details.
    /// Only keys in the design-v2 §18 allowlist are ever inserted.
    pub fn to_dto_with_details(&self) -> ErrorDto {
        let mut dto = AppError::LocalRuntime(self.clone()).to_dto();
        let mut details = BTreeMap::new();
        match self {
            LocalRuntimeError::NodeVersionIncompatible {
                version,
                required_version,
            } => {
                details.insert("version".into(), version.clone());
                details.insert("requiredVersion".into(), required_version.clone());
            }
            LocalRuntimeError::PiHubVersionIncompatible {
                version,
                required_version,
            } => {
                details.insert("version".into(), version.clone());
                details.insert("requiredVersion".into(), required_version.clone());
            }
            LocalRuntimeError::PortConflict { port }
            | LocalRuntimeError::PortNotReleased { port } => {
                details.insert("port".into(), port.to_string());
            }
            LocalRuntimeError::ServiceProtocolIncompatible { got, min, max } => {
                details.insert("got".into(), got.to_string());
                details.insert("min".into(), min.to_string());
                details.insert("max".into(), max.to_string());
            }
            LocalRuntimeError::ProcessExitedEarly { exit_code: Some(c) } => {
                details.insert("exitCode".into(), c.to_string());
            }
            LocalRuntimeError::ProcessExitedEarly { exit_code: None } => {}
            _ => {}
        }
        dto.details = details;
        dto
    }
}

/// V3 package management domain errors (docs/requirements-v3.md §14, §16).
///
/// Each variant carries only non-sensitive context (product, stage, version,
/// bytes). npm stderr, registry raw output, full environment and any secret
/// are never stored here (AGENTS.md §6.1, V3-SR-005).
#[derive(Debug, Clone, thiserror::Error)]
pub enum PackageManagementError {
    #[error("package management is not supported on this platform")]
    UnsupportedPlatform,
    #[error("another package operation is already in progress")]
    OperationInProgress,
    #[error("node.js is unavailable or incompatible for package operations")]
    NodeUnavailable,
    #[error("npm cli is unavailable or invalid")]
    NpmUnavailable,
    #[error("release metadata check failed for {product}")]
    ReleaseCheckFailed { product: String },
    #[error("release metadata for {product} is invalid")]
    ReleaseInvalid { product: String },
    #[error("release token is expired or unknown")]
    ReleaseTokenExpired,
    #[error("npm install failed to spawn")]
    InstallSpawnFailed,
    #[error("npm install failed for {product} (exit code {exit_code:?})")]
    InstallFailed {
        product: String,
        exit_code: Option<i32>,
    },
    #[error("npm install timed out for {product}")]
    InstallTimeout { product: String },
    #[error("post-install verification failed for {product}: {reason}")]
    VerificationFailed { product: String, reason: String },
    #[error("activation failed for {product}: {reason}")]
    ActivationFailed { product: String, reason: String },
    #[error("pi hub update requires restart confirmation")]
    UpdateRequiresRestart,
    #[error("an external pi hub is currently running")]
    ExternalRuntimeActive,
    #[error("rollback failed for {product}: {reason}")]
    RollbackFailed { product: String, reason: String },
    #[error("package operation cancelled")]
    Cancelled,
    #[error("disk space is insufficient (required {required_bytes} bytes)")]
    DiskSpaceInsufficient { required_bytes: u64 },
    #[error("package management internal error: {0}")]
    Internal(String),
}

impl PackageManagementError {
    pub fn code(&self) -> ErrorCode {
        match self {
            PackageManagementError::UnsupportedPlatform => ErrorCode::PackagePlatformUnsupported,
            PackageManagementError::OperationInProgress => ErrorCode::PackageOperationInProgress,
            PackageManagementError::NodeUnavailable => ErrorCode::PackageNodeUnavailable,
            PackageManagementError::NpmUnavailable => ErrorCode::PackageNpmUnavailable,
            PackageManagementError::ReleaseCheckFailed { .. } => {
                ErrorCode::PackageReleaseCheckFailed
            }
            PackageManagementError::ReleaseInvalid { .. } => ErrorCode::PackageReleaseInvalid,
            PackageManagementError::ReleaseTokenExpired => ErrorCode::PackageReleaseTokenExpired,
            PackageManagementError::InstallSpawnFailed => ErrorCode::PackageInstallSpawnFailed,
            PackageManagementError::InstallFailed { .. } => ErrorCode::PackageInstallFailed,
            PackageManagementError::InstallTimeout { .. } => ErrorCode::PackageInstallTimeout,
            PackageManagementError::VerificationFailed { .. } => {
                ErrorCode::PackageVerificationFailed
            }
            PackageManagementError::ActivationFailed { .. } => ErrorCode::PackageActivationFailed,
            PackageManagementError::UpdateRequiresRestart => {
                ErrorCode::PackageUpdateRequiresRestart
            }
            PackageManagementError::ExternalRuntimeActive => {
                ErrorCode::PackageExternalRuntimeActive
            }
            PackageManagementError::RollbackFailed { .. } => ErrorCode::PackageRollbackFailed,
            PackageManagementError::Cancelled => ErrorCode::PackageCancelled,
            PackageManagementError::DiskSpaceInsufficient { .. } => {
                ErrorCode::PackageDiskSpaceInsufficient
            }
            PackageManagementError::Internal(_) => ErrorCode::Internal,
        }
    }

    fn user_message(&self) -> String {
        match self {
            PackageManagementError::UnsupportedPlatform => {
                "当前平台不支持本机组件包管理。".to_string()
            }
            PackageManagementError::OperationInProgress => {
                "已有包管理操作正在进行，请稍候。".to_string()
            }
            PackageManagementError::NodeUnavailable => {
                "未找到可用的 Node.js，无法执行受管安装。请先安装或选择 Node.js。".to_string()
            }
            PackageManagementError::NpmUnavailable => {
                "未找到可用的 npm，无法执行受管安装。请确认 Node.js 附带 npm。".to_string()
            }
            PackageManagementError::ReleaseCheckFailed { product } => {
                format!("无法获取 {product} 的最新版本信息，请检查网络后重试。")
            }
            PackageManagementError::ReleaseInvalid { product } => {
                format!("{product} 的版本元数据不合法，暂不可安装。")
            }
            PackageManagementError::ReleaseTokenExpired => {
                "版本选择已过期，请重新检查更新。".to_string()
            }
            PackageManagementError::InstallSpawnFailed => "无法启动 npm 安装进程。".to_string(),
            PackageManagementError::InstallFailed { product, .. } => {
                format!("{product} 安装失败，已清理临时文件，未改动现有安装。")
            }
            PackageManagementError::InstallTimeout { product } => {
                format!("{product} 安装超时，已取消并清理。")
            }
            PackageManagementError::VerificationFailed { product, .. } => {
                format!("{product} 安装后校验失败，未激活，现有版本保持不变。")
            }
            PackageManagementError::ActivationFailed { product, .. } => {
                format!("{product} 激活失败，已尝试回滚。")
            }
            PackageManagementError::UpdateRequiresRestart => {
                "Pi Hub 更新需要确认后重启。".to_string()
            }
            PackageManagementError::ExternalRuntimeActive => {
                "检测到外部启动的 Pi Hub 正在运行，不会停止它；可稍后激活受管副本。".to_string()
            }
            PackageManagementError::RollbackFailed { product, .. } => {
                format!("{product} 回滚失败，请手动处理。")
            }
            PackageManagementError::Cancelled => "包管理操作已取消。".to_string(),
            PackageManagementError::DiskSpaceInsufficient { .. } => {
                "磁盘空间不足，请清理后重试。".to_string()
            }
            PackageManagementError::Internal(_) => "包管理发生内部错误。".to_string(),
        }
    }

    /// Build a serializable DTO with allowlisted non-sensitive details.
    pub fn to_dto_with_details(&self) -> ErrorDto {
        let mut dto = AppError::PackageManagement(self.clone()).to_dto();
        let mut details = BTreeMap::new();
        match self {
            PackageManagementError::ReleaseCheckFailed { product }
            | PackageManagementError::ReleaseInvalid { product }
            | PackageManagementError::InstallTimeout { product }
            | PackageManagementError::VerificationFailed { product, .. }
            | PackageManagementError::ActivationFailed { product, .. }
            | PackageManagementError::RollbackFailed { product, .. } => {
                details.insert("product".into(), product.clone());
            }
            PackageManagementError::InstallFailed { product, exit_code } => {
                details.insert("product".into(), product.clone());
                if let Some(c) = exit_code {
                    details.insert("exitCode".into(), c.to_string());
                }
            }
            PackageManagementError::DiskSpaceInsufficient { required_bytes } => {
                details.insert("requiredBytes".into(), required_bytes.to_string());
            }
            _ => {}
        }
        dto.details = details;
        dto
    }
}

/// Stable error DTO serialized across the Tauri boundary (design §19).
///
/// `details` is a map of allowlisted, non-sensitive context only. Never put
/// passwords, private keys, Authorization headers, cookies or page content
/// here (AGENTS.md §6.1, SR-005).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDto {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

impl fmt::Display for ErrorDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_and_host_key_errors_are_not_auto_retryable() {
        assert!(!ErrorCode::AuthenticationFailed.auto_retryable());
        assert!(!ErrorCode::HostKeyChanged.auto_retryable());
        assert!(!ErrorCode::HostKeyUnknown.auto_retryable());
        assert!(!ErrorCode::PrivateKeyInvalid.auto_retryable());
        assert!(!ErrorCode::InvalidProfile.auto_retryable());
        assert!(!ErrorCode::TlsError.auto_retryable());
    }

    #[test]
    fn transient_errors_are_auto_retryable() {
        assert!(ErrorCode::DnsFailed.auto_retryable());
        assert!(ErrorCode::SshConnectTimeout.auto_retryable());
        assert!(ErrorCode::TargetUnreachable.auto_retryable());
    }

    #[test]
    fn dto_round_trips_through_json_without_secrets() {
        let err = AppError::Ssh(SshError::AuthenticationFailed {
            user: "ubuntu".to_string(),
        });
        let dto = err.to_dto_with(Some("authenticating".to_string()), {
            let mut m = BTreeMap::new();
            m.insert("host".to_string(), "vps.example.com".to_string());
            m.insert("port".to_string(), "22".to_string());
            m
        });
        let json = serde_json::to_string(&dto).expect("serialize dto");
        // The DTO must serialize to a stable snake_case code.
        assert!(
            json.contains("\"code\":\"authentication_failed\""),
            "{json}"
        );
        assert!(json.contains("\"stage\":\"authenticating\""));
        // The user-facing message is non-empty and action-oriented.
        assert!(!dto.message.is_empty());
        // The raw private key / password must never appear (sanity guard).
        assert!(!json.contains("password"));
        assert!(!json.contains("BEGIN OPENSSH"));
        // Round trip preserves the code.
        let back: ErrorDto = serde_json::from_str(&json).expect("deserialize dto");
        assert_eq!(back.code, ErrorCode::AuthenticationFailed);
        assert!(!back.retryable);
    }

    #[test]
    fn cancelled_maps_to_cancelled_code() {
        assert_eq!(AppError::Cancelled.code(), ErrorCode::Cancelled);
    }

    #[test]
    fn host_key_changed_blocks_and_is_not_retryable() {
        let err = AppError::Ssh(SshError::HostKeyChanged {
            host: "vps.example.com".into(),
            port: 22,
        });
        assert_eq!(err.code(), ErrorCode::HostKeyChanged);
        assert!(!err.code().auto_retryable());
        assert!(err.to_dto().message.contains("Host Key"));
    }

    #[test]
    fn each_error_code_has_a_non_empty_message() {
        let cases: Vec<AppError> = vec![
            AppError::Profile(ProfileError::NotFound),
            AppError::Profile(ProfileError::Invalid("x".into())),
            AppError::Profile(ProfileError::Storage("x".into())),
            AppError::Profile(ProfileError::Migration("x".into())),
            AppError::Credential(CredentialError::NotFound),
            AppError::Credential(CredentialError::Backend("x".into())),
            AppError::Forward(ForwardError::ListenerFailed("x".into())),
            AppError::Forward(ForwardError::TargetUnreachable("x".into())),
            AppError::Service(ServiceError::Http("x".into())),
            AppError::Service(ServiceError::Tls("x".into())),
            AppError::Viewer(ViewerError::Other("x".into())),
            AppError::Platform(PlatformError::Unsupported),
            AppError::Cancelled,
            AppError::Ssh(SshError::SessionClosed {
                reason: SessionCloseReason::KeepaliveTimeout,
            }),
            AppError::Ssh(SshError::SessionClosed {
                reason: SessionCloseReason::RemoteDisconnect,
            }),
        ];
        for err in cases {
            assert!(!err.to_dto().message.is_empty(), "{err:?} empty message");
        }
    }

    /// Reliability error codes and retryability (plan §5.6). Transport-side
    /// session drops are auto-retryable; channel/viewer failures are not.
    #[test]
    fn reliability_error_codes_retryability() {
        assert!(ErrorCode::SshKeepaliveTimeout.auto_retryable());
        assert!(ErrorCode::SshTransportClosed.auto_retryable());
        assert!(ErrorCode::NetworkPathChanged.auto_retryable());
        assert!(ErrorCode::ForegroundSessionInvalid.auto_retryable());
        assert!(!ErrorCode::SshChannelOpenFailed.auto_retryable());
        assert!(!ErrorCode::ViewerReloadFailed.auto_retryable());
    }

    /// Every reliability code has a stable snake_case wire name (plan §5.6).
    #[test]
    fn reliability_error_codes_have_stable_wire_names() {
        assert_eq!(
            ErrorCode::SshKeepaliveTimeout.snake_case_name(),
            "ssh_keepalive_timeout"
        );
        assert_eq!(
            ErrorCode::SshTransportClosed.snake_case_name(),
            "ssh_transport_closed"
        );
        assert_eq!(
            ErrorCode::SshChannelOpenFailed.snake_case_name(),
            "ssh_channel_open_failed"
        );
        assert_eq!(
            ErrorCode::NetworkPathChanged.snake_case_name(),
            "network_path_changed"
        );
        assert_eq!(
            ErrorCode::ForegroundSessionInvalid.snake_case_name(),
            "foreground_session_invalid"
        );
        assert_eq!(
            ErrorCode::ViewerReloadFailed.snake_case_name(),
            "viewer_reload_failed"
        );
    }

    /// Mid-session closes map to retryable codes and distinguish keepalive
    /// timeout from other transport closes (plan §5.4 / §5.6).
    #[test]
    fn session_closed_maps_to_retryable_transport_codes() {
        let keepalive = AppError::Ssh(SshError::SessionClosed {
            reason: SessionCloseReason::KeepaliveTimeout,
        });
        assert_eq!(keepalive.code(), ErrorCode::SshKeepaliveTimeout);
        assert!(keepalive.code().auto_retryable());

        let remote = AppError::Ssh(SshError::SessionClosed {
            reason: SessionCloseReason::RemoteDisconnect,
        });
        assert_eq!(remote.code(), ErrorCode::SshTransportClosed);
        assert!(remote.code().auto_retryable());

        let net = AppError::Ssh(SshError::SessionClosed {
            reason: SessionCloseReason::NetworkError,
        });
        assert_eq!(net.code(), ErrorCode::SshTransportClosed);
        assert!(net.code().auto_retryable());
    }

    /// `SessionCloseReason` wire names are stable and non-sensitive.
    #[test]
    fn session_close_reason_wire_names_are_stable() {
        assert_eq!(
            SessionCloseReason::RemoteDisconnect.as_str(),
            "remote_disconnect"
        );
        assert_eq!(
            SessionCloseReason::KeepaliveTimeout.as_str(),
            "keepalive_timeout"
        );
        assert_eq!(SessionCloseReason::NetworkError.as_str(), "network_error");
        assert_eq!(SessionCloseReason::Unknown.as_str(), "unknown");
    }
}
