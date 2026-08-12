import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import {
  getLocalRuntimeStatus,
  restartLocalPiHub,
  scanLocalInstallations,
  startLocalPiHub,
  stopLocalPiHub,
  STATUS_CHANGED_EVENT,
} from "./api";
import { environmentStatusLabel, isBusy, runtimeStateLabel } from "./labels";
import type { LocalRuntimeSnapshot, LocalRuntimeState } from "./types";
import type { AppErrorDto } from "../../lib/tauri";

/**
 * The fixed `This Mac` card at the top of the macOS service list
 * (requirements-v2 §9 V2-FR-001, §13.1).
 *
 * It is NOT a normal DirectUrlProfile and never mixes with remote service
 * configs (design-v2 §3). The card only reflects Rust-side truth; optimistic
 * local state is never trusted for process existence (design-v2 §17.3).
 */
export function LocalRuntimeCard() {
  const navigate = useNavigate();
  const [snapshot, setSnapshot] = useState<LocalRuntimeSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<LocalRuntimeState | null>(null);

  const reload = useCallback(async () => {
    try {
      setSnapshot(await getLocalRuntimeStatus());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
    // Server-driven refresh (design-v2 §17.2 / §17.3).
    const unlistenP = listen<LocalRuntimeSnapshot>(
      STATUS_CHANGED_EVENT,
      (e) => {
        setSnapshot(e.payload);
      },
    );
    return () => {
      void unlistenP.then((fn) => fn());
    };
  }, [reload]);

  const run = async (
    fn: () => Promise<LocalRuntimeSnapshot>,
    busy: LocalRuntimeState,
  ) => {
    if (pending) return;
    setPending(busy);
    setError(null);
    try {
      setSnapshot(await fn());
    } catch (e) {
      const dto = toErrorDto(e);
      const message = dto?.message ?? "发生未知错误。";
      // Re-sync from Rust, which records domain failures in `last_error`.
      // When both surfaces describe the same error, render only the persisted
      // error so the card does not show duplicate banners.
      try {
        const next = await getLocalRuntimeStatus();
        setSnapshot(next);
        setError(
          dto?.code && next.last_error?.code === dto.code ? null : message,
        );
      } catch {
        setError(message);
      }
    } finally {
      setPending(null);
    }
  };

  const state = pending ?? snapshot?.runtime_state ?? "unknown";
  const busy = isBusy(state);
  const isManaged = snapshot?.runtime_state === "running_managed";
  const isRunning =
    snapshot?.runtime_state === "running_managed" ||
    snapshot?.runtime_state === "running_external";
  const envBlocked = snapshot?.environment?.overall === "blocked";
  // Detection gate for Start: node + pi_hub both detected (AGENTS.md §8.1 —
  // external `pi` CLI is informational, not a hard dependency).
  const installationReady = snapshot?.installation_state === "ready";
  const installation = snapshot?.installation;
  const piHubVersion = installation?.pi_hub?.version;
  const nodeVersion = installation?.node?.version;

  return (
    <section className="local-runtime-card" aria-label="本机 Pi Hub">
      <header className="local-runtime-header">
        <h2>This Mac</h2>
        <span
          className={`badge badge-${snapshot?.runtime_state ?? "unknown"}`}
          aria-live="polite"
        >
          <span className="badge-dot" aria-hidden="true" />
          {runtimeStateLabel(state, snapshot?.last_error?.code)}
        </span>
      </header>

      <div className="local-runtime-meta">
        {piHubVersion ? <span>Pi Hub {piHubVersion}</span> : null}
        {nodeVersion ? <span>Node {nodeVersion}</span> : null}
        {snapshot?.effective_url ? (
          <span className="mono">{snapshot.effective_url}</span>
        ) : null}
      </div>

      {snapshot?.environment ? (
        <div className="local-runtime-env">
          环境：
          <span className={`env-${snapshot.environment.overall}`}>
            {environmentStatusLabel(snapshot.environment.overall)}
          </span>
        </div>
      ) : null}

      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}

      {snapshot?.last_error ? (
        <div role="alert" className="error-banner local-runtime-last-error">
          <div className="local-runtime-last-error-head">
            <span>{snapshot.last_error.message}</span>
            {snapshot.last_error.code === "pi_hub_doctor_blocked" ? (
              <Link to="/local-runtime/environment">查看检查结果</Link>
            ) : (
              <Link to="/local-runtime/logs">查看日志</Link>
            )}
          </div>
          {snapshot.last_error.details?.recentOutput ? (
            <pre className="local-runtime-recent-output">
              {snapshot.last_error.details.recentOutput}
            </pre>
          ) : null}
        </div>
      ) : null}

      <div className="local-runtime-actions">
        <button
          type="button"
          className="local-action local-action-subtle"
          disabled={busy}
          title="检测本机 Node.js / Pi Hub 安装与端口状态"
          onClick={() => void run(scanLocalInstallations, "checking")}
        >
          检测安装
        </button>
        {isRunning ? (
          <button
            type="button"
            className="local-action local-action-primary"
            onClick={() => void navigate("/local-runtime/viewer")}
          >
            连接
          </button>
        ) : null}
        {snapshot?.runtime_state === "stopped" ||
        snapshot?.runtime_state === "failed" ? (
          <button
            type="button"
            className="local-action local-action-primary"
            disabled={busy || envBlocked || !installationReady}
            title={
              envBlocked
                ? "存在阻断项，无法启动"
                : !installationReady
                  ? "未检测到可用的 Pi Hub，请先点检测"
                  : undefined
            }
            onClick={() => void run(startLocalPiHub, "starting")}
          >
            启动
          </button>
        ) : null}
        {isManaged ? (
          <>
            <button
              type="button"
              className="local-action"
              disabled={busy}
              onClick={() => void run(stopLocalPiHub, "stopping")}
            >
              停止
            </button>
            <button
              type="button"
              className="local-action"
              disabled={busy}
              onClick={() => void run(restartLocalPiHub, "stopping")}
            >
              重启
            </button>
          </>
        ) : null}
        <span className="local-actions-divider" aria-hidden="true" />
        <Link
          className="local-action local-action-link"
          to="/local-runtime/environment"
        >
          查看检查结果
        </Link>
        <Link
          className="local-action local-action-link"
          to="/local-runtime/settings"
        >
          设置
        </Link>
        <Link
          className="local-action local-action-link"
          to="/local-runtime/logs"
        >
          日志
        </Link>
      </div>
    </section>
  );
}

function toErrorDto(e: unknown): AppErrorDto | undefined {
  if (!e || typeof e !== "object") return undefined;
  const dto = e as Partial<AppErrorDto>;
  return typeof dto.code === "string" && typeof dto.message === "string"
    ? (dto as AppErrorDto)
    : undefined;
}

function toMessage(e: unknown): string {
  return toErrorDto(e)?.message ?? "发生未知错误。";
}
