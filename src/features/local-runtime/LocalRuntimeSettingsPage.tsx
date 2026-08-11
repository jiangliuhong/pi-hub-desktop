import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import {
  getLocalRuntimeSettings,
  runLocalEnvironmentDoctor,
  scanLocalInstallations,
  updateLocalRuntimeSettings,
  validateLocalInstallation,
} from "./api";
import { sourceLabel } from "./labels";
import type {
  InstallationSet,
  LocalRuntimeSettings,
  LocalRuntimeSettingsUpdate,
} from "./types";
import type { AppErrorDto } from "../../lib/tauri";
import { PagePlaceholder } from "../../components/PagePlaceholder";

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
  const [validating, setValidating] = useState(false);
  const [validation, setValidation] = useState<InstallationSet | null>(null);

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

  const validate = async () => {
    setValidating(true);
    setError(null);
    setInfo(null);
    try {
      const set = await validateLocalInstallation({
        node_executable: settings.node_executable,
        pi_hub_entrypoint: settings.pi_hub_entrypoint,
        pi_hub_package_root: settings.pi_hub_package_root,
      });
      setValidation(set);
      if (set.node && set.pi_hub) {
        setInfo("所选 Node.js 与 Pi Hub 组合有效。");
      } else {
        setError("无法验证所选安装组合，请检查路径。");
      }
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setValidating(false);
    }
  };

  const rescan = async () => {
    setSaving(true);
    setError(null);
    try {
      const snap = await scanLocalInstallations();
      if (snap.installation?.node?.executable) {
        await apply({
          node_executable: snap.installation.node.canonical_executable,
        });
      }
      if (snap.installation?.pi_hub) {
        await apply({
          pi_hub_entrypoint: snap.installation.pi_hub.entrypoint,
          pi_hub_package_root: snap.installation.pi_hub.package_root,
        });
      }
      setInfo("已重新扫描并应用可用安装。");
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
      <h2>Pi Hub Installation</h2>

      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}
      {info ? <div className="info-banner">{info}</div> : null}

      <label className="field">
        <span>Node.js</span>
        <input
          type="text"
          value={settings.node_executable ?? ""}
          placeholder="例如 /opt/homebrew/bin/node"
          onChange={(e) =>
            setSettings({ ...settings, node_executable: e.target.value })
          }
          onBlur={(e) => void apply({ node_executable: e.target.value })}
          disabled={saving}
        />
        {validation?.node ? (
          <small>
            验证通过：v{validation.node.version}（
            {sourceLabel(validation.node.source)}）
          </small>
        ) : null}
      </label>

      <label className="field">
        <span>Pi Hub 入口</span>
        <input
          type="text"
          value={settings.pi_hub_entrypoint ?? ""}
          placeholder="例如 /usr/local/lib/node_modules/@jarome/pi-hub/bin/pi-hub.js"
          onChange={(e) =>
            setSettings({ ...settings, pi_hub_entrypoint: e.target.value })
          }
          onBlur={(e) => void apply({ pi_hub_entrypoint: e.target.value })}
          disabled={saving}
        />
      </label>

      <label className="field">
        <span>Pi Hub 安装根目录</span>
        <input
          type="text"
          value={settings.pi_hub_package_root ?? ""}
          placeholder="Pi Hub package root"
          onChange={(e) =>
            setSettings({ ...settings, pi_hub_package_root: e.target.value })
          }
          onBlur={(e) => void apply({ pi_hub_package_root: e.target.value })}
          disabled={saving}
        />
        {validation?.pi_hub ? (
          <small>验证通过：v{validation.pi_hub.version}</small>
        ) : null}
      </label>

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
        <button
          type="button"
          onClick={() => void validate()}
          disabled={validating}
        >
          {validating ? "验证中…" : "验证所选安装"}
        </button>
        <button type="button" onClick={() => void rescan()} disabled={saving}>
          重新扫描
        </button>
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
