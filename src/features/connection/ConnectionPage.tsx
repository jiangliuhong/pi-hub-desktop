import { useEffect, useState } from "react";
import { useNavigate, useParams, Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import {
  connectService,
  getConnectionStatus,
  respondHostKeyChallenge,
  STATE_CHANGED_EVENT,
  type ConnectResult,
  type HostKeyChallengeDto,
  type StateChangedPayload,
} from "./api";
import { connectionStateLabel } from "./model";
import { HostKeyDialog } from "./HostKeyDialog";
import type { AppErrorDto } from "../../lib/tauri";
import type { ConnectionDiagnostics } from "../../types";

/**
 * Connection progress / diagnostics page (docs/requirements-v1.md §8.4,
 * FR-006/FR-009).
 *
 * Drives the Rust connect flow and surfaces the host-key challenge when SSH
 * presents an unknown key (FR-007). The Rust manager is the single source of
 * truth; the UI only reflects what it returns (AGENTS.md §5.3).
 */
export function ConnectionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [state, setState] = useState<string>("idle");
  const [error, setError] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<ConnectionDiagnostics | null>(
    null,
  );
  const [challenge, setChallenge] = useState<HostKeyChallengeDto | null>(null);
  const [phase, setPhase] = useState<"idle" | "connecting" | "done">("idle");

  // Kick off the connect once on mount.
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    const run = async () => {
      setPhase("connecting");
      setState("validating");
      try {
        const result = await connectService(id);
        if (cancelled) return;
        handleResult(result);
      } catch (e) {
        if (cancelled) return;
        fail(e);
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  // Reliability (plan §5.5.1): reflect Rust-driven state changes — including
  // `reconnecting` — so the user sees the real connection state rather than a
  // stale local guess. The Rust ConnectionManager is the source of truth.
  useEffect(() => {
    if (!id) return;
    const unlisten = listen<StateChangedPayload>(
      STATE_CHANGED_EVENT,
      (event) => {
        if (event.payload.service_id !== id) return;
        // Only adopt Rust-driven states that aren't already handled locally.
        // `reconnecting` / `error` during a live connection surface here; the
        // happy-path `connected` is handled by the connect result handler.
        setState(event.payload.state);
      },
    );
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [id]);

  const handleResult = (result: ConnectResult) => {
    if (result.kind === "host_key_challenge") {
      setChallenge(result.payload);
      setState("verifying_host_key");
      return;
    }
    setState("connected");
    setPhase("done");
    if (id) {
      void navigate(`/viewer/${id}`, { replace: true });
    }
  };

  const fail = (e: unknown) => {
    const dto = e as AppErrorDto | undefined;
    setError(dto?.message ?? "连接失败。");
    setState("error");
    setPhase("idle");
    // Best-effort: pull non-sensitive diagnostics from the manager.
    if (id) {
      void getConnectionStatus(id)
        .then((s) => {
          if (s) setDiagnostics(s.diagnostics);
        })
        .catch(() => undefined);
    }
  };

  const respondChallenge = async (accept: boolean) => {
    if (!challenge) return;
    setChallenge(null);
    setState("connecting_ssh");
    try {
      const result = await respondHostKeyChallenge(
        challenge.challenge_id,
        accept,
      );
      handleResult(result);
    } catch (e) {
      fail(e);
    }
  };

  const retry = async () => {
    if (!id) return;
    setError(null);
    setPhase("connecting");
    setState("validating");
    try {
      handleResult(await connectService(id));
    } catch (e) {
      fail(e);
    }
  };

  return (
    <div className="connection-page">
      <h2>正在连接…</h2>
      <p className="state-label">
        当前阶段：{connectionStateLabel(state as never)}
      </p>

      {error ? (
        <div role="alert" className="error-banner">
          {error}
          <div className="row-gap">
            <button
              type="button"
              onClick={() => void retry()}
              disabled={phase === "connecting"}
            >
              重试
            </button>
            {id ? <Link to={`/services/${id}/edit`}>编辑配置</Link> : null}
            <Link to="/">返回列表</Link>
          </div>
        </div>
      ) : null}

      {diagnostics ? <DiagnosticsPanel diagnostics={diagnostics} /> : null}

      {challenge ? (
        <HostKeyDialog
          challenge={challenge}
          onRespond={(a) => void respondChallenge(a)}
        />
      ) : null}
    </div>
  );
}

function DiagnosticsPanel({
  diagnostics,
}: {
  diagnostics: ConnectionDiagnostics;
}) {
  return (
    <details className="advanced">
      <summary>诊断信息</summary>
      <dl className="diagnostics">
        <dt>阶段</dt>
        <dd>{diagnostics.stage ?? "—"}</dd>
        <dt>错误码</dt>
        <dd>{diagnostics.error_code ?? "—"}</dd>
        <dt>SSH Host</dt>
        <dd>
          {diagnostics.ssh_host ?? "—"}:{diagnostics.ssh_port ?? "—"}
        </dd>
        <dt>Target</dt>
        <dd>
          {diagnostics.target_host ?? "—"}:{diagnostics.target_port ?? "—"}
        </dd>
        <dt>已建立 Listener</dt>
        <dd>{diagnostics.listener_started ? "是" : "否"}</dd>
        <dt>重试次数</dt>
        <dd>{diagnostics.retry_count}</dd>
        <dt>重连次数</dt>
        <dd>{diagnostics.reconnect_count}</dd>
        {diagnostics.last_close_reason ? (
          <>
            <dt>上次断开原因</dt>
            <dd>{diagnostics.last_close_reason}</dd>
          </>
        ) : null}
      </dl>
      <p className="hint">
        诊断信息只包含非敏感数据（FR-016）。本地随机端口不在此展示。
      </p>
    </details>
  );
}
