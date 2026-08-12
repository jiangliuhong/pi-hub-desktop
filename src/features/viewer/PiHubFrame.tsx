import { useEffect, useRef } from "react";
import {
  createHostExtensionRegistration,
  OPEN_SETTINGS_ITEM_ID,
  parsePiHubHostExtensionEvent,
} from "./bridge";

interface PiHubFrameProps {
  effectiveUrl: string;
  onReturnToServices: () => void | Promise<void>;
}

/**
 * Shared isolated Pi Hub frame for remote profiles and the macOS local
 * runtime. The framed page receives no Tauri capability; all host actions are
 * origin-checked and handled by the trusted App Shell.
 */
export function PiHubFrame({
  effectiveUrl,
  onReturnToServices,
}: PiHubFrameProps) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const registrationTimersRef = useRef<ReturnType<typeof setTimeout>[]>([]);
  const allowedOrigin = new URL(effectiveUrl).origin;

  useEffect(() => {
    const receiveHostAction = (event: MessageEvent<unknown>) => {
      if (event.source !== frameRef.current?.contentWindow) return;
      if (event.origin !== allowedOrigin) return;
      const extensionEvent = parsePiHubHostExtensionEvent(event.data);
      if (extensionEvent?.itemId === OPEN_SETTINGS_ITEM_ID) {
        window.dispatchEvent(new Event("app:open-settings"));
      } else if (extensionEvent?.itemId === "return_to_services") {
        void onReturnToServices();
      }
    };
    window.addEventListener("message", receiveHostAction);
    return () => {
      window.removeEventListener("message", receiveHostAction);
      for (const timer of registrationTimersRef.current) clearTimeout(timer);
      registrationTimersRef.current = [];
    };
  }, [allowedOrigin, onReturnToServices]);

  const registerHostExtensions = () => {
    const registration = createHostExtensionRegistration();
    const send = () => {
      frameRef.current?.contentWindow?.postMessage(registration, allowedOrigin);
    };

    for (const timer of registrationTimersRef.current) clearTimeout(timer);
    registrationTimersRef.current = [];
    send();
    for (const delay of [50, 250, 1000]) {
      registrationTimersRef.current.push(setTimeout(send, delay));
    }
  };

  return (
    <iframe
      ref={frameRef}
      className="service-view-frame"
      src={effectiveUrl}
      title="Pi Hub"
      referrerPolicy="no-referrer"
      allow={`clipboard-read ${allowedOrigin}; clipboard-write ${allowedOrigin}`}
      onLoad={registerHostExtensions}
    />
  );
}
