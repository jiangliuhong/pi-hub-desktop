/**
 * Local runtime command surface (docs/design-v2.md §16). Thin typed wrappers
 * around `invoke`. The frontend never passes arbitrary commands/args/PIDs and
 * never stores process truth locally (V2-SR-001, design-v2 §17.1).
 */

import { callCommand } from "../../lib/tauri";
import type {
  EnvironmentReport,
  InstallationSet,
  LocalRuntimeSettings,
  LocalRuntimeSettingsUpdate,
  LocalRuntimeSnapshot,
  LogLine,
  ValidateInstallationInput,
} from "./types";

export const STATUS_CHANGED_EVENT = "local-runtime://status-changed";

export function getLocalRuntimeStatus(): Promise<LocalRuntimeSnapshot> {
  return callCommand<LocalRuntimeSnapshot>("get_local_runtime_status");
}

export function getLocalRuntimePlatformSupport(): Promise<boolean> {
  return callCommand<boolean>("get_local_runtime_platform_support");
}

export function scanLocalInstallations(): Promise<LocalRuntimeSnapshot> {
  return callCommand<LocalRuntimeSnapshot>("scan_local_installations");
}

export function validateLocalInstallation(
  input: ValidateInstallationInput,
): Promise<InstallationSet> {
  return callCommand<InstallationSet>("validate_local_installation", { input });
}

export function runLocalEnvironmentDoctor(
  force = true,
): Promise<EnvironmentReport> {
  return callCommand<EnvironmentReport>("run_local_environment_doctor", {
    force,
  });
}

export function startLocalPiHub(): Promise<LocalRuntimeSnapshot> {
  return callCommand<LocalRuntimeSnapshot>("start_local_pi_hub");
}

export function stopLocalPiHub(): Promise<LocalRuntimeSnapshot> {
  return callCommand<LocalRuntimeSnapshot>("stop_local_pi_hub");
}

export function restartLocalPiHub(): Promise<LocalRuntimeSnapshot> {
  return callCommand<LocalRuntimeSnapshot>("restart_local_pi_hub");
}

export function getLocalRuntimeSettings(): Promise<LocalRuntimeSettings> {
  return callCommand<LocalRuntimeSettings>("get_local_runtime_settings");
}

export function updateLocalRuntimeSettings(
  input: LocalRuntimeSettingsUpdate,
): Promise<LocalRuntimeSettings> {
  return callCommand<LocalRuntimeSettings>("update_local_runtime_settings", {
    input,
  });
}

export function getLocalRuntimeLogs(limit?: number): Promise<LogLine[]> {
  return callCommand<LogLine[]>("get_local_runtime_logs", { limit });
}

export function clearLocalRuntimeLogs(): Promise<void> {
  return callCommand<void>("clear_local_runtime_logs");
}
