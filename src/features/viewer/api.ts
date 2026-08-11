/**
 * Service View + file dialog command surface (docs/design-v1.md §13.1, §14).
 *
 * The Service View loads untrusted remote Pi Hub content and receives zero
 * Tauri capability (AGENTS.md §6.4). These helpers wrap the trusted App Shell
 * commands; the remote page never reaches them.
 */

import { callCommand } from "../../lib/tauri";

export interface OpenServiceViewResponse {
  service_id: string;
  allowed_origin: string;
}

/** Open the isolated Service View for a service (returns the allowlisted origin). */
export function openServiceView(
  serviceId: string,
  effectiveUrl: string,
): Promise<OpenServiceViewResponse> {
  return callCommand<OpenServiceViewResponse>("open_service_view", {
    request: { service_id: serviceId, effective_url: effectiveUrl },
  });
}

/** Close the Service View for a service. */
export function closeServiceView(serviceId: string): Promise<void> {
  return callCommand<void>("close_service_view", { serviceId });
}
