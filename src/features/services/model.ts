/**
 * Service profile contract (Trusted App Shell side).
 *
 * Mirrors the Rust tagged enum in docs/design-v1.md §6.1. `direct_url` and
 * `ssh_forward` are a discriminated union — never expressed as a bag of
 * nullable fields (AGENTS.md §11). Sensitive values live only in Keychain and
 * are referenced by id.
 *
 * NOTE: this module only declares the type contract for the scaffold. CRUD
 * behavior, persistence and validation wiring land in V1 Phase 1.
 */

/** V1 connection kinds. */
export type ConnectionType = "direct_url" | "ssh_forward";

/** SSH authentication kinds. */
export type SshAuthType = "password" | "private_key";

/** HTTP scheme used to reach Pi Hub through a forward. */
export type ServiceScheme = "http" | "https";

/** Fields shared by every service profile. */
export interface ProfileMetadata {
  id: string;
  schema_version: number;
  name: string;
  /** Keychain credential id for optional Pi Hub HTTP auth. */
  pi_hub_credential_id: string | null;
  created_at: string;
  updated_at: string;
}

/** Direct URL service profile. */
export interface DirectUrlProfile {
  metadata: ProfileMetadata;
  connection_type: "direct_url";
  base_url: string;
}

/** SSH Local Port Forward service profile. */
export interface SshForwardProfile {
  metadata: ProfileMetadata;
  connection_type: "ssh_forward";
  ssh_host: string;
  ssh_port: number;
  ssh_username: string;
  ssh_auth_type: SshAuthType;
  /** Keychain credential id for the SSH secret. */
  ssh_credential_id: string;
  target_host: string;
  target_port: number;
  service_scheme: ServiceScheme;
  service_base_path: string;
}

/** Discriminated union of all V1 service profiles. */
export type ServiceProfile = DirectUrlProfile | SshForwardProfile;
