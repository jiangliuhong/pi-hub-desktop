import { BrowserRouter } from "react-router-dom";
import { AppProviders } from "./providers";
import { AppRoutes } from "./router";

/**
 * Trusted App Shell root.
 *
 * This is the only React surface allowed to call Tauri commands. The remote
 * Pi Hub Service WebView (added in a later phase) is strictly isolated and
 * receives no Tauri capability — see AGENTS.md §6.4 and docs/design-v1.md §14.
 */
export function App() {
  return (
    <AppProviders>
      <BrowserRouter>
        <div className="app-shell">
          <header className="app-header">
            <h1>Pi Hub Client</h1>
            <span className="tag">V1 · 初始化阶段</span>
          </header>
          <main className="app-main">
            <AppRoutes />
          </main>
        </div>
      </BrowserRouter>
    </AppProviders>
  );
}
