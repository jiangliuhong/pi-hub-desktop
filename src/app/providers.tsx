import type { ReactNode } from "react";

/**
 * Global provider seam for the Trusted App Shell.
 *
 * V1 does not yet require a state manager; this wrapper keeps a stable place
 * to mount future global context (e.g. connection status subscription) without
 * scattering provider nesting across the app.
 */
export function AppProviders({ children }: { children: ReactNode }) {
  return <>{children}</>;
}
