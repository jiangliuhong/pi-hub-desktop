import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ViewerToolbar } from "./ViewerToolbar";
import { createHostExtensionRegistration } from "./bridge";

const mocks = vi.hoisted(() => ({
  getConnectionStatus: vi.fn(),
  openServiceView: vi.fn(),
  closeServiceView: vi.fn(),
  // Captures the state-changed listener registered by ViewerToolbar so a test
  // can simulate an SSH reconnect event (plan §5.5.4).
  stateChangedHandler: null as ((e: { payload: unknown }) => void) | null,
}));

vi.mock("../connection/api", () => ({
  getConnectionStatus: mocks.getConnectionStatus,
  STATE_CHANGED_EVENT: "connection://state-changed",
}));
vi.mock("./api", () => ({
  openServiceView: mocks.openServiceView,
  closeServiceView: mocks.closeServiceView,
}));
vi.mock("@tauri-apps/api/event", () => ({
  // listen captures the handler; returns an unlisten stub.
  listen: (_event: string, handler: (e: { payload: unknown }) => void) => {
    mocks.stateChangedHandler = handler;
    return Promise.resolve(vi.fn());
  },
}));

describe("ViewerToolbar", () => {
  beforeEach(() => {
    mocks.getConnectionStatus.mockReset();
    mocks.openServiceView.mockReset();
    mocks.closeServiceView.mockReset();
    mocks.getConnectionStatus.mockResolvedValue({
      state: "connected",
      effective_url: "https://pi.example.com/",
      diagnostics: {},
    });
    mocks.openServiceView.mockResolvedValue({
      service_id: "service-1",
      allowed_origin: "https://pi.example.com",
    });
    mocks.closeServiceView.mockResolvedValue(undefined);
  });

  it("renders the connected Pi Hub page full-window", async () => {
    renderViewer();

    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");
    expect(frame).toHaveAttribute("src", "https://pi.example.com/");
    expect(frame).toHaveAttribute(
      "allow",
      "clipboard-read https://pi.example.com; clipboard-write https://pi.example.com",
    );
    expect(
      screen.queryByLabelText("打开 Pi Hub Client 菜单"),
    ).not.toBeInTheDocument();
  });

  it("registers declarative host extensions with the exact service origin", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");
    const postMessage = vi.spyOn(frame.contentWindow!, "postMessage");

    fireEvent.load(frame);

    expect(postMessage).toHaveBeenCalledWith(
      createHostExtensionRegistration(),
      "https://pi.example.com",
    );
  });

  it("accepts an allowlisted extension event only from the active frame and origin", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");

    dispatchReturnEvent(frame, "https://pi.example.com");

    await waitFor(() =>
      expect(mocks.closeServiceView).toHaveBeenCalledWith("service-1"),
    );
    expect(await screen.findByText("服务列表")).toBeInTheDocument();
  });

  it("opens Desktop settings from the allowlisted settings action", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");
    const openSettings = vi.fn();
    window.addEventListener("app:open-settings", openSettings);

    dispatchSettingsEvent(frame, "https://pi.example.com");

    await waitFor(() => expect(openSettings).toHaveBeenCalledTimes(1));
    window.removeEventListener("app:open-settings", openSettings);
  });

  it("ignores extension events from another origin", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");

    dispatchReturnEvent(frame, "https://attacker.example");

    expect(mocks.closeServiceView).not.toHaveBeenCalled();
  });

  it("reloads onto a new effective URL when SSH reconnects (plan §5.5.4)", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");
    expect(frame).toHaveAttribute("src", "https://pi.example.com/");

    // Simulate the Rust manager emitting a state-changed event after an SSH
    // reconnect, with a new loopback effective URL.
    expect(mocks.stateChangedHandler).not.toBeNull();
    act(() => {
      mocks.stateChangedHandler!({
        payload: {
          service_id: "service-1",
          state: "connected",
          effective_url: "http://127.0.0.1:54321/",
        },
      });
    });

    // The iframe must switch to the new URL (declarative reload) and
    // openServiceView must be re-invoked to register the new allowed origin.
    await waitFor(() =>
      expect(frame).toHaveAttribute("src", "http://127.0.0.1:54321/"),
    );
    expect(mocks.openServiceView).toHaveBeenCalledWith(
      "service-1",
      "http://127.0.0.1:54321/",
    );
  });

  it("ignores state-changed events for a different service", async () => {
    renderViewer();
    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");

    mocks.stateChangedHandler!({
      payload: {
        service_id: "other-service",
        state: "connected",
        effective_url: "http://127.0.0.1:9999/",
      },
    });

    expect(frame).toHaveAttribute("src", "https://pi.example.com/");
    expect(mocks.openServiceView).not.toHaveBeenCalledWith(
      "service-1",
      "http://127.0.0.1:9999/",
    );
  });
});

function dispatchReturnEvent(frame: HTMLIFrameElement, origin: string) {
  window.dispatchEvent(
    new MessageEvent("message", {
      source: frame.contentWindow,
      origin,
      data: {
        channel: "pi-hub-host-extension",
        protocolVersion: 1,
        type: "extension_event",
        extensionId: "pi-hub-client-menu",
        itemId: "return_to_services",
        event: "activate",
      },
    }),
  );
}

function dispatchSettingsEvent(frame: HTMLIFrameElement, origin: string) {
  window.dispatchEvent(
    new MessageEvent("message", {
      source: frame.contentWindow,
      origin,
      data: {
        channel: "pi-hub-host-extension",
        protocolVersion: 1,
        type: "extension_event",
        extensionId: "pi-hub-client-menu",
        itemId: "open_settings",
        event: "activate",
      },
    }),
  );
}

function renderViewer() {
  return render(
    <MemoryRouter initialEntries={["/viewer/service-1"]}>
      <Routes>
        <Route path="/viewer/:id" element={<ViewerToolbar />} />
        <Route path="/" element={<p>服务列表</p>} />
      </Routes>
    </MemoryRouter>,
  );
}
