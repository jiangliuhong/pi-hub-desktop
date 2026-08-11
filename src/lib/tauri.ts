import { invoke, type InvokeArgs } from "@tauri-apps/api/core";

/**
 * Thin typed wrapper around Tauri `invoke`.
 *
 * All Trusted App Shell → Rust Core calls go through here so command names and
 * argument shapes stay centralized. Command implementations are added per V1
 * phase; the surface mirrors docs/design-v1.md §13.1.
 *
 * Sensitive values must never be embedded in URLs, logs, or event payloads
 * (AGENTS.md §6.1, §6.4).
 */
export async function callCommand<T>(
  command: string,
  args?: InvokeArgs,
): Promise<T> {
  return invoke<T>(command, args);
}

/**
 * Stable error DTO returned by every Tauri command on failure
 * (docs/design-v1.md §19). `details` only ever carries allowlisted,
 * non-sensitive context.
 */
export interface AppErrorDto {
  code: string;
  message: string;
  retryable: boolean;
  stage?: string;
  details?: Record<string, string>;
}
