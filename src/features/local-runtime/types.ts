/**
 * V2 local runtime DTO types — mirror the Rust `LocalRuntimeSnapshot` and
 * related models (docs/design-v2.md §4, §5, §8). These carry no secrets; the
 * Rust side is the single source of truth (design-v2 §17.1).
 */

export type InstallationState =
  "unknown" | "not_found" | "invalid" | "incompatible" | "ready";

export type LocalRuntimeState =
  | "unknown"
  | "checking"
  | "stopped"
  | "starting"
  | "running_managed"
  | "running_external"
  | "stopping"
  | "port_conflict"
  | "failed";

export type EnvironmentStatus = "ready" | "degraded" | "blocked" | "unknown";

export type InstallationSource =
  | "persisted"
  | "path"
  | "homebrew"
  | "nvm"
  | "volta"
  | "fnm"
  | "asdf"
  | "mise"
  | "manual";

export type CheckCategory =
  | "runtime"
  | "pi_hub"
  | "pi_environment"
  | "auth_and_models"
  | "optional_tools";

export type CheckSeverity = "required" | "recommended" | "informational";

export type CheckStatus = "pass" | "warn" | "fail" | "skipped";

export interface NodeInstallation {
  executable: string;
  canonical_executable: string;
  version: string;
  source: InstallationSource;
}

export interface PiHubInstallation {
  package_root: string;
  entrypoint: string;
  version: string;
  node_requirement: string;
  source: InstallationSource;
}

export interface PiCliInstallation {
  executable: string;
  version?: string;
  kind: "npm" | "standalone" | "unknown";
  source: InstallationSource;
}

export interface InstallationSet {
  node?: NodeInstallation;
  pi_hub?: PiHubInstallation;
  pi_cli?: PiCliInstallation;
}

export interface CheckResult {
  id: string;
  category: CheckCategory;
  severity: CheckSeverity;
  status: CheckStatus;
  code?: string;
  message?: string;
  remediation?: string;
  details?: Record<string, unknown>;
}

export interface EnvironmentReport {
  overall: EnvironmentStatus;
  generated_at?: string;
  checks: CheckResult[];
}

export interface ManagedProcessSummary {
  pid: number;
  started_at: string;
  ready_at?: string;
  node_executable: string;
  pi_hub_entrypoint: string;
  port: number;
}

export interface LocalRuntimeSnapshot {
  installation_state: InstallationState;
  runtime_state: LocalRuntimeState;
  environment: EnvironmentReport;
  installation?: InstallationSet;
  managed_process?: ManagedProcessSummary;
  effective_url?: string;
  last_error?: LocalRuntimeErrorDto;
  checked_at?: string;
}

/** V2 error DTO with allowlisted details (mirrors `crate::error::ErrorDto`). */
export interface LocalRuntimeErrorDto {
  code: string;
  message: string;
  retryable: boolean;
  stage?: string;
  details?: Record<string, string>;
}

export interface LocalRuntimeSettings {
  schema_version: number;
  port: number;
  auto_start_on_app_launch: boolean;
  stop_managed_on_app_exit: boolean;
  node_executable?: string;
  pi_hub_entrypoint?: string;
  pi_hub_package_root?: string;
  pi_agent_dir?: string;
  pi_hub_credential_id?: string;
  auto_start_failures?: string[];
}

/** Partial update DTO accepted by `update_local_runtime_settings`. */
export interface LocalRuntimeSettingsUpdate {
  port?: number;
  auto_start_on_app_launch?: boolean;
  stop_managed_on_app_exit?: boolean;
  node_executable?: string;
  pi_hub_entrypoint?: string;
  pi_hub_package_root?: string;
  pi_agent_dir?: string;
  pi_hub_credential_id?: string;
}

export interface ValidateInstallationInput {
  node_executable?: string;
  pi_hub_entrypoint?: string;
  pi_hub_package_root?: string;
}

export interface LogLine {
  timestamp: string;
  stream: "stdout" | "stderr";
  text: string;
}
