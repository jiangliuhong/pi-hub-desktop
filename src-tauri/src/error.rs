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
            | ErrorCode::Io => true,
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
            | ErrorCode::Internal => false,
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
            SshError::Transport(_) => ErrorCode::Internal,
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
        ];
        for err in cases {
            assert!(!err.to_dto().message.is_empty(), "{err:?} empty message");
        }
    }
}
