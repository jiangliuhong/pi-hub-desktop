import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
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
    expect(await screen.findByText("打开")).toBeInTheDocument();
  });

  it("hides stop and restart for an external process", async () => {
    renderCard(snapshot({ runtime_state: "running_external" }));
    expect(await screen.findByText("打开")).toBeInTheDocument();
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

  it("uses distinct symbols per check status", () => {
    expect(checkStatusSymbol("pass")).not.toBe(checkStatusSymbol("fail"));
    expect(checkStatusSymbol("warn")).toBe("!");
    expect(checkStatusSymbol("fail")).toBe("✕");
  });
});
