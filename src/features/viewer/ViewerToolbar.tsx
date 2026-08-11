import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { closeServiceView, openServiceView } from "./api";
import { disconnectService, getConnectionStatus } from "../connection/api";
import { connectionStateLabel } from "../connection/model";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import type { AppErrorDto } from "../../lib/tauri";

/**
 * Trusted toolbar over the untrusted Service View
 * (docs/requirements-v1.md §8.5, FR-012).
 *
 * Provides return-to-list, status, refresh and disconnect controls. These
 * controls are App Shell UI only — the remote Pi Hub page cannot reach them
 * (AGENTS.md §6.4). The actual Service WebView window is opened via the Rust
 * viewer commands and remains capability-isolated.
 */
export function ViewerToolbar() {
  const { id } = useParams<{ id: string }>();
  const [status, setStatus] = useState<string>("idle");
  const [allowedOrigin, setAllowedOrigin] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    const run = async () => {
      try {
        const s = await getConnectionStatus(id);
        if (cancelled) return;
        if (s?.effective_url) {
          setStatus(s.state);
          const resp = await openServiceView(id, s.effective_url);
          if (cancelled) return;
          setAllowedOrigin(resp.allowed_origin);
        } else {
          setError("当前没有活动连接，请返回列表重新连接。");
        }
      } catch (e) {
        const dto = e as AppErrorDto | undefined;
        if (!cancelled) setError(dto?.message ?? "打开 Pi Hub 失败。");
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [id]);

  const disconnect = async () => {
    if (!id) return;
    try {
      await disconnectService(id);
      await closeServiceView(id);
      setStatus("disconnected");
    } catch (e) {
      const dto = e as AppErrorDto | undefined;
      setError(dto?.message ?? "断开失败。");
    }
  };

  if (error) {
    return (
      <PagePlaceholder title="无法打开 Pi Hub">
        <p>{error}</p>
        <Link to="/">返回服务列表</Link>
      </PagePlaceholder>
    );
  }

  return (
    <div className="viewer-toolbar">
      <div className="viewer-controls">
        <Link to="/">← 返回列表</Link>
        <span className="state-label">
          连接状态：{connectionStateLabel(status as never)}
        </span>
        {allowedOrigin ? (
          <span className="allowed-origin" title="允许的 origin">
            {allowedOrigin}
          </span>
        ) : null}
        <button type="button" onClick={() => window.location.reload()}>
          刷新
        </button>
        <button
          type="button"
          className="danger"
          onClick={() => void disconnect()}
        >
          断开 / 重连
        </button>
      </div>
      <p className="hint">
        Service WebView 加载的是远端内容，按不可信处理，不具备任何 Tauri
        原生权限 （AGENTS.md §6.4）。
      </p>
    </div>
  );
}
