import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import {
  getLocalRuntimeStatus,
  restartLocalPiHub,
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
      setError(toMessage(e));
      // Re-sync from the server (it recorded the failure).
      void reload();
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
  const installation = snapshot?.installation;
  const piHubVersion = installation?.pi_hub?.version;
  const nodeVersion = installation?.node?.version;

  return (
    <section className="local-runtime-card" aria-label="本机 Pi Hub">
      <header className="local-runtime-header">
        <h2>This Mac</h2>
        <span className={`badge badge-${snapshot?.runtime_state ?? "unknown"}`}>
          {runtimeStateLabel(state)}
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

      <div className="local-runtime-actions">
        {isRunning ? (
          <button
            type="button"
            onClick={() => {
              window.open(snapshot?.effective_url, "_blank", "noopener");
            }}
          >
            打开
          </button>
        ) : null}
        {snapshot?.runtime_state === "stopped" ||
        snapshot?.runtime_state === "failed" ? (
          <button
            type="button"
            disabled={busy || envBlocked}
            title={envBlocked ? "存在阻断项，无法启动" : undefined}
            onClick={() => void run(startLocalPiHub, "starting")}
          >
            启动
          </button>
        ) : null}
        {isManaged ? (
          <>
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(stopLocalPiHub, "stopping")}
            >
              停止
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(restartLocalPiHub, "stopping")}
            >
              重启
            </button>
          </>
        ) : null}
        <Link to="/local-runtime/environment">查看检查结果</Link>
        <Link to="/local-runtime/settings">设置</Link>
        <Link to="/local-runtime/logs">日志</Link>
      </div>
    </section>
  );
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}
