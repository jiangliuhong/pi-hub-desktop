import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LocalRuntimeViewer } from "./LocalRuntimeViewer";

const mocks = vi.hoisted(() => ({
  scanLocalInstallations: vi.fn(),
}));

vi.mock("./api", () => ({
  scanLocalInstallations: mocks.scanLocalInstallations,
}));

describe("LocalRuntimeViewer", () => {
  beforeEach(() => {
    mocks.scanLocalInstallations.mockReset();
  });

  it("actively verifies and loads a running local Pi Hub in-app", async () => {
    mocks.scanLocalInstallations.mockResolvedValue({
      runtime_state: "running_managed",
      effective_url: "http://127.0.0.1:30142",
    });

    renderViewer();

    const frame = await screen.findByTitle<HTMLIFrameElement>("Pi Hub");
    expect(frame).toHaveAttribute("src", "http://127.0.0.1:30142");
    expect(frame).toHaveAttribute(
      "allow",
      "clipboard-read http://127.0.0.1:30142; clipboard-write http://127.0.0.1:30142",
    );
    expect(mocks.scanLocalInstallations).toHaveBeenCalledTimes(1);
  });

  it("refuses to load a stale URL when the local service is stopped", async () => {
    mocks.scanLocalInstallations.mockResolvedValue({
      runtime_state: "stopped",
      effective_url: undefined,
    });

    renderViewer();

    expect(
      await screen.findByText(
        "本机 Pi Hub 当前不可连接，请返回列表重新检测或启动。",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByTitle("Pi Hub")).not.toBeInTheDocument();
  });
});

function renderViewer() {
  return render(
    <MemoryRouter initialEntries={["/local-runtime/viewer"]}>
      <Routes>
        <Route path="/local-runtime/viewer" element={<LocalRuntimeViewer />} />
        <Route path="/" element={<p>服务列表</p>} />
      </Routes>
    </MemoryRouter>,
  );
}
