import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import type { AppErrorDto } from "../../lib/tauri";
import { PiHubFrame } from "../viewer/PiHubFrame";
import { scanLocalInstallations } from "./api";

/**
 * Connects to the current macOS local Runtime and presents it inside the same
 * isolated in-app Viewer used by remote profiles. The fresh Rust probe is the
 * source of truth; a stale URL from the card is never loaded directly.
 */
export function LocalRuntimeViewer() {
  const navigate = useNavigate();
  const [effectiveUrl, setEffectiveUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const connect = async () => {
      try {
        const snapshot = await scanLocalInstallations();
        if (cancelled) return;
        if (
          (snapshot.runtime_state === "running_managed" ||
            snapshot.runtime_state === "running_external") &&
          snapshot.effective_url
        ) {
          setEffectiveUrl(snapshot.effective_url);
        } else {
          setError("本机 Pi Hub 当前不可连接，请返回列表重新检测或启动。");
        }
      } catch (e) {
        const dto = e as AppErrorDto | undefined;
        if (!cancelled) setError(dto?.message ?? "连接本机 Pi Hub 失败。");
      }
    };
    void connect();
    return () => {
      cancelled = true;
    };
  }, []);

  const returnToList = useCallback(() => {
    void navigate("/");
  }, [navigate]);

  if (error) {
    return (
      <PagePlaceholder title="无法连接本机 Pi Hub">
        <p>{error}</p>
        <Link to="/">返回服务列表</Link>
      </PagePlaceholder>
    );
  }

  if (!effectiveUrl) {
    return (
      <PagePlaceholder title="正在连接本机 Pi Hub…">
        <p>正在确认本机服务身份和运行状态。</p>
      </PagePlaceholder>
    );
  }

  return (
    <div className="viewer-page">
      <PiHubFrame
        effectiveUrl={effectiveUrl}
        onReturnToServices={returnToList}
      />
    </div>
  );
}
