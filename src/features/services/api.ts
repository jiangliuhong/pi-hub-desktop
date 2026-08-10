/**
 * Service profile command surface.
 *
 * Wraps the Rust commands listed in docs/design-v1.md §13.1
 * (`list_services`, `get_service`, `create_service`, `update_service`,
 * `delete_service`). Implementations are added in V1 Phase 1.
 */

import type { ServiceProfile } from "./model";

/**
 * Placeholder service list.
 *
 * The real implementation will call `callCommand<ServiceProfile[]>(
 * "list_services")`. Until Phase 1 wires up the Rust ProfileStore, this returns
 * an empty list so the UI renders its "no services yet" state deterministically
 * instead of fabricating data (FR-001 forbids showing fake "Online" state).
 */
export function listServices(): Promise<ServiceProfile[]> {
  return Promise.resolve([]);
}
