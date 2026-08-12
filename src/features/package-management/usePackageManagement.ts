/**
 * React state bridge for package management (docs/requirements-v3.md §12).
 *
 * Loads the snapshot, subscribes to the `status-changed` / `operation-changed`
 * events, and exposes action handlers with loading + error state and
 * anti-double-submit protection (AGENTS.md §11). The UI never derives
 * permissions — it only renders `allowed_actions` from Rust.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  activateManagedProduct,
  cancelPackageOperation,
  checkProductUpdates,
  confirmPiHubUpdateRestart,
  getPackageManagementPlatformSupport,
  getPackageManagementStatus,
  OPERATION_CHANGED_EVENT,
  scanManagedProducts,
  startProductInstall,
  startProductUpdate,
  STATUS_CHANGED_EVENT,
} from "./api";
import type {
  PackageErrorDto,
  PackageManagementSnapshot,
  ProductId,
} from "./types";

const EMPTY: PackageManagementSnapshot = {
  platform_supported: false,
  prerequisites: {
    node: { name: "node", satisfied: false },
    npm: { name: "npm", satisfied: false },
  },
  products: [],
};

export interface PackageManagementActions {
  scan: (product: ProductId) => Promise<void>;
  checkUpdates: (product: ProductId, force?: boolean) => Promise<void>;
  install: (product: ProductId) => Promise<void>;
  update: (product: ProductId) => Promise<void>;
  confirmRestart: (operationId: string) => Promise<void>;
  cancel: (operationId: string) => Promise<void>;
  activate: (product: ProductId) => Promise<void>;
}

export interface UsePackageManagement {
  supported: boolean;
  snapshot: PackageManagementSnapshot;
  loading: boolean;
  /** Last actionable error (cleared on the next successful action). */
  error: PackageErrorDto | null;
  /** Product that triggered {@link error}, so the UI can surface it in-card. */
  errorProduct: ProductId | null;
  actions: PackageManagementActions;
}

/**
 * Drive the package-management snapshot. `disabled` hides the surface on iOS
 * (requirements-v3 §4.2) so the hook short-circuits without invoking commands.
 */
export function usePackageManagement(): UsePackageManagement {
  const [supported, setSupported] = useState(false);
  const [snapshot, setSnapshot] = useState<PackageManagementSnapshot>(EMPTY);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<PackageErrorDto | null>(null);
  const [errorProduct, setErrorProduct] = useState<ProductId | null>(null);
  /** Guard against overlapping async actions per product. */
  const pending = useRef<Set<string>>(new Set());

  useEffect(() => {
    let active = true;
    let unstatus: (() => void) | undefined;
    let unop: (() => void) | undefined;

    void getPackageManagementPlatformSupport()
      .then((ok) => {
        if (active) setSupported(ok);
      })
      .catch(() => undefined);

    if (supported) {
      void getPackageManagementStatus()
        .then((snap) => {
          if (active) setSnapshot(snap);
        })
        .catch(() => undefined);

      void listen<PackageManagementSnapshot>(STATUS_CHANGED_EVENT, (event) => {
        if (active) setSnapshot(event.payload);
      })
        .then((cleanup) => {
          unstatus = cleanup;
        })
        .catch(() => undefined);

      void listen(OPERATION_CHANGED_EVENT, () => {
        // Operations are reflected in the status snapshot; just nudge a refresh.
        void getPackageManagementStatus()
          .then((snap) => {
            if (active) setSnapshot(snap);
          })
          .catch(() => undefined);
      })
        .then((cleanup) => {
          unop = cleanup;
        })
        .catch(() => undefined);
    }

    return () => {
      active = false;
      unstatus?.();
      unop?.();
    };
  }, [supported]);

  const runWithGuard = useCallback(
    async <T>(
      key: string,
      fn: () => Promise<T>,
      product?: ProductId,
    ): Promise<T | undefined> => {
      if (pending.current.has(key)) return undefined;
      pending.current.add(key);
      setLoading(true);
      setError(null);
      setErrorProduct(null);
      try {
        const result = await fn();
        // Refresh the snapshot after any mutation.
        const snap = await getPackageManagementStatus();
        setSnapshot(snap);
        return result;
      } catch (raw) {
        setError(toErrorDto(raw));
        setErrorProduct(product ?? null);
        return undefined;
      } finally {
        pending.current.delete(key);
        setLoading(false);
      }
    },
    [],
  );

  const actions: PackageManagementActions = {
    scan: (product) =>
      runWithGuard(
        `scan:${product}`,
        () => scanManagedProducts(product),
        product,
      ).then(() => undefined),
    checkUpdates: (product, force = true) =>
      runWithGuard(
        `check:${product}`,
        () => checkProductUpdates(product, force),
        product,
      ).then(() => undefined),
    install: (product) =>
      runWithGuard(
        `install:${product}`,
        async () => {
          const token = releaseTokenFor(snapshot, product);
          if (!token) {
            // Need a fresh check before install is actionable.
            const snap = await checkProductUpdates(product, true);
            const tok = releaseTokenFor(snap, product);
            if (!tok) throw noTokenError(product);
            return startProductInstall(product, tok);
          }
          return startProductInstall(product, token);
        },
        product,
      ).then(() => undefined),
    update: (product) =>
      runWithGuard(
        `update:${product}`,
        async () => {
          const token = releaseTokenFor(snapshot, product);
          if (!token) {
            const snap = await checkProductUpdates(product, true);
            const tok = releaseTokenFor(snap, product);
            if (!tok) throw noTokenError(product);
            return startProductUpdate(product, tok);
          }
          return startProductUpdate(product, token);
        },
        product,
      ).then(() => undefined),
    confirmRestart: (operationId) =>
      runWithGuard(
        `confirm:${operationId}`,
        () => confirmPiHubUpdateRestart(operationId),
        snapshot.active_operation?.product,
      ).then(() => undefined),
    cancel: (operationId) =>
      runWithGuard(
        `cancel:${operationId}`,
        () => cancelPackageOperation(operationId),
        snapshot.active_operation?.product,
      ).then(() => undefined),
    activate: (product) =>
      runWithGuard(
        `activate:${product}`,
        () => activateManagedProduct(product),
        product,
      ).then(() => undefined),
  };

  return { supported, snapshot, loading, error, errorProduct, actions };
}

function releaseTokenFor(
  snapshot: PackageManagementSnapshot,
  product: ProductId,
): string | undefined {
  return snapshot.products.find((p) => p.product === product)?.release_token;
}

function noTokenError(product: ProductId): unknown {
  return {
    code: "package_release_check_failed",
    message: `无法获取 ${product === "pi" ? "Pi" : "Pi Hub"} 的最新版本信息，请检查网络后重试。`,
    retryable: true,
  };
}

function toErrorDto(raw: unknown): PackageErrorDto {
  if (raw && typeof raw === "object") {
    const e = raw as Partial<PackageErrorDto>;
    return {
      code: typeof e.code === "string" ? e.code : "internal",
      message:
        typeof e.message === "string" ? e.message : "操作失败，请稍后重试。",
      retryable: Boolean(e.retryable),
      stage: e.stage,
      details: e.details,
    };
  }
  return {
    code: "internal",
    message: "操作失败，请稍后重试。",
    retryable: false,
  };
}
