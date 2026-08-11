import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HostKeyDialog } from "./HostKeyDialog";
import type { HostKeyChallengeDto } from "./api";

/**
 * Host Key confirmation UI (docs/requirements-v1.md FR-007 / FR-008).
 * First-time connect must show host, port, algorithm and SHA-256 fingerprint,
 * and require explicit accept/reject — never auto-accept.
 */
const challenge: HostKeyChallengeDto = {
  challenge_id: "ch-1",
  connection_id: "conn-1",
  service_id: "svc-1",
  ssh_host: "vps.example.com",
  ssh_port: 22,
  algorithm: "ssh-ed25519",
  sha256_fingerprint: "SHA256:abc123",
};

describe("HostKeyDialog", () => {
  it("shows host, port, algorithm and fingerprint", () => {
    render(<HostKeyDialog challenge={challenge} onRespond={vi.fn()} />);
    expect(screen.getByText(/vps.example.com:22/)).toBeInTheDocument();
    expect(screen.getByText("ssh-ed25519")).toBeInTheDocument();
    expect(screen.getByText("SHA256:abc123")).toBeInTheDocument();
  });

  it("requires explicit accept and forwards the decision", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    render(<HostKeyDialog challenge={challenge} onRespond={onRespond} />);
    await user.click(screen.getByRole("button", { name: "确认并连接" }));
    expect(onRespond).toHaveBeenCalledWith(true);
  });

  it("forwards an explicit reject", async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn();
    render(<HostKeyDialog challenge={challenge} onRespond={onRespond} />);
    await user.click(screen.getByRole("button", { name: "拒绝" }));
    expect(onRespond).toHaveBeenCalledWith(false);
  });
});
