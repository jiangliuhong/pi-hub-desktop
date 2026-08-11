import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { clearLocalRuntimeLogs, getLocalRuntimeLogs } from "./api";
import type { LogLine } from "./types";
import type { AppErrorDto } from "../../lib/tauri";
import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Runtime logs page (requirements-v2 §9 V2-FR-014, §13.4). Only the bounded,
 * already-redacted ring-buffer tail is shown — the page never re-fetches full
 * output and never renders un-redacted content (design-v2 §15.3, V2-SR-004).
 */
export function RuntimeLogsPage() {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setLines(await getLocalRuntimeLogs(200));
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const clear = async () => {
    try {
      await clearLocalRuntimeLogs();
      await reload();
    } catch (e) {
      setError(toMessage(e));
    }
  };

  const copySummary = async () => {
    // Copy a non-sensitive *summary* (counts), never full env or raw output.
    const stderrCount = lines.filter((l) => l.stream === "stderr").length;
    const stdoutCount = lines.filter((l) => l.stream === "stdout").length;
    const summary = `Pi Hub Client local runtime log summary\nstdout lines: ${stdoutCount}\nstderr lines: ${stderrCount}\nbuffered total: ${lines.length}`;
    try {
      await navigator.clipboard.writeText(summary);
    } catch {
      // Clipboard may be unavailable; ignore.
    }
  };

  if (loading) {
    return (
      <PagePlaceholder title="加载日志…">
        <p>正在读取最近的本机 Pi Hub 启动日志。</p>
      </PagePlaceholder>
    );
  }

  return (
    <div className="local-runtime-logs">
      <header className="logs-header">
        <h2>启动日志</h2>
        <div className="local-runtime-actions">
          <button type="button" onClick={() => void reload()}>
            刷新
          </button>
          <button type="button" onClick={() => void copySummary()}>
            复制诊断摘要
          </button>
          <button type="button" className="danger" onClick={() => void clear()}>
            清空
          </button>
          <Link to="/">返回</Link>
        </div>
      </header>

      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}

      <pre className="logs-view" aria-label="本机 Pi Hub 日志">
        {lines.length === 0
          ? "（暂无日志）"
          : lines
              .map((l) => `${l.timestamp} [${l.stream}] ${l.text}`)
              .join("\n")}
      </pre>
    </div>
  );
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}
