import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { App } from "./App";

/**
 * The trusted App Shell loads services via Tauri commands. We stub `invoke` so
 * the home route renders its empty state deterministically (FR-001: never
 * fabricate an "Online" status).
 */
const invokeMock =
  vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));
// The home route now subscribes to V2 status events; stub `listen` so the
// LocalRuntimeCard mount does not reach Tauri internals in unit tests.
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(vi.fn()),
}));

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    // V1-focused tests: keep the V2 local-runtime card hidden so the empty
    // service state renders unchanged.
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_local_runtime_platform_support") {
        return Promise.resolve(false);
      }
      return Promise.resolve([]);
    });
  });

  it("renders the home route without duplicating the native app title bar", async () => {
    render(<App />);
    expect(await screen.findByText(/还没有 Pi Hub 服务/)).toBeInTheDocument();
    expect(screen.queryByText("Pi Hub Client")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("打开设置")).not.toBeInTheDocument();
  });

  it("opens settings as a global dialog", async () => {
    render(<App />);
    await act(() => {
      window.dispatchEvent(new Event("app:open-settings"));
      return Promise.resolve();
    });
    expect(
      await screen.findByRole("dialog", { name: "设置" }),
    ).toBeInTheDocument();
  });

  it("exposes the add-service entry point from the empty state", async () => {
    render(<App />);
    expect(await screen.findByText(/添加服务/)).toBeInTheDocument();
  });

  it("lists services returned by the Rust core", async () => {
    invokeMock.mockResolvedValue([
      {
        schema_version: 1,
        connection_type: "direct_url",
        id: "svc-1",
        name: "Cloud Pi Hub",
        base_url: "https://pi.example.com/",
        created_at: "t",
        updated_at: "t",
      },
    ]);
    render(<App />);
    expect(await screen.findByText("Cloud Pi Hub")).toBeInTheDocument();
    expect(screen.getByText(/Direct URL/)).toBeInTheDocument();
  });
});
