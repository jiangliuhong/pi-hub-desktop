import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PackageManagementSettings } from "./PackageManagementSettings";
import type { PackageErrorDto, PackageManagementSnapshot } from "./types";
import type { PackageManagementActions } from "./usePackageManagement";

const actions: PackageManagementActions = {
  scan: vi.fn().mockResolvedValue(undefined),
  checkUpdates: vi.fn().mockResolvedValue(undefined),
  install: vi.fn().mockResolvedValue(undefined),
  update: vi.fn().mockResolvedValue(undefined),
  confirmRestart: vi.fn().mockResolvedValue(undefined),
  cancel: vi.fn().mockResolvedValue(undefined),
  activate: vi.fn().mockResolvedValue(undefined),
};

function snapshot(): PackageManagementSnapshot {
  return {
    platform_supported: true,
    prerequisites: {
      node: { name: "node", satisfied: true, version: "24.19.0" },
      npm: { name: "npm", satisfied: true, version: "11.0.0" },
    },
    products: [
      {
        product: "pi",
        install_state: "not_installed",
        alternatives: [],
        update_status: "available",
        latest_version: "0.90.0",
        allowed_actions: ["scan", "check_updates", "install"],
      },
      {
        product: "pi_hub",
        install_state: "installed",
        current: {
          installation_id: "global-pi-hub",
          package_name: "@jarome/pi-hub",
          version: "0.3.0",
          source: "npm_global",
          ownership: "external",
        },
        alternatives: [],
        update_status: "available",
        latest_version: "0.4.0",
        allowed_actions: ["scan", "check_updates", "update"],
      },
    ],
  };
}

describe("PackageManagementSettings", () => {
  it("shows npm-global install and upgrade actions", () => {
    render(
      <PackageManagementSettings
        snapshot={snapshot()}
        loading={false}
        actions={actions}
        error={null}
        errorProduct={null}
      />,
    );

    expect(screen.getByText("npm 全局组件")).toBeInTheDocument();
    expect(screen.getByText("Node.js")).toBeInTheDocument();
    expect(screen.getByText("npm")).toBeInTheDocument();
    expect(
      screen.getByText(
        "npm install -g @jarome/pi-hub@latest --registry=https://registry.npmjs.org",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "安装" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "升级" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pi 重新扫描" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pi Hub 重新扫描" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pi 检查更新" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pi Hub 检查更新" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Pi 重新扫描" }));
    fireEvent.click(screen.getByRole("button", { name: "Pi Hub 检查更新" }));
    expect(actions.scan).toHaveBeenCalledWith("pi");
    expect(actions.checkUpdates).toHaveBeenCalledWith("pi_hub", true);
  });

  it("does not report an installation as missing or damaged when npm is unavailable", () => {
    const unavailable = snapshot();
    unavailable.prerequisites.npm = { name: "npm", satisfied: false };
    unavailable.products[0] = {
      product: "pi",
      install_state: "unknown",
      alternatives: [],
      update_status: "available",
      latest_version: "0.90.0",
      allowed_actions: ["scan"],
      issue: {
        code: "package_npm_unavailable",
        message: "npm 不可用。",
      },
    };

    render(
      <PackageManagementSettings
        snapshot={unavailable}
        loading={false}
        actions={actions}
        error={null}
        errorProduct={null}
      />,
    );

    expect(screen.getAllByText("无法验证")).toHaveLength(2);
    expect(screen.queryByText("安装损坏")).not.toBeInTheDocument();
  });

  it("confirms an upgrade through the high-risk dialog and calls actions.update", async () => {
    let resolveUpdate: () => void = () => undefined;
    actions.update = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveUpdate = resolve;
        }),
    );

    render(
      <PackageManagementSettings
        snapshot={snapshot()}
        loading={false}
        actions={actions}
        error={null}
        errorProduct={null}
      />,
    );

    // Open the confirmation dialog (update is a high-risk action).
    fireEvent.click(screen.getByRole("button", { name: "升级" }));
    const confirm = await screen.findByRole("button", { name: "确认" });
    expect(confirm).toBeInTheDocument();

    // Clicking confirm should fire the async action and show a pending state.
    fireEvent.click(confirm);
    await waitFor(() => {
      expect(actions.update).toHaveBeenCalledWith("pi_hub");
    });
    // While the action is in flight the button reads "处理中…" and is disabled.
    const pending = await screen.findByRole("button", { name: "处理中…" });
    expect(pending).toBeDisabled();

    // Once the action resolves the dialog closes back to the plain "升级" button.
    resolveUpdate();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "处理中…" })).toBeNull(),
    );
  });

  it("surfaces an attributed action error inside the product card", () => {
    const error: PackageErrorDto = {
      code: "package_release_check_failed",
      message: "无法获取 Pi Hub 的最新版本信息，请检查网络后重试。",
      retryable: true,
    };

    render(
      <PackageManagementSettings
        snapshot={snapshot()}
        loading={false}
        actions={actions}
        error={error}
        errorProduct="pi_hub"
      />,
    );

    // The message appears both in the global banner and inside the attributed
    // product card, so there are at least two occurrences.
    const occurrences = screen.getAllByText(error.message);
    expect(occurrences.length).toBeGreaterThanOrEqual(2);
  });
});
