/**
 * Credential client-side model helpers (docs/design-v1.md §10).
 *
 * Credential ids are stable, non-secret UUIDs generated client-side and used as
 * Keychain references. The id itself never carries a secret (AGENTS.md §6.1).
 */

/** Generate a fresh credential id (UUID v4). */
export function generateCredentialId(): string {
  return crypto.randomUUID();
}
