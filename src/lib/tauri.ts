import { invoke } from "@tauri-apps/api/core";

/**
 * Thin typed wrapper around Tauri `invoke`.
 *
 * All Trusted App Shell → Rust Core calls go through here so command names and
 * argument shapes stay centralized. Command implementations are added per V1
 * phase; the list below documents the intended surface
 * (docs/design-v1.md §13.1).
 *
 * Sensitive values must never be embedded in URLs, logs, or event payloads
 * (AGENTS.md §6.1, §6.4).
 */
export async function callCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(command, args);
}
