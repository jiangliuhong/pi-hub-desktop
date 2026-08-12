import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import {
  getLocalRuntimeSettings,
  runLocalEnvironmentDoctor,
  updateLocalRuntimeSettings,
} from "./api";
import type { LocalRuntimeSettings, LocalRuntimeSettingsUpdate } from "./types";
import type { AppErrorDto } from "../../lib/tauri";
import { PagePlaceholder } from "../../components/PagePlaceholder";
import { PackageManagementSettings } from "../package-management/PackageManagementSettings";
import { usePackageManagement } from "../package-management/usePackageManagement";

/**
 * Local runtime settings page (requirements-v2 §9 V2-FR-015, §13.2).
 *
 * Only allowlisted, non-secret fields are accepted — there is deliberately no
 * "custom command" or "extra env" field (V2-SR-001). Manual Node / Pi Hub
 * selection is validated through the Rust detector before being saved
 * (V2-FR-003).
 */
export function LocalRuntimeSettingsPage() {
  const [settings, setSettings] = useState<LocalRuntimeSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const packages = usePackageManagement();

  const reload = useCallback(async () => {
    try {
      setSettings(await getLocalRuntimeSettings());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) {
    return (
      <PagePlaceholder title="加载本机设置…">
        <p>正在读取本机 Pi Hub 设置。</p>
      </PagePlaceholder>
    );
  }

  const apply = async (patch: LocalRuntimeSettingsUpdate) => {
    setSaving(true);
    setError(null);
    setInfo(null);
    try {
      const next = await updateLocalRuntimeSettings(patch);
      setSettings(next);
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const recheck = async () => {
    setSaving(true);
    setError(null);
    try {
      await runLocalEnvironmentDoctor(true);
      setInfo("已重新运行环境检查。");
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="local-runtime-settings">
      <h2>本机组件</h2>

      {packages.supported ? (
        <PackageManagementSettings
          snapshot={packages.snapshot}
          loading={packages.loading}
          actions={packages.actions}
          error={packages.error}
          errorProduct={packages.errorProduct}
        />
      ) : null}

      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}
      {info ? <div className="info-banner">{info}</div> : null}

      <label className="field">
        <span>Pi Agent 数据目录（可选）</span>
        <input
          type="text"
          value={settings.pi_agent_dir ?? ""}
          placeholder="留空则使用 ~/.pi/agent"
          onChange={(e) =>
            setSettings({ ...settings, pi_agent_dir: e.target.value })
          }
          onBlur={(e) => void apply({ pi_agent_dir: e.target.value })}
          disabled={saving}
        />
      </label>

      <label className="field">
        <span>本地端口</span>
        <input
          type="number"
          min={1}
          max={65535}
          value={settings.port}
          onChange={(e) =>
            setSettings({ ...settings, port: Number(e.target.value) })
          }
          onBlur={(e) => void apply({ port: Number(e.target.value) })}
          disabled={saving}
        />
      </label>

      <label className="field field-inline">
        <input
          type="checkbox"
          checked={settings.auto_start_on_app_launch}
          onChange={(e) =>
            void apply({ auto_start_on_app_launch: e.target.checked })
          }
          disabled={saving}
        />
        <span>打开 Pi Hub Client 时自动启动本机 Pi Hub</span>
      </label>

      <label className="field field-inline">
        <input
          type="checkbox"
          checked={settings.stop_managed_on_app_exit}
          onChange={(e) =>
            void apply({ stop_managed_on_app_exit: e.target.checked })
          }
          disabled={saving}
        />
        <span>退出 Pi Hub Client 时停止由本次启动的 Pi Hub</span>
      </label>

      <div className="local-runtime-actions">
        <button type="button" onClick={() => void recheck()} disabled={saving}>
          重新检查环境
        </button>
        <Link to="/local-runtime/environment">查看检查结果</Link>
        <Link to="/">返回</Link>
      </div>
    </div>
  );
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}
