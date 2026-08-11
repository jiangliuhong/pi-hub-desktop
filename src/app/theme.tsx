import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type ThemeMode = "light" | "dark" | "system";

interface ThemeContextValue {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);
// Use a new key so an older dark-mode preference cannot make the first launch dark.
const STORAGE_KEY = "pi-hub-client-theme-v3";

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<ThemeMode>(() => readMode());

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = mode === "dark" || (mode === "system" && media?.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    media?.addEventListener("change", apply);
    localStorage.setItem(STORAGE_KEY, mode);
    return () => media?.removeEventListener("change", apply);
  }, [mode]);

  const value = useMemo(() => ({ mode, setMode }), [mode]);
  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

// The hook intentionally lives with the provider to keep the theme API small.
// eslint-disable-next-line react-refresh/only-export-components
export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used inside ThemeProvider");
  return context;
}

function readMode(): ThemeMode {
  const value = localStorage.getItem(STORAGE_KEY);
  return value === "dark" || value === "system" ? value : "light";
}
