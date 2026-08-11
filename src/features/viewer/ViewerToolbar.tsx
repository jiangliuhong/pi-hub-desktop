import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { closeServiceView, openServiceView } from "./api";
import {
  createHostExtensionRegistration,
  OPEN_SETTINGS_ITEM_ID,
  parsePiHubHostExtensionEvent,
} from "./bridge";
import { getConnectionStatus } from "../connection/api";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import type { AppErrorDto } from "../../lib/tauri";

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
  const frameRef = useRef<HTMLIFrameElement>(null);
  const registrationTimersRef = useRef<ReturnType<typeof setTimeout>[]>([]);
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

  useEffect(() => {
    if (!effectiveUrl) return;
    const allowedOrigin = new URL(effectiveUrl).origin;
    const receiveHostAction = (event: MessageEvent<unknown>) => {
      if (event.source !== frameRef.current?.contentWindow) return;
      if (event.origin !== allowedOrigin) return;
      const extensionEvent = parsePiHubHostExtensionEvent(event.data);
      if (extensionEvent?.itemId === OPEN_SETTINGS_ITEM_ID) {
        window.dispatchEvent(new Event("app:open-settings"));
      } else if (extensionEvent?.itemId === "return_to_services") {
        void returnToList();
      }
    };
    window.addEventListener("message", receiveHostAction);
    return () => {
      window.removeEventListener("message", receiveHostAction);
      for (const timer of registrationTimersRef.current) clearTimeout(timer);
      registrationTimersRef.current = [];
    };
  }, [effectiveUrl, returnToList]);

  const registerHostExtensions = () => {
    if (!effectiveUrl) return;
    const allowedOrigin = new URL(effectiveUrl).origin;
    const registration = createHostExtensionRegistration();
    const send = () => {
      frameRef.current?.contentWindow?.postMessage(registration, allowedOrigin);
    };

    // The iframe's load event can race React hydration in the remote Hub. A
    // single postMessage may arrive before useHostExtensions subscribes, so
    // retry briefly after load. Registrations are idempotent by extension id
    // and revision, and all timers are cleared when the viewer unmounts.
    for (const timer of registrationTimersRef.current) clearTimeout(timer);
    registrationTimersRef.current = [];
    send();
    for (const delay of [50, 250, 1000]) {
      registrationTimersRef.current.push(setTimeout(send, delay));
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
    <div className="viewer-page">
      {effectiveUrl ? (
        <iframe
          ref={frameRef}
          className="service-view-frame"
          src={effectiveUrl}
          title="Pi Hub"
          referrerPolicy="no-referrer"
          // Clipboard access is required for user-initiated paste in the
          // cross-origin Service View. This does not grant the frame any
          // Tauri capability or access to the trusted App Shell.
          allow={`clipboard-read ${new URL(effectiveUrl).origin}; clipboard-write ${new URL(effectiveUrl).origin}`}
          onLoad={registerHostExtensions}
        />
      ) : null}
    </div>
  );
}
