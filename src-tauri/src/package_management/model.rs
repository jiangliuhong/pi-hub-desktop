//! V3 package management models (docs/requirements-v3.md §7, §8, §12;
//! design §8, §12).
//!
//! All of these are safe to serialize across the Tauri boundary: no secrets,
//! no npm stderr, no full environment, no registry raw output (AGENTS.md §6.1,
//! V3-SR-005).

use crate::error::PackageManagementError;
use crate::local_runtime::model::InstallationSource;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// ---- fixed product → package mapping (design §3, §9.3) ----

/// Fixed npm package name for a product (design §3.1/§3.2). The frontend can
/// never supply a package name — this is the single source of truth.
pub fn package_name(product: ProductId) -> &'static str {
    match product {
        ProductId::Pi => "@earendil-works/pi-coding-agent",
        ProductId::PiHub => "@jarome/pi-hub",
    }
}

/// Fixed CLI bin entry name for a product (design §8.4, §17.4).
pub fn bin_name(product: ProductId) -> &'static str {
    match product {
        ProductId::Pi => "pi",
        ProductId::PiHub => "pi-hub",
    }
}

// ---- enums ----

/// The two independently-managed products (design §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductId {
    Pi,
    PiHub,
}

impl ProductId {
    pub fn api_name(self) -> &'static str {
        match self {
            ProductId::Pi => "pi",
            ProductId::PiHub => "pi_hub",
        }
    }

    /// All products in canonical order (Pi before Pi Hub for one-click install).
    pub fn all() -> &'static [ProductId] {
        &[ProductId::Pi, ProductId::PiHub]
    }
}

/// Per-product installation lifecycle state (requirements-v3 §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductInstallState {
    Unknown,
    NotInstalled,
    Installed,
    Invalid,
    Incompatible,
}

impl ProductInstallState {
    pub fn api_name(self) -> &'static str {
        match self {
            ProductInstallState::Unknown => "unknown",
            ProductInstallState::NotInstalled => "not_installed",
            ProductInstallState::Installed => "installed",
            ProductInstallState::Invalid => "invalid",
            ProductInstallState::Incompatible => "incompatible",
        }
    }
}

/// Whether an installation is owned by Desktop or external (design §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallOwnership {
    DesktopManaged,
    External,
}

impl InstallOwnership {
    pub fn api_name(self) -> &'static str {
        match self {
            InstallOwnership::DesktopManaged => "desktop_managed",
            InstallOwnership::External => "external",
        }
    }
}

/// How a Pi CLI was packaged (design §8.3). Pi Hub is always `Npm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Npm,
    Standalone,
    Unknown,
}

/// Update availability vs. the registry `latest` (requirements-v3 §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Unknown,
    Checking,
    UpToDate,
    Available,
    NewerThanLatest,
    Unavailable,
}

impl UpdateStatus {
    pub fn api_name(self) -> &'static str {
        match self {
            UpdateStatus::Unknown => "unknown",
            UpdateStatus::Checking => "checking",
            UpdateStatus::UpToDate => "up_to_date",
            UpdateStatus::Available => "available",
            UpdateStatus::NewerThanLatest => "newer_than_latest",
            UpdateStatus::Unavailable => "unavailable",
        }
    }
}

/// Operation kind (requirements-v3 §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageOperationKind {
    Install,
    Update,
    Repair,
    Activate,
}

impl PackageOperationKind {
    pub fn api_name(self) -> &'static str {
        match self {
            PackageOperationKind::Install => "install",
            PackageOperationKind::Update => "update",
            PackageOperationKind::Repair => "repair",
            PackageOperationKind::Activate => "activate",
        }
    }
}

/// Operation stage (requirements-v3 §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageOperationStage {
    Preparing,
    FetchingMetadata,
    Installing,
    Verifying,
    AwaitingRestartConfirmation,
    Activating,
    Restarting,
    RollingBack,
    Completed,
    Cancelled,
    Failed,
}

impl PackageOperationStage {
    pub fn api_name(self) -> &'static str {
        match self {
            PackageOperationStage::Preparing => "preparing",
            PackageOperationStage::FetchingMetadata => "fetching_metadata",
            PackageOperationStage::Installing => "installing",
            PackageOperationStage::Verifying => "verifying",
            PackageOperationStage::AwaitingRestartConfirmation => "awaiting_restart_confirmation",
            PackageOperationStage::Activating => "activating",
            PackageOperationStage::Restarting => "restarting",
            PackageOperationStage::RollingBack => "rolling_back",
            PackageOperationStage::Completed => "completed",
            PackageOperationStage::Cancelled => "cancelled",
            PackageOperationStage::Failed => "failed",
        }
    }

    /// Whether an operation in this stage can be cancelled (design §13.2).
    pub fn cancellable(self) -> bool {
        matches!(
            self,
            PackageOperationStage::Preparing
                | PackageOperationStage::FetchingMetadata
                | PackageOperationStage::Installing
                | PackageOperationStage::Verifying
        )
    }

    /// Whether the stage is terminal (no further transitions).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            PackageOperationStage::Completed
                | PackageOperationStage::Cancelled
                | PackageOperationStage::Failed
        )
    }
}

/// UI action allowlist (requirements-v3 §7.4, §12). Computed by Rust only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductAction {
    Scan,
    CheckUpdates,
    Install,
    Update,
    Repair,
    Activate,
    Cancel,
    ConfirmRestart,
}

impl ProductAction {
    pub fn api_name(self) -> &'static str {
        match self {
            ProductAction::Scan => "scan",
            ProductAction::CheckUpdates => "check_updates",
            ProductAction::Install => "install",
            ProductAction::Update => "update",
            ProductAction::Repair => "repair",
            ProductAction::Activate => "activate",
            ProductAction::Cancel => "cancel",
            ProductAction::ConfirmRestart => "confirm_restart",
        }
    }
}

// ---- internal facts (not serialized directly) ----

/// Frozen release metadata fetched from the registry (design §9).
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub product: ProductId,
    pub version: semver::Version,
    pub node_engine: Option<String>,
    pub integrity: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
}

/// A short-lived, opaque-to-frontend release handle. The frontend only ever
/// sees `ReleaseToken::id` as an opaque string; the manager redeems it from an
/// in-memory registry it issued itself, so it cannot be forged or tampered
/// with (design §9.3, §14.1).
#[derive(Debug, Clone)]
pub struct ReleaseToken {
    pub id: Uuid,
    pub product: ProductId,
    pub version: semver::Version,
    pub node_engine: Option<String>,
    pub integrity: Option<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ReleaseToken {
    /// Token validity window (design §9.3 "短期有效").
    pub const TTL_SECS: i64 = 600;

    pub fn new(info: ReleaseInfo) -> Self {
        let now = Utc::now();
        ReleaseToken {
            id: Uuid::new_v4(),
            product: info.product,
            version: info.version,
            node_engine: info.node_engine,
            integrity: info.integrity,
            issued_at: now,
            expires_at: now + chrono::Duration::seconds(Self::TTL_SECS),
        }
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now > self.expires_at
    }
}

// ---- DTOs serialized across the Tauri boundary ----

/// A discovered installation, surfaced to the UI (design §8.1). No secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInstallationDto {
    /// Stable id derived from the canonical location (so activate/switch can
    /// reference the same install across rescans). Not a credential.
    pub installation_id: String,
    pub package_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<PathBuf>,
    pub source: InstallationSource,
    pub ownership: InstallOwnership,
    /// Pi only; Pi Hub is always Npm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<PackageKind>,
}

/// A single prerequisite (Node / npm) status (design §6.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductPrerequisite {
    pub name: String,
    pub satisfied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}

/// Aggregated prerequisites (requirements-v3 §8.2/§8.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagePrerequisites {
    pub node: ProductPrerequisite,
    pub npm: ProductPrerequisite,
}

impl Default for PackagePrerequisites {
    fn default() -> Self {
        PackagePrerequisites {
            node: ProductPrerequisite {
                name: "node".into(),
                satisfied: false,
                version: None,
                location: None,
                issue: None,
            },
            npm: ProductPrerequisite {
                name: "npm".into(),
                satisfied: false,
                version: None,
                location: None,
                issue: None,
            },
        }
    }
}

/// A non-sensitive issue attached to a product (design §6.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIssueDto {
    pub code: String,
    pub message: String,
}

/// Per-product status in the snapshot (requirements-v3 §12).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductStatus {
    pub product: ProductId,
    pub install_state: ProductInstallState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<ProductInstallationDto>,
    #[serde(default)]
    pub alternatives: Vec<ProductInstallationDto>,
    pub update_status: UpdateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_update_check_at: Option<DateTime<Utc>>,
    /// Opaque token to pass back for install/update (design §9.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_token: Option<String>,
    pub allowed_actions: Vec<ProductAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<PackageIssueDto>,
}

/// The active operation surfaced to the UI (requirements-v3 §12).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageOperationDto {
    pub operation_id: Uuid,
    pub product: ProductId,
    pub kind: PackageOperationKind,
    pub stage: PackageOperationStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    pub started_at: DateTime<Utc>,
    pub can_cancel: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<PackageIssueDto>,
}

/// The full non-sensitive snapshot published to the UI (requirements-v3 §12).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManagementSnapshot {
    pub platform_supported: bool,
    pub prerequisites: PackagePrerequisites,
    pub products: Vec<ProductStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_operation: Option<PackageOperationDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<DateTime<Utc>>,
}

impl Default for PackageManagementSnapshot {
    fn default() -> Self {
        PackageManagementSnapshot {
            platform_supported: crate::package_management::platform_supported(),
            prerequisites: PackagePrerequisites::default(),
            products: ProductId::all()
                .iter()
                .map(|p| ProductStatus {
                    product: *p,
                    install_state: ProductInstallState::Unknown,
                    current: None,
                    alternatives: Vec::new(),
                    update_status: UpdateStatus::Unknown,
                    latest_version: None,
                    last_update_check_at: None,
                    release_token: None,
                    allowed_actions: Vec::new(),
                    issue: None,
                })
                .collect(),
            active_operation: None,
            checked_at: None,
        }
    }
}

/// A single bounded install/update operation log line (design §18).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageOperationLogLine {
    pub timestamp: DateTime<Utc>,
    pub stage: PackageOperationStage,
    pub level: PackageLogLevel,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageLogLevel {
    Info,
    Warn,
    Error,
}

/// Derive a stable installation id from a canonical path (design §8.2). Uses a
/// short SHA-256 prefix so the same physical install keeps the same id across
/// rescans, while different locations differ.
pub fn installation_id_from_path(canonical: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    let mut out = String::with_capacity(12);
    for b in hash.iter().take(6) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Map a package error to the snapshot issue DTO (non-sensitive).
pub fn issue_from_error(err: &PackageManagementError) -> PackageIssueDto {
    PackageIssueDto {
        code: err.code().snake_case_name().to_string(),
        message: err.user_message_for_issue(),
    }
}

/// Internal helper trait so the model module can build issue messages without
/// reaching into the private `user_message`. Kept small + non-sensitive.
impl PackageManagementError {
    pub fn user_message_for_issue(&self) -> String {
        // Reuse the AppError user_message path indirectly via a short summary.
        match self {
            PackageManagementError::UnsupportedPlatform => "当前平台不支持。".into(),
            PackageManagementError::OperationInProgress => "已有操作进行中。".into(),
            PackageManagementError::NodeUnavailable => "Node.js 不可用。".into(),
            PackageManagementError::NpmUnavailable => "npm 不可用。".into(),
            PackageManagementError::ReleaseCheckFailed { product } => {
                format!("无法获取 {product} 版本信息。")
            }
            PackageManagementError::ReleaseInvalid { product } => {
                format!("{product} 版本元数据无效。")
            }
            PackageManagementError::ReleaseTokenExpired => "版本选择已过期。".into(),
            PackageManagementError::InstallSpawnFailed => "npm 启动失败。".into(),
            PackageManagementError::InstallFailed { product, .. } => {
                format!("{product} 安装失败，未改动现有安装。")
            }
            PackageManagementError::InstallTimeout { product } => {
                format!("{product} 安装超时。")
            }
            PackageManagementError::VerificationFailed { product, .. } => {
                format!("{product} 校验失败，未激活。")
            }
            PackageManagementError::ActivationFailed { product, .. } => {
                format!("{product} 激活失败。")
            }
            PackageManagementError::UpdateRequiresRestart => "Pi Hub 更新需确认重启。".into(),
            PackageManagementError::ExternalRuntimeActive => "外部 Pi Hub 运行中。".into(),
            PackageManagementError::RollbackFailed { product, .. } => {
                format!("{product} 回滚失败。")
            }
            PackageManagementError::Cancelled => "操作已取消。".into(),
            PackageManagementError::DiskSpaceInsufficient { .. } => "磁盘空间不足。".into(),
            PackageManagementError::Internal(_) => "内部错误。".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_are_fixed() {
        assert_eq!(
            package_name(ProductId::Pi),
            "@earendil-works/pi-coding-agent"
        );
        assert_eq!(package_name(ProductId::PiHub), "@jarome/pi-hub");
        assert_eq!(bin_name(ProductId::Pi), "pi");
        assert_eq!(bin_name(ProductId::PiHub), "pi-hub");
    }

    #[test]
    fn product_ids_round_trip() {
        let json = serde_json::to_string(&ProductId::PiHub).unwrap();
        assert_eq!(json, "\"pi_hub\"");
        let back: ProductId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ProductId::PiHub);
    }

    #[test]
    fn installation_id_is_stable_and_distinct() {
        let a = installation_id_from_path(std::path::Path::new("/x/y/pi"));
        let a2 = installation_id_from_path(std::path::Path::new("/x/y/pi"));
        let b = installation_id_from_path(std::path::Path::new("/x/z/pi"));
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert_eq!(a.len(), 12);
    }

    #[test]
    fn cancellable_stages_match_design() {
        assert!(PackageOperationStage::Installing.cancellable());
        assert!(!PackageOperationStage::Activating.cancellable());
        assert!(!PackageOperationStage::AwaitingRestartConfirmation.cancellable());
    }

    #[test]
    fn release_token_expires() {
        let info = ReleaseInfo {
            product: ProductId::Pi,
            version: semver::Version::new(0, 84, 0),
            node_engine: None,
            integrity: None,
            published_at: None,
        };
        let tok = ReleaseToken::new(info);
        assert!(!tok.is_expired(Utc::now()));
        let later = Utc::now() + chrono::Duration::seconds(ReleaseToken::TTL_SECS + 1);
        assert!(tok.is_expired(later));
    }

    #[test]
    fn snapshot_default_has_both_products_unknown() {
        let snap = PackageManagementSnapshot::default();
        assert_eq!(snap.products.len(), 2);
        assert_eq!(snap.products[0].install_state, ProductInstallState::Unknown);
    }
}
