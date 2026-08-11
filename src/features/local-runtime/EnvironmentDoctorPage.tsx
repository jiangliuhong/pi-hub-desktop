import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { getLocalRuntimeStatus, runLocalEnvironmentDoctor } from "./api";
import {
  categoryLabel,
  checkStatusSymbol,
  environmentStatusLabel,
  severityLabel,
} from "./labels";
import type { CheckResult, LocalRuntimeSnapshot } from "./types";
import type { AppErrorDto } from "../../lib/tauri";
import { PagePlaceholder } from "../../components/PagePlaceholder";

/**
 * Environment Doctor page (requirements-v2 §9 V2-FR-004, §13.3). Groups
 * Desktop + Pi Hub checks by category and surfaces remediation steps. Never
 * auto-installs or auto-fixes (design-v2 §8.1).
 */
export function EnvironmentDoctorPage() {
  const [snapshot, setSnapshot] = useState<LocalRuntimeSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const reload = useCallback(async () => {
    try {
      setSnapshot(await getLocalRuntimeStatus());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const recheck = async () => {
    setRunning(true);
    setError(null);
    try {
      const report = await runLocalEnvironmentDoctor(true);
      setSnapshot((prev) => (prev ? { ...prev, environment: report } : prev));
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setRunning(false);
    }
  };

  const grouped = useMemo(() => groupByCategory(snapshot), [snapshot]);
  const overall = snapshot?.environment.overall ?? "unknown";

  return (
    <div className="local-runtime-doctor">
      <header className="doctor-header">
        <h2>Environment Doctor</h2>
        <span className={`env-${overall}`}>
          总体：{environmentStatusLabel(overall)}
        </span>
      </header>

      {error ? (
        <div role="alert" className="error-banner">
          {error}
        </div>
      ) : null}

      <div className="local-runtime-actions">
        <button type="button" onClick={() => void recheck()} disabled={running}>
          {running ? "检查中…" : "重新检查"}
        </button>
        <Link to="/local-runtime/settings">设置</Link>
        <Link to="/">返回</Link>
      </div>

      {snapshot?.environment.checks.length === 0 ? (
        <PagePlaceholder title="尚未运行检查">
          <p>点击“重新检查”以检测本机 Pi Hub 环境。</p>
        </PagePlaceholder>
      ) : (
        <div className="doctor-groups">
          {grouped.map(([category, checks]) => (
            <section key={category} className="doctor-group">
              <h3>{categoryLabel(category)}</h3>
              <ul>
                {checks.map((c) => (
                  <li key={c.id} className={`check check-${c.status}`}>
                    <span className="check-symbol">
                      {checkStatusSymbol(c.status)}
                    </span>
                    <span className="check-message">{c.message ?? c.id}</span>
                    <span className="check-severity">
                      {severityLabel(c.severity)}
                    </span>
                    {c.remediation ? (
                      <span className="check-remediation">
                        建议：{c.remediation}
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function groupByCategory(
  snapshot: LocalRuntimeSnapshot | null,
): [string, CheckResult[]][] {
  const checks = snapshot?.environment.checks ?? [];
  const order = [
    "runtime",
    "pi_hub",
    "pi_environment",
    "auth_and_models",
    "optional_tools",
  ];
  const map = new Map<string, CheckResult[]>();
  for (const c of checks) {
    const list = map.get(c.category) ?? [];
    list.push(c);
    map.set(c.category, list);
  }
  return order
    .filter((cat) => map.has(cat))
    .map((cat) => [cat, map.get(cat)!] as [string, CheckResult[]]);
}

function toMessage(e: unknown): string {
  const dto = e as AppErrorDto | undefined;
  return dto?.message ?? "发生未知错误。";
}
