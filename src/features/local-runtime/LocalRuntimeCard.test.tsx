import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { LocalRuntimeCard } from "./LocalRuntimeCard";
import {
  checkStatusSymbol,
  environmentStatusLabel,
  runtimeStateLabel,
} from "./labels";
import type { LocalRuntimeSnapshot } from "./types";

/**
 * The V2 `This Mac` card reflects Rust-side truth only. These tests assert the
 * button matrix for each runtime state (requirements-v2 §7.2, design-v2
 * §21.4): blocked disables start, external hides stop/restart, managed shows
 * all controls. No optimistic local state is ever trusted.
 */
const invokeMock = vi.fn<(c: string, a?: unknown) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (c: string, a?: unknown) => invokeMock(c, a),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(vi.fn()),
}));

function snapshot(
  over: Partial<LocalRuntimeSnapshot> = {},
): LocalRuntimeSnapshot {
  return {
    installation_state: "ready",
    runtime_state: "stopped",
    environment: { overall: "ready", checks: [] },
    installation: {
      node: {
        executable: "/usr/local/bin/node",
        canonical_executable: "/usr/local/bin/node",
        version: "24.19.0",
        source: "homebrew",
      },
      pi_hub: {
        package_root: "/p",
        entrypoint: "/p/bin/pi-hub.js",
        version: "0.0.42",
        node_requirement: ">=22.19.0",
        source: "homebrew",
      },
    },
    effective_url: "http://127.0.0.1:30142",
    ...over,
  };
}

function renderCard(snap: LocalRuntimeSnapshot) {
  invokeMock.mockImplementation((command: string) => {
    if (command === "get_local_runtime_status") return Promise.resolve(snap);
    return Promise.resolve({});
  });
  return render(
    <MemoryRouter>
      <LocalRuntimeCard />
    </MemoryRouter>,
  );
}

describe("LocalRuntimeCard", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("shows the This Mac header and versions", async () => {
    renderCard(snapshot());
    expect(await screen.findByText("This Mac")).toBeInTheDocument();
    expect(await screen.findByText(/Pi Hub 0\.0\.42/)).toBeInTheDocument();
    expect(await screen.findByText(/Node 24\.19\.0/)).toBeInTheDocument();
  });

  it("labels the scan action as installation detection", async () => {
    renderCard(snapshot());
    expect(await screen.findByText("检测安装")).toBeInTheDocument();
  });

  it("shows start but not stop when stopped", async () => {
    renderCard(snapshot({ runtime_state: "stopped" }));
    expect(await screen.findByText("启动")).toBeInTheDocument();
    expect(screen.queryByText("停止")).not.toBeInTheDocument();
    expect(screen.queryByText("重启")).not.toBeInTheDocument();
  });

  it("shows stop and restart for a managed process", async () => {
    renderCard(snapshot({ runtime_state: "running_managed" }));
    expect(await screen.findByText("停止")).toBeInTheDocument();
    expect(await screen.findByText("重启")).toBeInTheDocument();
    expect(await screen.findByText("连接")).toBeInTheDocument();
  });

  it("navigates to the in-app local connection viewer", async () => {
    const snap = snapshot({ runtime_state: "running_managed" });
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_local_runtime_status") return Promise.resolve(snap);
      return Promise.resolve({});
    });
    render(
      <MemoryRouter initialEntries={["/"]}>
        <Routes>
          <Route path="/" element={<LocalRuntimeCard />} />
          <Route
            path="/local-runtime/viewer"
            element={<p>本机连接 Viewer</p>}
          />
        </Routes>
      </MemoryRouter>,
    );
    await userEvent.click(await screen.findByRole("button", { name: "连接" }));
    expect(await screen.findByText("本机连接 Viewer")).toBeInTheDocument();
  });

  it("hides stop and restart for an external process", async () => {
    renderCard(snapshot({ runtime_state: "running_external" }));
    expect(await screen.findByText("连接")).toBeInTheDocument();
    expect(screen.queryByText("停止")).not.toBeInTheDocument();
    expect(screen.queryByText("重启")).not.toBeInTheDocument();
  });

  it("disables start when environment is blocked", async () => {
    renderCard(
      snapshot({
        runtime_state: "stopped",
        environment: { overall: "blocked", checks: [] },
      }),
    );
    const start = await screen.findByText("启动");
    expect(start).toBeDisabled();
  });

  it("shows the effective loopback url", async () => {
    renderCard(snapshot({ runtime_state: "running_managed" }));
    expect(
      await screen.findByText("http://127.0.0.1:30142"),
    ).toBeInTheDocument();
  });

  it("invokes start when the start button is clicked", async () => {
    renderCard(snapshot({ runtime_state: "stopped" }));
    const start = await screen.findByText("启动");
    await userEvent.click(start);
    const calls = invokeMock.mock.calls.map((c) => c[0]);
    expect(calls).toContain("start_local_pi_hub");
  });

  it("shows last_error with recent output and a logs link when failed", async () => {
    renderCard(
      snapshot({
        runtime_state: "failed",
        last_error: {
          code: "local_process_exited_early",
          message:
            "本机 Pi Hub 启动后立即退出（退出码 1）。请查看「日志」了解详情。",
          retryable: false,
          details: {
            exitCode: "1",
            recentOutput: "[stderr] Error: Cannot find module '@jarome/pi-hub'",
          },
        },
      }),
    );
    expect(
      await screen.findByText(/启动后立即退出（退出码 1）/),
    ).toBeInTheDocument();
    expect(await screen.findByText("查看日志")).toBeInTheDocument();
    expect(
      await screen.findByText(
        /\[stderr\] Error: Cannot find module '@jarome\/pi-hub'/,
      ),
    ).toBeInTheDocument();
  });

  it("shows Doctor blocking as prevented start and links to the report", async () => {
    renderCard(
      snapshot({
        runtime_state: "failed",
        environment: {
          overall: "blocked",
          checks: [
            {
              id: "DEP-NODE-001",
              category: "runtime",
              severity: "required",
              status: "fail",
              message: "Node.js 版本不满足要求。",
            },
          ],
        },
        last_error: {
          code: "pi_hub_doctor_blocked",
          message:
            "存在阻断性问题，无法启动本机 Pi Hub。请先查看环境检查结果。",
          retryable: false,
        },
      }),
    );

    expect(await screen.findByText("启动被阻止")).toBeInTheDocument();
    const reportLinks = await screen.findAllByText("查看检查结果");
    expect(reportLinks).toHaveLength(2);
    expect(reportLinks[0]).toHaveAttribute(
      "href",
      "/local-runtime/environment",
    );
  });

  it("labels an unreleased port as stop failure, not start failure", async () => {
    renderCard(
      snapshot({
        runtime_state: "failed",
        last_error: {
          code: "local_port_not_released",
          message: "停止后本地端口 30142 仍未释放，请检查是否有残留进程。",
          retryable: true,
        },
      }),
    );

    expect(await screen.findByText("停止失败")).toBeInTheDocument();
    expect(screen.queryByText("启动失败")).not.toBeInTheDocument();
  });

  it("does not duplicate a command error already persisted by Rust", async () => {
    const blocked = snapshot({
      runtime_state: "failed",
      environment: { overall: "blocked", checks: [] },
      last_error: {
        code: "pi_hub_doctor_blocked",
        message: "存在阻断性问题，无法启动本机 Pi Hub。请先查看环境检查结果。",
        retryable: false,
      },
    });
    let statusCalls = 0;
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_local_runtime_status") {
        statusCalls += 1;
        return Promise.resolve(statusCalls === 1 ? snapshot() : blocked);
      }
      if (command === "start_local_pi_hub") {
        return Promise.reject(
          Object.assign(
            new Error(blocked.last_error?.message),
            blocked.last_error,
          ),
        );
      }
      return Promise.resolve({});
    });
    render(
      <MemoryRouter>
        <LocalRuntimeCard />
      </MemoryRouter>,
    );

    await userEvent.click(await screen.findByText("启动"));
    await waitFor(() => {
      expect(screen.getAllByRole("alert")).toHaveLength(1);
    });
    expect(
      screen.getAllByText(/存在阻断性问题，无法启动本机 Pi Hub/),
    ).toHaveLength(1);
  });
});

describe("labels", () => {
  it("maps every runtime state to a readable label", () => {
    const states = [
      "unknown",
      "checking",
      "stopped",
      "starting",
      "running_managed",
      "running_external",
      "stopping",
      "port_conflict",
      "failed",
    ] as const;
    for (const s of states) {
      expect(runtimeStateLabel(s)).not.toBe(s);
    }
  });

  it("maps environment statuses", () => {
    expect(environmentStatusLabel("ready")).toBe("就绪");
    expect(environmentStatusLabel("degraded")).toBe("部分可用");
    expect(environmentStatusLabel("blocked")).toBe("存在阻断");
    expect(environmentStatusLabel("unknown")).toBe("未检查");
  });

  it("distinguishes a Doctor block from a process start failure", () => {
    expect(runtimeStateLabel("failed", "pi_hub_doctor_blocked")).toBe(
      "启动被阻止",
    );
    expect(runtimeStateLabel("failed", "local_process_exited_early")).toBe(
      "启动失败",
    );
  });

  it("uses distinct symbols per check status", () => {
    expect(checkStatusSymbol("pass")).not.toBe(checkStatusSymbol("fail"));
    expect(checkStatusSymbol("warn")).toBe("!");
    expect(checkStatusSymbol("fail")).toBe("✕");
  });
});
