import { useCallback, useEffect, useState } from "react";
import { BrowserRouter, useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { AppProviders } from "./providers";
import { ThemeProvider } from "./theme";
import { AppRoutes } from "./router";
import { SettingsDialog } from "../features/settings/SettingsPage";

/**
 * Trusted App Shell root.
 *
 * This is the only React surface allowed to call Tauri commands. The remote
 * Pi Hub Service WebView is strictly isolated and receives no Tauri
 * capability — see AGENTS.md §6.4 and docs/design-v1.md §14.
 */
export function App() {
  return (
    <AppProviders>
      <BrowserRouter>
        <ThemeProvider>
          <AppShell />
        </ThemeProvider>
      </BrowserRouter>
    </AppProviders>
  );
}

function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const isViewer =
    location.pathname.startsWith("/viewer/") ||
    location.pathname === "/local-runtime/viewer";

  useEffect(() => {
    let unlistenHome: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;

    void listen("app://return-to-services", () => {
      void navigate("/");
    })
      .then((cleanup) => {
        unlistenHome = cleanup;
      })
      .catch(() => undefined);
    void listen("app://open-settings", () => {
      setSettingsOpen(true);
    })
      .then((cleanup) => {
        unlistenSettings = cleanup;
      })
      .catch(() => undefined);

    const openSettings = () => setSettingsOpen(true);
    window.addEventListener("app:open-settings", openSettings);

    return () => {
      window.removeEventListener("app:open-settings", openSettings);
      unlistenHome?.();
      unlistenSettings?.();
    };
  }, [navigate]);

  return (
    <div className={`app-shell${isViewer ? " viewer-shell" : ""}`}>
      <main className={isViewer ? "app-main viewer-main" : "app-main"}>
        <AppRoutes />
      </main>
      {settingsOpen ? <SettingsDialog onClose={closeSettings} /> : null}
    </div>
  );
}
