/**
 * Package management settings section (docs/requirements-v3.md §6.1, §6.5).
 *
 * Shown inside the settings dialog on macOS only (iOS hides the entry,
 * requirements-v3 §4.2). Combines: prerequisites, the two product cards, the
 * active operation progress and the Pi Hub "update and restart" confirmation.
 * The UI only renders `allowed_actions` from Rust and never derives
 * permissions (V3 §7.4).
 */

import { ProductCard } from "./ProductCard";
import { stageLabel } from "./labels";
import type { PackageManagementActions } from "./usePackageManagement";
import type {
  PackageErrorDto,
  PackageManagementSnapshot,
  PackageOperationStage,
  ProductAction,
  ProductId,
} from "./types";

export interface PackageManagementSettingsProps {
  snapshot: PackageManagementSnapshot;
  loading: boolean;
  actions: PackageManagementActions;
  /** Last actionable error (global banner fallback). */
  error: PackageErrorDto | null;
  /** Product the {@link error} is attributed to, or null. */
  errorProduct: ProductId | null;
}

export function PackageManagementSettings({
  snapshot,
  loading,
  actions,
  error,
  errorProduct,
}: PackageManagementSettingsProps) {
  const { prerequisites, products, active_operation: activeOp } = snapshot;
  const busy =
    Boolean(activeOp) &&
    activeOp?.stage !== "completed" &&
    activeOp?.stage !== "failed" &&
    activeOp?.stage !== "cancelled";

  const onAction = (
    action: ProductAction,
    product: ProductId,
  ): Promise<void> => {
    switch (action) {
      case "scan":
        return actions.scan(product);
      case "check_updates":
        return actions.checkUpdates(product, true);
      case "install":
        return actions.install(product);
      case "update":
        return actions.update(product);
      case "repair":
        // Repair is modeled as an install of the same/compatible version.
        return actions.install(product);
      case "activate":
        // Activate the managed install for this product (manager resolves it).
        return actions.activate(product);
      case "cancel":
        return activeOp
          ? actions.cancel(activeOp.operation_id)
          : Promise.resolve();
      case "confirm_restart":
        return activeOp
          ? actions.confirmRestart(activeOp.operation_id)
          : Promise.resolve();
    }
  };

  return (
    <div className="settings-card pkg-settings">
      <div className="pkg-section-heading">
        <div>
          <h3>npm 全局组件</h3>
          <p className="settings-description">
            管理当前 Node.js 环境中的 Pi 与 Pi Hub，无需管理员权限。
          </p>
        </div>
        <PrerequisiteSummary prerequisites={prerequisites} />
      </div>

      {error ? (
        <p className="pkg-issue" role="alert">
          {error.message}
        </p>
      ) : null}

      {activeOp ? (
        <OperationProgress stage={activeOp.stage} product={activeOp.product} />
      ) : null}

      <div className="pkg-cards">
        {products.map((p) => (
          <ProductCard
            key={p.product}
            status={p}
            loading={loading}
            busy={busy}
            onAction={onAction}
            actionError={errorProduct === p.product ? error : null}
          />
        ))}
      </div>
    </div>
  );
}

function PrerequisiteSummary({
  prerequisites,
}: {
  prerequisites: PackageManagementSnapshot["prerequisites"];
}) {
  return (
    <div className="pkg-environment" aria-label="npm 运行环境">
      <EnvironmentItem
        label="Node.js"
        satisfied={prerequisites.node.satisfied}
        version={prerequisites.node.version}
        issue={prerequisites.node.issue}
      />
      <EnvironmentItem
        label="npm"
        satisfied={prerequisites.npm.satisfied}
        version={prerequisites.npm.version}
        issue={prerequisites.npm.issue}
      />
    </div>
  );
}

function EnvironmentItem({
  label,
  satisfied,
  version,
  issue,
}: {
  label: string;
  satisfied: boolean;
  version?: string;
  issue?: string;
}) {
  const value = satisfied ? versionLabel(version) : "不可用";
  return (
    <span
      className={`pkg-environment-item${satisfied ? "" : " pkg-environment-item-error"}`}
      title={issue}
    >
      <strong>{label}</strong>
      <span>{value}</span>
    </span>
  );
}

function versionLabel(version?: string): string {
  if (!version) return "可用";
  return version.startsWith("v") ? version : `v${version}`;
}

function OperationProgress({
  stage,
  product,
}: {
  stage: PackageOperationStage;
  product: ProductId;
}) {
  return (
    <div className="pkg-op-progress" role="status" aria-live="polite">
      <span className="pkg-spinner" aria-hidden="true" />
      <span>
        {product === "pi" ? "Pi" : "Pi Hub"} · {stageLabel(stage)}
      </span>
    </div>
  );
}
