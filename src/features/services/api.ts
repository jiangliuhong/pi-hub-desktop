/**
 * Service profile command surface.
 *
 * Wraps the Rust commands in docs/design-v1.md §13.1
 * (`list_services`, `get_service`, `create_service`, `update_service`,
 * `delete_service`). Profiles only ever carry credential *references* — never
 * secrets (AGENTS.md §6.1).
 */

import { callCommand } from "../../lib/tauri";
import type {
  ConnectionType,
  SshAuthType,
  ServiceScheme,
  ServiceProfile,
} from "./model";

/** DTO sent to `create_service` / `update_service` (mirrors `ProfileDraft`). */
export type ProfileDraft =
  | {
      connection_type: "direct_url";
      name: string;
      base_url: string;
      pi_hub_credential_id?: string | null;
    }
  | {
      connection_type: "ssh_forward";
      name: string;
      ssh_host: string;
      ssh_port: number;
      ssh_username: string;
      ssh_auth_type: SshAuthType;
      ssh_credential_id: string;
      target_host?: string;
      target_port?: number;
      service_scheme?: ServiceScheme;
      service_base_path?: string;
      pi_hub_credential_id?: string | null;
    };

interface ProfileMetadataDto {
  id: string;
  name: string;
  pi_hub_credential_id?: string | null;
  created_at: string;
  updated_at: string;
}

interface DirectUrlDto extends ProfileMetadataDto {
  connection_type: "direct_url";
  base_url: string;
}

interface SshForwardDto extends ProfileMetadataDto {
  connection_type: "ssh_forward";
  ssh_host: string;
  ssh_port: number;
  ssh_username: string;
  ssh_auth_type: SshAuthType;
  ssh_credential_id: string;
  target_host: string;
  target_port: number;
  service_scheme: ServiceScheme;
  service_base_path: string;
}

/** Typed response mirroring the Rust `ServiceProfileDto`. */
export type ServiceProfileDto = (DirectUrlDto | SshForwardDto) & {
  schema_version: number;
};

export function listServices(): Promise<ServiceProfile[]> {
  return callCommand<ServiceProfileDto[]>("list_services").then((dtos) =>
    dtos.map((d) => unwrapProfile(d)),
  );
}

export function getService(id: string): Promise<ServiceProfile> {
  return callCommand<ServiceProfileDto>("get_service", { id }).then(
    unwrapProfile,
  );
}

export function createService(draft: ProfileDraft): Promise<ServiceProfile> {
  return callCommand<ServiceProfileDto>("create_service", { draft }).then(
    unwrapProfile,
  );
}

export function updateService(
  id: string,
  draft: ProfileDraft,
): Promise<ServiceProfile> {
  return callCommand<ServiceProfileDto>("update_service", {
    payload: { id, ...draft } as unknown as Record<string, unknown>,
  }).then(unwrapProfile);
}

export function deleteService(id: string): Promise<void> {
  return callCommand<void>("delete_service", { id });
}

/**
 * Widen the typed DTO into the frontend discriminated union. Because the DTO is
 * itself discriminated on `connection_type`, this is a safe narrowing.
 */
function unwrapProfile(dto: ServiceProfileDto): ServiceProfile {
  const metadata: ServiceProfile["metadata"] = {
    id: dto.id,
    schema_version: dto.schema_version,
    name: dto.name,
    pi_hub_credential_id: dto.pi_hub_credential_id ?? null,
    created_at: dto.created_at,
    updated_at: dto.updated_at,
  };
  if (dto.connection_type === "direct_url") {
    return {
      metadata,
      connection_type: "direct_url",
      base_url: dto.base_url,
    };
  }
  return {
    metadata,
    connection_type: "ssh_forward",
    ssh_host: dto.ssh_host,
    ssh_port: dto.ssh_port,
    ssh_username: dto.ssh_username,
    ssh_auth_type: dto.ssh_auth_type,
    ssh_credential_id: dto.ssh_credential_id,
    target_host: dto.target_host,
    target_port: dto.target_port,
    service_scheme: dto.service_scheme,
    service_base_path: dto.service_base_path,
  };
}

export type { ConnectionType };
