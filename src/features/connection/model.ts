/**
 * Connection state machine contract (Trusted App Shell side).
 *
 * Mirrors docs/design-v1.md §12.3 and docs/requirements-v1.md FR-009. States
 * are an explicit enum — never scattered strings (AGENTS.md §9). The Rust
 * ConnectionManager is the single source of truth; the UI only reflects the
 * states and events it emits.
 */

/** Stable error codes surfaced across the Tauri boundary (AGENTS.md §9). */
export type ConnectionErrorCode =
  | "invalid_profile"
  | "dns_failed"
  | "ssh_connect_timeout"
  | "host_key_unknown"
  | "host_key_changed"
  | "authentication_failed"
  | "private_key_invalid"
  | "private_key_passphrase_required"
  | "target_unreachable"
  | "local_listener_failed"
  | "service_http_error"
  | "tls_error"
  | "cancelled"
  | "unsupported_platform"
  // Connection reliability codes (plan-remote-pi-hub-performance §5.6).
  | "ssh_keepalive_timeout"
  | "ssh_transport_closed"
  | "ssh_channel_open_failed"
  | "network_path_changed"
  | "foreground_session_invalid"
  | "viewer_reload_failed";

/** Connection lifecycle states. */
export type ConnectionState =
  | "idle"
  | "validating"
  | "connecting_ssh"
  | "verifying_host_key"
  | "authenticating"
  | "opening_forward"
  | "checking_service"
  | "connected"
  | "reconnecting"
  | "disconnecting"
  | "disconnected"
  | "error";

/** Human-readable label for a connection state. */
export function connectionStateLabel(state: ConnectionState): string {
  switch (state) {
    case "idle":
      return "未连接";
    case "validating":
      return "校验配置";
    case "connecting_ssh":
      return "连接 SSH";
    case "verifying_host_key":
      return "等待 Host Key 确认";
    case "authenticating":
      return "SSH 认证";
    case "opening_forward":
      return "建立映射";
    case "checking_service":
      return "检查 Pi Hub";
    case "connected":
      return "已连接";
    case "reconnecting":
      return "重新连接";
    case "disconnecting":
      return "正在断开";
    case "disconnected":
      return "已断开";
    case "error":
      return "连接失败";
  }
}
