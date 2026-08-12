/**
 * User-facing labels for V2 local runtime states (AGENTS.md §11: UI uses
 * readable labels, not raw enum strings). Plain-Chinese copy matches the rest
 * of the app; no secrets or shell commands are ever shown to end users
 * (AGENTS.md §11, V2-SR-001).
 */

import type {
  CheckSeverity,
  CheckStatus,
  EnvironmentStatus,
  InstallationSource,
  InstallationState,
  LocalRuntimeState,
} from "./types";

export function runtimeStateLabel(
  state: LocalRuntimeState,
  lastErrorCode?: string,
): string {
  switch (state) {
    case "unknown":
      return "未知状态";
    case "checking":
      return "正在检测";
    case "stopped":
      return "已停止";
    case "starting":
      return "正在启动";
    case "running_managed":
      return "正在运行 · 由 Client 管理";
    case "running_external":
      return "正在运行 · 外部启动";
    case "stopping":
      return "正在停止";
    case "port_conflict":
      return "端口冲突";
    case "failed":
      if (lastErrorCode === "pi_hub_doctor_blocked") {
        return "启动被阻止";
      }
      if (
        lastErrorCode === "local_port_not_released" ||
        lastErrorCode === "local_process_stop_timeout"
      ) {
        return "停止失败";
      }
      return "启动失败";
  }
}

/** Whether the state means an operation is in flight (buttons disabled). */
export function isBusy(state: LocalRuntimeState): boolean {
  return state === "checking" || state === "starting" || state === "stopping";
}

export function environmentStatusLabel(status: EnvironmentStatus): string {
  switch (status) {
    case "ready":
      return "就绪";
    case "degraded":
      return "部分可用";
    case "blocked":
      return "存在阻断";
    case "unknown":
      return "未检查";
  }
}

export function installationStateLabel(state: InstallationState): string {
  switch (state) {
    case "unknown":
      return "未检测";
    case "not_found":
      return "未安装";
    case "invalid":
      return "安装无效";
    case "incompatible":
      return "版本不兼容";
    case "ready":
      return "可用";
  }
}

export function sourceLabel(source: InstallationSource): string {
  const map: Record<InstallationSource, string> = {
    persisted: "已保存",
    path: "PATH",
    homebrew: "Homebrew",
    nvm: "NVM",
    volta: "Volta",
    fnm: "FNM",
    asdf: "ASDF",
    mise: "Mise",
    manual: "手动选择",
    npm_global: "npm 全局安装",
    desktop_managed: "Desktop 受管",
  };
  return map[source];
}

export function severityLabel(severity: CheckSeverity): string {
  switch (severity) {
    case "required":
      return "必需";
    case "recommended":
      return "建议";
    case "informational":
      return "信息";
  }
}

export function checkStatusLabel(status: CheckStatus): string {
  switch (status) {
    case "pass":
      return "通过";
    case "warn":
      return "警告";
    case "fail":
      return "失败";
    case "skipped":
      return "跳过";
  }
}

/** Symbol for a check status, for the doctor list (FR-004 / design §13.3). */
export function checkStatusSymbol(status: CheckStatus): string {
  switch (status) {
    case "pass":
      return "✓";
    case "warn":
      return "!";
    case "fail":
      return "✕";
    case "skipped":
      return "–";
  }
}

export function categoryLabel(category: string): string {
  switch (category) {
    case "runtime":
      return "Runtime";
    case "pi_hub":
      return "Pi Hub";
    case "pi_environment":
      return "Pi 环境";
    case "auth_and_models":
      return "认证与模型";
    case "optional_tools":
      return "可选工具";
    default:
      return category;
  }
}
