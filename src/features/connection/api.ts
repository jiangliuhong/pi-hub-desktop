/**
 * Connection command surface (docs/design-v1.md §13.1).
 *
 * Wraps the Rust connection commands. The Rust ConnectionManager is the single
 * source of truth for connection state; the UI only reflects what these calls
 * return (AGENTS.md §5.3).
 */

import { callCommand } from "../../lib/tauri";
import type { ConnectionState } from "./model";
import type { ConnectionDiagnostics } from "../../types";

/**
 * Event names pushed from the Rust ConnectionManager to the App Shell
 * (plan-remote-pi-hub-performance §5.5). Tauri v2 does not allowlist event
 * names, so these require no capability entry.
 */
export const STATE_CHANGED_EVENT = "connection://state-changed";
export const DIAGNOSTICS_UPDATED_EVENT = "connection://diagnostics-updated";

/** Non-sensitive payload for `connection://state-changed`. */
export interface StateChangedPayload {
  service_id: string;
  state: ConnectionState;
  effective_url?: string;
}

/** Non-sensitive payload for `connection://diagnostics-updated`. */
export interface DiagnosticsPayload extends ConnectionDiagnostics {
  service_id: string;
}

/** Result of `connect_service`: ready, or a host-key confirmation (FR-007). */
export type ConnectResult =
  | { kind: "connected"; effective_url: string }
  | { kind: "host_key_challenge"; payload: HostKeyChallengeDto };

export interface HostKeyChallengeDto {
  challenge_id: string;
  connection_id: string;
  service_id: string;
  ssh_host: string;
  ssh_port: number;
  algorithm: string;
  sha256_fingerprint: string;
}

export interface ConnectionStatusDto {
  state: ConnectionState;
  effective_url: string | null;
  diagnostics: ConnectionDiagnostics;
}

export function connectService(serviceId: string): Promise<ConnectResult> {
  return callCommand<ConnectResult>("connect_service", { serviceId });
}

export function respondHostKeyChallenge(
  challengeId: string,
  accept: boolean,
): Promise<ConnectResult> {
  return callCommand<ConnectResult>("respond_host_key_challenge", {
    request: { challenge_id: challengeId, accept },
  });
}

export function disconnectService(serviceId: string): Promise<void> {
  return callCommand<void>("disconnect_service", { serviceId });
}

export function getConnectionStatus(
  serviceId: string,
): Promise<ConnectionStatusDto | null> {
  return callCommand<ConnectionStatusDto | null>("get_connection_status", {
    serviceId,
  });
}

export function replaceKnownHostAndConnect(
  serviceId: string,
): Promise<ConnectResult> {
  return callCommand<ConnectResult>("replace_known_host_and_connect", {
    serviceId,
  });
}
