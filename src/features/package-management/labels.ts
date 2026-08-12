/**
 * User-facing labels for V3 package management (AGENTS.md §11: readable labels,
 * not raw enum strings). Plain-Chinese copy; no shell commands or secrets are
 * ever shown (V3-SR-001/005).
 */

import type {
  InstallationSource,
  PackageOperationStage,
  ProductAction,
  ProductId,
  ProductInstallState,
  UpdateStatus,
} from "./types";

export function productIdLabel(product: ProductId): string {
  switch (product) {
    case "pi":
      return "Pi";
    case "pi_hub":
      return "Pi Hub";
  }
}

export function productDescription(product: ProductId): string {
  switch (product) {
    case "pi":
      return "用于终端交互；不影响 Pi Hub 内嵌 Runtime。";
    case "pi_hub":
      return "本机 Web 服务；更新可能需要重启。";
  }
}

export function installStateLabel(state: ProductInstallState): string {
  switch (state) {
    case "unknown":
      return "无法验证";
    case "not_installed":
      return "未安装";
    case "installed":
      return "已安装";
    case "invalid":
      return "安装损坏";
    case "incompatible":
      return "版本不兼容";
  }
}

export function updateStatusLabel(status: UpdateStatus): string {
  switch (status) {
    case "unknown":
      return "未检查";
    case "checking":
      return "正在检查";
    case "up_to_date":
      return "已是最新";
    case "available":
      return "有可用更新";
    case "newer_than_latest":
      return "版本高于 latest（不降级）";
    case "unavailable":
      return "无法获取版本";
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
    manual: "手动路径",
    npm_global: "npm 全局安装",
    desktop_managed: "Desktop 受管",
  };
  return map[source];
}

export function ownershipLabel(
  ownership: "desktop_managed" | "external",
): string {
  return ownership === "desktop_managed" ? "受管" : "外部";
}

export function stageLabel(stage: PackageOperationStage): string {
  switch (stage) {
    case "preparing":
      return "准备中";
    case "fetching_metadata":
      return "获取版本信息";
    case "installing":
      return "下载并安装";
    case "verifying":
      return "验证";
    case "awaiting_restart_confirmation":
      return "等待确认重启";
    case "activating":
      return "切换版本";
    case "restarting":
      return "重启";
    case "rolling_back":
      return "回滚";
    case "completed":
      return "完成";
    case "cancelled":
      return "已取消";
    case "failed":
      return "失败";
  }
}

/** Whether a stage is terminal (no progress bar). */
export function isTerminalStage(stage: PackageOperationStage): boolean {
  return stage === "completed" || stage === "cancelled" || stage === "failed";
}

/** Whether a stage is in-flight (spinner + cancel). */
export function isInFlight(stage: PackageOperationStage): boolean {
  return !isTerminalStage(stage);
}

export function actionLabel(action: ProductAction): string {
  switch (action) {
    case "scan":
      return "重新扫描";
    case "check_updates":
      return "检查更新";
    case "install":
      return "安装";
    case "update":
      return "升级";
    case "repair":
      return "修复安装";
    case "activate":
      return "使用此版本";
    case "cancel":
      return "取消";
    case "confirm_restart":
      return "更新并重启";
  }
}

/** High-risk operations require a second confirmation (AGENTS.md §11). */
export function isHighRisk(action: ProductAction): boolean {
  return (
    action === "update" ||
    action === "install" ||
    action === "repair" ||
    action === "confirm_restart"
  );
}
