/**
 * Cross-feature shared types.
 *
 * Feature-local types live next to their feature (e.g.
 * `features/services/model.ts`). This barrel holds contracts referenced from
 * more than one feature.
 */

/**
 * Non-sensitive diagnostics surfaced on the connection failure page
 * (docs/requirements-v1.md FR-016). Must never contain secrets, Authorization,
 * Cookie, private key material or business data.
 */
export interface ConnectionDiagnostics {
  /** Current connection stage. */
  stage: string | null;
  /** Stable error code (AGENTS.md §9). */
  error_code: string | null;
  ssh_host: string | null;
  ssh_port: number | null;
  target_host: string | null;
  target_port: number | null;
  listener_started: boolean;
  retry_count: number;
}
