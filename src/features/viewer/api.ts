/**
 * Service View command surface.
 *
 * Wraps the Rust commands in docs/design-v1.md §13.1 (`open_service_view`,
 * `close_service_view`). The Service View loads untrusted remote Pi Hub
 * content and must receive zero Tauri capability (AGENTS.md §6.4). Real
 * lifecycle management lands in a later phase once the Phase 0 Service View
 * spike is complete.
 */

/** Open the isolated Service View for a given service. Stubbed for init. */
export function openServiceView(serviceId: string): Promise<void> {
  void serviceId;
  return Promise.resolve();
}

/** Close the Service View for a given service. Stubbed for init. */
export function closeServiceView(serviceId: string): Promise<void> {
  void serviceId;
  return Promise.resolve();
}
