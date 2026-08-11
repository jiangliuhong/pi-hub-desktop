import { useEffect } from "react";
import { useTheme, type ThemeMode } from "../../app/theme";

const options: { value: ThemeMode; label: string; description: string }[] = [
  { value: "light", label: "亮色", description: "始终使用亮色外观" },
  { value: "dark", label: "暗色", description: "始终使用暗色外观" },
  {
    value: "system",
    label: "跟随系统",
    description: "跟随 macOS 的外观设置",
  },
];

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const { mode, setMode } = useTheme();

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return (
    <div
      className="settings-dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <header className="settings-dialog-header">
          <h2 id="settings-title">设置</h2>
          <button type="button" aria-label="关闭设置" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="settings-card">
          <h3>外观</h3>
          <p className="settings-description">
            选择 Pi Hub Client 的显示方式。
          </p>
          <fieldset className="theme-options">
            <legend className="sr-only">主题模式</legend>
            {options.map((option) => (
              <label className="theme-option" key={option.value}>
                <input
                  type="radio"
                  name="theme"
                  value={option.value}
                  checked={mode === option.value}
                  onChange={() => setMode(option.value)}
                />
                <span>
                  <strong>{option.label}</strong>
                  <small>{option.description}</small>
                </span>
              </label>
            ))}
          </fieldset>
        </div>
      </section>
    </div>
  );
}
