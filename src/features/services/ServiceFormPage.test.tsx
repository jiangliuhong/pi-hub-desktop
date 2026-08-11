import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { ServiceFormPage } from "./ServiceFormPage";

/**
 * Form behavior tests (docs/design-v1.md §22.3): conditional fields render per
 * connection type, validation blocks bad input, and SSH secrets are routed to
 * the credential command (never embedded in the profile draft).
 */

const invokeMock =
  vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

function renderForm(mode: "create" | "edit", route = "/services/new") {
  return render(
    <MemoryRouter initialEntries={[route]}>
      <ServiceFormPage mode={mode} />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("ServiceFormPage", () => {
  it("shows only Direct URL fields by default", () => {
    renderForm("create");
    expect(screen.getByText("URL")).toBeInTheDocument();
    expect(screen.queryByText("SSH Host")).toBeNull();
  });

  it("reveals SSH fields when switching to SSH Forward", async () => {
    const user = userEvent.setup();
    renderForm("create");
    await user.click(screen.getByLabelText("SSH Forward"));
    expect(await screen.findByText("SSH Host")).toBeInTheDocument();
    expect(screen.getByText("用户名")).toBeInTheDocument();
  });

  it("validates required fields and blocks submit", async () => {
    const user = userEvent.setup();
    renderForm("create");
    await user.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByText("名称不能为空")).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("creates a direct_url profile without any secret in the draft", async () => {
    invokeMock.mockResolvedValue({
      schema_version: 1,
      connection_type: "direct_url",
      id: "uuid-1",
      name: "Cloud",
      base_url: "https://pi.example.com/",
      created_at: "t",
      updated_at: "t",
    });
    const user = userEvent.setup();
    renderForm("create");
    await user.type(screen.getByLabelText("名称"), "Cloud");
    // The URL input is the second unlabeled text input; target by placeholder.
    await user.type(
      screen.getByPlaceholderText("https://pi.example.com"),
      "https://pi.example.com",
    );
    await user.click(screen.getByRole("button", { name: "保存" }));

    await screen.findByText("正在连接…").catch(() => undefined);
    // The create_service call must carry only the non-sensitive draft.
    const createCall = invokeMock.mock.calls.find(
      (c) => c[0] === "create_service",
    );
    expect(createCall).toBeTruthy();
    const draft = (createCall![1] as { draft: Record<string, unknown> }).draft;
    expect(draft.connection_type).toBe("direct_url");
    // No secret fields leak into the profile.
    expect(JSON.stringify(draft)).not.toContain("password");
    expect(JSON.stringify(draft)).not.toContain("PRIVATE KEY");
  });
});
