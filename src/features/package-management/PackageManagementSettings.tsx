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
  PackageManagementSnapshot,
  PackageOperationStage,
  ProductAction,
  ProductId,
} from "./types";

export interface PackageManagementSettingsProps {
  snapshot: PackageManagementSnapshot;
  loading: boolean;
  actions: PackageManagementActions;
  errorText: string | null;
}

export function PackageManagementSettings({
  snapshot,
  loading,
  actions,
  errorText,
}: PackageManagementSettingsProps) {
  const { prerequisites, products, active_operation: activeOp } = snapshot;
  const busy =
    Boolean(activeOp) &&
    activeOp?.stage !== "completed" &&
    activeOp?.stage !== "failed" &&
    activeOp?.stage !== "cancelled";

  const onAction = (action: ProductAction, product: ProductId) => {
    switch (action) {
      case "scan":
        void actions.scan();
        break;
      case "check_updates":
        void actions.checkUpdates(true);
        break;
      case "install":
        void actions.install(product);
        break;
      case "update":
        void actions.update(product);
        break;
      case "repair":
        // Repair is modeled as an install of the same/compatible version.
        void actions.install(product);
        break;
      case "activate":
        // Activate the managed install for this product (manager resolves it).
        void actions.activate(product);
        break;
      case "cancel":
        if (activeOp) void actions.cancel(activeOp.operation_id);
        break;
      case "confirm_restart":
        if (activeOp) void actions.confirmRestart(activeOp.operation_id);
        break;
    }
  };

  return (
    <div className="settings-card">
      <h3>本机组件</h3>
      <p className="settings-description">
        分别检测并管理本机 Pi 与 Pi Hub。安装或更新只写入 Desktop
        受管目录，不会修改 Homebrew / NVM / Volta
        等外部安装，也不会请求管理员权限。
      </p>

      <div className="pkg-prereqs">
        <PrereqItem
          label="Node.js"
          ok={prerequisites.node.satisfied}
          detail={prerequisites.node.version}
          issue={prerequisites.node.issue}
        />
        <PrereqItem
          label="npm"
          ok={prerequisites.npm.satisfied}
          detail={prerequisites.npm.version}
          issue={prerequisites.npm.issue}
        />
      </div>

      {errorText ? (
        <p className="pkg-issue" role="alert">
          {errorText}
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
            busy={busy}
            onAction={onAction}
          />
        ))}
      </div>

      <div className="pkg-global-actions">
        <button
          type="button"
          className="pkg-btn pkg-btn-secondary"
          disabled={loading || busy}
          onClick={() => void actions.scan()}
        >
          重新扫描
        </button>
        <button
          type="button"
          className="pkg-btn pkg-btn-secondary"
          disabled={loading || busy}
          onClick={() => void actions.checkUpdates(true)}
        >
          检查更新
        </button>
      </div>
    </div>
  );
}

function PrereqItem({
  label,
  ok,
  detail,
  issue,
}: {
  label: string;
  ok: boolean;
  detail?: string;
  issue?: string;
}) {
  return (
    <div className={`pkg-prereq${ok ? "" : " pkg-prereq-bad"}`}>
      <span className="pkg-prereq-symbol">{ok ? "✓" : "✕"}</span>
      <span className="pkg-prereq-label">{label}</span>
      {detail ? <span className="pkg-prereq-detail">{detail}</span> : null}
      {!ok && issue ? <span className="pkg-prereq-issue">{issue}</span> : null}
    </div>
  );
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
