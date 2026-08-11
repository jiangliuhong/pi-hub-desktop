/**
 * Credential command surface (docs/design-v1.md §13.1, §10).
 *
 * Wraps `put_credential` / `delete_credential`. Secrets are sent to Rust, stored
 * to Keychain, and the caller must immediately clear its input state
 * (AGENTS.md §6.1). Nothing here persists or logs secrets (SR-005).
 */

import { callCommand } from "../../lib/tauri";

export type CredentialKind =
  "ssh-password" | "ssh-private-key" | "ssh-key-passphrase" | "pi-hub-password";

export function putCredential(
  credentialId: string,
  kind: CredentialKind,
  secret: string,
): Promise<{ credential_id: string }> {
  return callCommand<{ credential_id: string }>("put_credential", {
    request: { credential_id: credentialId, kind, secret },
  });
}

export function deleteCredential(
  credentialId: string,
  kind: CredentialKind,
): Promise<void> {
  return callCommand<void>("delete_credential", { credentialId, kind });
}
