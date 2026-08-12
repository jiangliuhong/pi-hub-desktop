import { useState } from "react";
import {
  actionLabel,
  installStateLabel,
  isHighRisk,
  ownershipLabel,
  productDescription,
  productIdLabel,
  sourceLabel,
  updateStatusLabel,
} from "./labels";
import type { PackageErrorDto, ProductAction, ProductStatus } from "./types";

export interface ProductCardProps {
  status: ProductStatus;
  loading: boolean;
  busy: boolean;
  onAction: (
    action: ProductAction,
    product: ProductStatus["product"],
  ) => Promise<void>;
  /** Action error attributed to this product, surfaced in-card. */
  actionError?: PackageErrorDto | null;
}

const PRIMARY_ACTIONS: ProductAction[] = [
  "install",
  "update",
  "repair",
  "cancel",
  "confirm_restart",
];
const MAINTENANCE_ACTIONS: ProductAction[] = ["scan", "check_updates"];

export function ProductCard({
  status,
  loading,
  busy,
  onAction,
  actionError,
}: ProductCardProps) {
  const [pendingAction, setPendingAction] = useState<ProductAction | null>(
    null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [copied, setCopied] = useState(false);
  const current = status.current;
  const actions = status.allowed_actions.filter((action) =>
    PRIMARY_ACTIONS.includes(action),
  );
  const maintenanceActions = status.allowed_actions.filter((action) =>
    MAINTENANCE_ACTIONS.includes(action),
  );
  const packageName =
    current?.package_name ??
    (status.product === "pi"
      ? "@earendil-works/pi-coding-agent"
      : "@jarome/pi-hub");
  const command = `npm install -g ${packageName}@latest --registry=https://registry.npmjs.org`;
  const commandLabel = current ? "升级命令" : "安装命令";

  const handle = (action: ProductAction) => {
    if (isHighRisk(action)) {
      setPendingAction(action);
      return;
    }
    void onAction(action, status.product);
  };

  const confirmPending = async () => {
    if (!pendingAction) return;
    setSubmitting(true);
    try {
      // Awaiting keeps the dialog open (with a loading affordance) until the
      // backend either accepts the operation or the hook records an error.
      // runWithGuard swallows rejections into the error state, so this promise
      // always resolves — failures surface via `actionError` after close.
      await onAction(pendingAction, status.product);
      setPendingAction(null);
    } finally {
      setSubmitting(false);
    }
  };

  const copyCommand = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section className="pkg-card">
      <header className="pkg-card-header">
        <div>
          <h4>{productIdLabel(status.product)}</h4>
          <p className="pkg-description">
            {productDescription(status.product)}
          </p>
        </div>
        <div className="pkg-card-controls">
          <span className={`pkg-state pkg-state-${status.install_state}`}>
            {installStateLabel(status.install_state)}
          </span>
          <div className="pkg-card-actions">
            {maintenanceActions.map((action) => (
              <button
                key={action}
                type="button"
                className="pkg-btn pkg-btn-secondary"
                aria-label={`${productIdLabel(status.product)} ${actionLabel(action)}`}
                disabled={loading || busy}
                onClick={() => handle(action)}
              >
                {actionLabel(action)}
              </button>
            ))}
          </div>
        </div>
      </header>

      <dl className="pkg-summary">
        <SummaryItem label="当前版本" value={currentVersionLabel(status)} />
        <SummaryItem
          label="最新版本"
          value={status.latest_version ? `v${status.latest_version}` : "—"}
          detail={updateStatusLabel(status.update_status)}
        />
        <SummaryItem
          label="来源"
          value={
            current
              ? `${sourceLabel(current.source)} · ${ownershipLabel(current.ownership)}`
              : "—"
          }
        />
      </dl>

      {current ? (
        <details className="pkg-path-details">
          <summary>安装位置</summary>
          <code title={current.entrypoint ?? current.executable}>
            {current.entrypoint ??
              current.executable ??
              current.package_root ??
              "—"}
          </code>
        </details>
      ) : null}

      <div className="pkg-command-section">
        <h5>{commandLabel}</h5>
        <div className="pkg-command-row">
          <code title={command}>{command}</code>
          <button
            type="button"
            className="pkg-copy-btn"
            aria-label={copied ? "已复制" : `复制${commandLabel}`}
            title={copied ? "已复制" : "复制命令"}
            onClick={() => void copyCommand()}
          >
            {copied ? (
              <span className="pkg-copy-success">✓</span>
            ) : (
              <CopyIcon />
            )}
          </button>
          {actions.map((action) => (
            <button
              key={action}
              type="button"
              className={
                action === "cancel" ? "pkg-btn pkg-btn-secondary" : "pkg-btn"
              }
              disabled={
                busy && action !== "cancel" && action !== "confirm_restart"
              }
              onClick={() => handle(action)}
            >
              {actionLabel(action)}
            </button>
          ))}
        </div>
      </div>

      {actions.length === 0 ? (
        <p className="pkg-hint">当前无需安装或升级。</p>
      ) : null}
      {status.issue ? (
        <p className="pkg-issue" role="alert">
          {status.issue.message}
        </p>
      ) : null}
      {actionError ? (
        <p className="pkg-issue" role="alert">
          {actionError.message}
        </p>
      ) : null}

      {pendingAction ? (
        <div
          className="pkg-confirm-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setPendingAction(null);
          }}
        >
          <div className="pkg-confirm" role="alertdialog" aria-modal="true">
            <p>
              确认要{actionLabel(pendingAction)}「
              {productIdLabel(status.product)}」吗？
            </p>
            <p className="pkg-confirm-note">
              该操作会修改当前 Node.js 环境的 npm 全局包。Pi Hub
              由本客户端管理并正在运行时，升级完成后会自动重启。
            </p>
            <div className="pkg-confirm-actions">
              <button
                type="button"
                className="pkg-btn pkg-btn-secondary"
                disabled={submitting}
                onClick={() => setPendingAction(null)}
              >
                取消
              </button>
              <button
                type="button"
                className="pkg-btn"
                disabled={submitting}
                onClick={() => void confirmPending()}
              >
                {submitting ? "处理中…" : "确认"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function SummaryItem({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
      {detail ? <small>{detail}</small> : null}
    </div>
  );
}

function currentVersionLabel(status: ProductStatus): string {
  if (status.current?.version) return `v${status.current.version}`;

  switch (status.install_state) {
    case "not_installed":
      return "未安装";
    case "unknown":
      return "无法验证";
    case "invalid":
      return "安装损坏";
    case "incompatible":
      return "版本不兼容";
    case "installed":
      return "—";
  }
}

function CopyIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="8" y="8" width="11" height="11" rx="2" />
      <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    </svg>
  );
}
