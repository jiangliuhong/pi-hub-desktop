/**
 * Pure validation helpers shared across service forms and the connection layer.
 *
 * These functions intentionally contain no secrets and no I/O, so they can be
 * unit tested without a Tauri or network environment
 * (docs/requirements-v1.md SR-007, docs/design-v1.md §22.1).
 */

export type ValidationResult = { ok: true } | { ok: false; reason: string };

/** Allowed schemes for a Direct URL service. */
const DIRECT_URL_SCHEMES = ["http:", "https:"] as const;

/**
 * Validate a Direct URL profile's base URL.
 *
 * - Must be parseable.
 * - Scheme must be `http` or `https`.
 * - Host is required (no scheme-relative or bare-path URLs).
 */
export function validateDirectUrl(input: string): ValidationResult {
  const value = input.trim();
  if (value.length === 0) {
    return { ok: false, reason: "URL 不能为空" };
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return { ok: false, reason: "URL 格式无效" };
  }

  if (
    !DIRECT_URL_SCHEMES.includes(
      url.protocol as (typeof DIRECT_URL_SCHEMES)[number],
    )
  ) {
    return { ok: false, reason: "仅支持 http 或 https" };
  }

  if (!url.hostname) {
    return { ok: false, reason: "URL 缺少主机名" };
  }

  return { ok: true };
}

/**
 * Whether a Direct URL is a plaintext (HTTP) connection that needs the one-time
 * safety warning (FR-005). Loopback HTTP is still plaintext but the warning is
 * the same.
 */
export function isPlaintextDirectUrl(input: string): boolean {
  try {
    return new URL(input.trim()).protocol === "http:";
  } catch {
    return false;
  }
}

/**
 * Validate an SSH or target TCP port (SR-007).
 *
 * Port 0 is rejected as an explicit port value; it is only valid for *local
 * listener* allocation, which is handled by the Rust side and never accepted
 * from user input (AGENTS.md §6.3).
 */
export function validatePort(input: number): ValidationResult {
  if (!Number.isInteger(input)) {
    return { ok: false, reason: "端口必须是整数" };
  }
  if (input < 1 || input > 65535) {
    return { ok: false, reason: "端口必须在 1-65535 范围内" };
  }
  return { ok: true };
}

/** Validate an SSH username (non-empty after trim) (SR-007). */
export function validateSshUsername(input: string): ValidationResult {
  if (input.trim().length === 0) {
    return { ok: false, reason: "SSH 用户名不能为空" };
  }
  return { ok: true };
}

/** Validate an SSH host (non-empty after trim) (SR-007). */
export function validateHost(input: string): ValidationResult {
  if (input.trim().length === 0) {
    return { ok: false, reason: "主机不能为空" };
  }
  return { ok: true };
}

/** Validate a service display name (non-empty after trim). */
export function validateName(input: string): ValidationResult {
  if (input.trim().length === 0) {
    return { ok: false, reason: "名称不能为空" };
  }
  return { ok: true };
}

/** Validate an OpenSSH private key PEM (basic shape check; full decode is Rust-side). */
export function validatePrivateKeyPem(input: string): ValidationResult {
  const value = input.trim();
  if (value.length === 0) {
    return { ok: false, reason: "私钥不能为空" };
  }
  if (!value.startsWith("-----BEGIN ") || !value.includes("PRIVATE KEY-----")) {
    return { ok: false, reason: "私钥看起来不是 OpenSSH 格式" };
  }
  return { ok: true };
}
