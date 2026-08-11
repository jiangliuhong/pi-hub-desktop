import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue([]);
  });

  it("renders the trusted app shell header over the home route", async () => {
    render(<App />);
    expect(screen.getByText("Pi Hub Client")).toBeInTheDocument();
    expect(await screen.findByText(/还没有 Pi Hub 服务/)).toBeInTheDocument();
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
