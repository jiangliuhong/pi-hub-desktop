import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { App } from "./App";

describe("App", () => {
  it("renders the trusted app shell header over the home route", async () => {
    render(<App />);
    // Header is present immediately.
    expect(screen.getByText("Pi Hub Client")).toBeInTheDocument();
    // Await the async list effect so the settled state is covered by act().
    expect(await screen.findByText(/还没有 Pi Hub 服务/)).toBeInTheDocument();
  });

  it("exposes the add-service entry point from the empty state", async () => {
    render(<App />);
    expect(await screen.findByText(/添加服务/)).toBeInTheDocument();
  });
});
