/**
 * A single product card (Pi / Pi Hub) in the package-management settings
 * (docs/requirements-v3.md §6.2, §18.2). Renders only the `allowed_actions`
 * computed by Rust; high-risk actions ask for a second confirmation
 * (AGENTS.md §11). Never shows shell commands or secrets (V3-SR-001/005).
 */

import { useState } from "react";
import {
  actionLabel,
  installStateLabel,
  isHighRisk,
  ownershipLabel,
  productIdLabel,
  productDescription,
  sourceLabel,
  updateStatusLabel,
} from "./labels";
import type { ProductAction, ProductStatus } from "./types";

export interface ProductCardProps {
  status: ProductStatus;
  /** A global operation is in flight (disables non-cancel actions). */
  busy: boolean;
  onAction: (action: ProductAction, product: ProductStatus["product"]) => void;
}

export function ProductCard({ status, busy, onAction }: ProductCardProps) {
  const [pendingAction, setPendingAction] = useState<ProductAction | null>(
    null,
  );
  const current = status.current;
  const allowed = status.allowed_actions;

  const handle = (action: ProductAction) => {
    if (isHighRisk(action)) {
      setPendingAction(action);
      return;
    }
    onAction(action, status.product);
  };

  const confirmPending = () => {
    if (pendingAction) onAction(pendingAction, status.product);
    setPendingAction(null);
  };

  return (
    <section className="pkg-card">
      <header className="pkg-card-header">
        <h4>{productIdLabel(status.product)}</h4>
        <span className={`pkg-state pkg-state-${status.install_state}`}>
          {installStateLabel(status.install_state)}
        </span>
      </header>
      <p className="pkg-description">{productDescription(status.product)}</p>

      <dl className="pkg-facts">
        <div>
          <dt>当前版本</dt>
          <dd>{current?.version ?? "—"}</dd>
        </div>
        <div>
          <dt>最新版本</dt>
          <dd>{status.latest_version ?? "—"}</dd>
        </div>
        <div>
          <dt>更新状态</dt>
          <dd>{updateStatusLabel(status.update_status)}</dd>
        </div>
        {current ? (
          <>
            <div>
              <dt>来源</dt>
              <dd>
                {sourceLabel(current.source)}（
                {ownershipLabel(current.ownership)}）
              </dd>
            </div>
            <div>
              <dt>位置</dt>
              <dd
                className="pkg-path"
                title={current.entrypoint ?? current.package_root}
              >
                {current.entrypoint ?? current.package_root ?? "—"}
              </dd>
            </div>
          </>
        ) : null}
      </dl>

      {status.issue ? (
        <p className="pkg-issue" role="alert">
          {status.issue.message}
        </p>
      ) : null}

      <div className="pkg-actions">
        {allowed.length === 0 ? (
          <span className="pkg-hint">无可用操作</span>
        ) : (
          allowed.map((action) => (
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
          ))
        )}
      </div>

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
              该操作不会修改你的外部安装；将安装 Desktop
              受管副本并在验证后切换。
            </p>
            <div className="pkg-confirm-actions">
              <button
                type="button"
                className="pkg-btn pkg-btn-secondary"
                onClick={() => setPendingAction(null)}
              >
                取消
              </button>
              <button
                type="button"
                className="pkg-btn"
                onClick={confirmPending}
              >
                确认
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  );
}
