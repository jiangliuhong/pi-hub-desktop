import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ViewerToolbar } from "./ViewerToolbar";
import { createHostExtensionRegistration } from "./bridge";

const mocks = vi.hoisted(() => ({
  getConnectionStatus: vi.fn(),
  openServiceView: vi.fn(),
  closeServiceView: vi.fn(),
}));

vi.mock("../connection/api", () => ({
  getConnectionStatus: mocks.getConnectionStatus,
}));
vi.mock("./api", () => ({
  openServiceView: mocks.openServiceView,
  closeServiceView: mocks.closeServiceView,
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
