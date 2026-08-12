/**
 * V3 package management DTO types — mirror the Rust `PackageManagementSnapshot`
 * and related models (docs/requirements-v3.md §7, §12). No secrets; the Rust
 * side is the single source of truth (design §14.1, §17.1).
 *
 * The frontend never derives permissions: `allowed_actions` is computed by
 * Rust and the UI only renders those actions (requirements-v3 §7.4).
 */

export type ProductId = "pi" | "pi_hub";

export type ProductInstallState =
  "unknown" | "not_installed" | "installed" | "invalid" | "incompatible";

export type InstallOwnership = "desktop_managed" | "external";

export type PackageKind = "npm" | "standalone" | "unknown";

export type InstallationSource =
  | "persisted"
  | "path"
  | "homebrew"
  | "nvm"
  | "volta"
  | "fnm"
  | "asdf"
  | "mise"
  | "manual"
  | "npm_global"
  | "desktop_managed";

export type UpdateStatus =
  | "unknown"
  | "checking"
  | "up_to_date"
  | "available"
  | "newer_than_latest"
  | "unavailable";

export type PackageOperationKind = "install" | "update" | "repair" | "activate";

export type PackageOperationStage =
  | "preparing"
  | "fetching_metadata"
  | "installing"
  | "verifying"
  | "awaiting_restart_confirmation"
  | "activating"
  | "restarting"
  | "rolling_back"
  | "completed"
  | "cancelled"
  | "failed";

export type ProductAction =
  | "scan"
  | "check_updates"
  | "install"
  | "update"
  | "repair"
  | "activate"
  | "cancel"
  | "confirm_restart";

export type PackageLogLevel = "info" | "warn" | "error";

export interface ProductInstallationDto {
  installation_id: string;
  package_name: string;
  version?: string;
  executable?: string;
  package_root?: string;
  entrypoint?: string;
  source: InstallationSource;
  ownership: InstallOwnership;
  kind?: PackageKind;
}

export interface ProductPrerequisite {
  name: string;
  satisfied: boolean;
  version?: string;
  location?: string;
  issue?: string;
}

export interface PackagePrerequisites {
  node: ProductPrerequisite;
  npm: ProductPrerequisite;
}

export interface PackageIssueDto {
  code: string;
  message: string;
}

export interface ProductStatus {
  product: ProductId;
  install_state: ProductInstallState;
  current?: ProductInstallationDto;
  alternatives: ProductInstallationDto[];
  update_status: UpdateStatus;
  latest_version?: string;
  last_update_check_at?: string;
  release_token?: string;
  allowed_actions: ProductAction[];
  issue?: PackageIssueDto;
}

export interface PackageOperationDto {
  operation_id: string;
  product: ProductId;
  kind: PackageOperationKind;
  stage: PackageOperationStage;
  from_version?: string;
  target_version?: string;
  started_at: string;
  can_cancel: boolean;
  issue?: PackageIssueDto;
}

export interface PackageManagementSnapshot {
  platform_supported: boolean;
  prerequisites: PackagePrerequisites;
  products: ProductStatus[];
  active_operation?: PackageOperationDto;
  checked_at?: string;
}

export interface PackageOperationLogLine {
  timestamp: string;
  stage: PackageOperationStage;
  level: PackageLogLevel;
  text: string;
}

/** Stable error DTO (mirrors `crate::error::ErrorDto`). */
export interface PackageErrorDto {
  code: string;
  message: string;
  retryable: boolean;
  stage?: string;
  details?: Record<string, string>;
}
