/**
 * Package management command surface (docs/requirements-v3.md §12; design §14).
 * Thin typed wrappers around `invoke`. The frontend never passes package
 * names, version specs, commands, args, PIDs or paths — only the fixed product
 * enum, an opaque release token and UUIDs (V3-SR-001).
 */

import { callCommand } from "../../lib/tauri";
import type {
  PackageManagementSnapshot,
  PackageOperationDto,
  PackageOperationLogLine,
  ProductId,
} from "./types";

export const STATUS_CHANGED_EVENT = "package-management://status-changed";
export const OPERATION_CHANGED_EVENT = "package-management://operation-changed";

export function getPackageManagementPlatformSupport(): Promise<boolean> {
  return callCommand<boolean>("get_package_management_platform_support");
}

export function getPackageManagementStatus(): Promise<PackageManagementSnapshot> {
  return callCommand<PackageManagementSnapshot>(
    "get_package_management_status",
  );
}

export function scanManagedProducts(): Promise<PackageManagementSnapshot> {
  return callCommand<PackageManagementSnapshot>("scan_managed_products");
}

export function checkProductUpdates(
  force = true,
): Promise<PackageManagementSnapshot> {
  return callCommand<PackageManagementSnapshot>("check_product_updates", {
    force,
  });
}

export function startProductInstall(
  product: ProductId,
  releaseToken: string,
): Promise<PackageOperationDto> {
  return callCommand<PackageOperationDto>("start_product_install", {
    input: { product, release_token: releaseToken },
  });
}

export function startProductUpdate(
  product: ProductId,
  releaseToken: string,
): Promise<PackageOperationDto> {
  return callCommand<PackageOperationDto>("start_product_update", {
    input: { product, release_token: releaseToken },
  });
}

export function confirmPiHubUpdateRestart(
  operationId: string,
): Promise<PackageOperationDto> {
  return callCommand<PackageOperationDto>("confirm_pi_hub_update_restart", {
    operationId,
  });
}

export function cancelPackageOperation(operationId: string): Promise<void> {
  return callCommand<void>("cancel_package_operation", { operationId });
}

export function activateManagedProduct(
  product: ProductId,
): Promise<PackageManagementSnapshot> {
  return callCommand<PackageManagementSnapshot>("activate_managed_product", {
    product,
  });
}

export function getPackageOperationLog(
  operationId: string,
  limit?: number,
): Promise<PackageOperationLogLine[]> {
  return callCommand<PackageOperationLogLine[]>("get_package_operation_log", {
    operationId,
    limit,
  });
}
