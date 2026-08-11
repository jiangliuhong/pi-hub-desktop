import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import {
  connectService,
  getConnectionStatus,
  respondHostKeyChallenge,
  type ConnectResult,
  type HostKeyChallengeDto,
} from "./api";
import { connectionStateLabel } from "./model";
import { HostKeyDialog } from "./HostKeyDialog";
import { PagePlaceholder } from "../../components/PagePlaceholder";
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

  const handleResult = (result: ConnectResult) => {
    if (result.kind === "host_key_challenge") {
      setChallenge(result.payload);
      setState("verifying_host_key");
      return;
    }
    setState("connected");
    setPhase("done");
    // The Service View opens in the toolbar surface; here we just confirm.
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

      {state === "connected" && !error ? (
        <PagePlaceholder title="已连接">
          <p>Pi Hub 服务已就绪。</p>
          <div className="row-gap">
            <Link to={`/viewer/${id ?? ""}`}>打开 Pi Hub</Link>
            <Link to="/">返回列表</Link>
          </div>
        </PagePlaceholder>
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
      </dl>
      <p className="hint">
        诊断信息只包含非敏感数据（FR-016）。本地随机端口不在此展示。
      </p>
    </details>
  );
}
