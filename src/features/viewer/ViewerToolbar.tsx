import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { closeServiceView, openServiceView } from "./api";
import {
  getConnectionStatus,
  STATE_CHANGED_EVENT,
  type StateChangedPayload,
} from "../connection/api";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import type { AppErrorDto } from "../../lib/tauri";
import { PiHubFrame } from "./PiHubFrame";

/**
 * Full-window Service View for the existing Pi Hub WebUI
 * (docs/requirements-v1.md §8.5, FR-012).
 *
 * Pi Hub may render host-declared controls in its native extension slots after
 * receiving the versioned postMessage registration documented in
 * docs/pi-hub-embed-contract-v1.md. The remote page still receives no Tauri
 * capability (AGENTS.md §6.4).
 */
export function ViewerToolbar() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [effectiveUrl, setEffectiveUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    const run = async () => {
      try {
        const s = await getConnectionStatus(id);
        if (cancelled) return;
        if (s?.effective_url) {
          await openServiceView(id, s.effective_url);
          if (cancelled) return;
          setEffectiveUrl(s.effective_url);
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

  // Reliability (plan §5.5.4): after an SSH reconnect the Rust side publishes
  // a new effective_url (the loopback port changes). Subscribe to state-changed
  // and reload the iframe onto the new URL so the Viewer doesn't keep hitting
  // a dead, released listener. `src={effectiveUrl}` is declarative, so
  // updating the state is enough for React to reload the frame; the host
  // extension registration effect (below) re-keys on `effectiveUrl` too.
  useEffect(() => {
    if (!id) return;
    const unlisten = listen<StateChangedPayload>(
      STATE_CHANGED_EVENT,
      (event) => {
        const payload = event.payload;
        if (payload.service_id !== id) return;
        if (payload.effective_url && payload.effective_url !== effectiveUrl) {
          void openServiceView(id, payload.effective_url).catch(() => {
            // Re-registration is best-effort; the iframe reload is the
            // important part. A failure here surfaces as a Viewer load error
            // on the new URL, which the user can retry.
          });
          setEffectiveUrl(payload.effective_url);
        }
      },
    );
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [id, effectiveUrl]);

  const returnToList = useCallback(async () => {
    if (id) {
      try {
        await closeServiceView(id);
      } catch {
        // Navigation must remain available even if cleanup already happened.
      }
    }
    void navigate("/");
  }, [id, navigate]);

  if (error) {
    return (
      <PagePlaceholder title="无法打开 Pi Hub">
        <p>{error}</p>
        <Link to="/">返回服务列表</Link>
      </PagePlaceholder>
    );
  }

  return (
    <div className="viewer-page">
      {effectiveUrl ? (
        <PiHubFrame
          effectiveUrl={effectiveUrl}
          onReturnToServices={returnToList}
        />
      ) : null}
    </div>
  );
}
