import { describe, expect, it } from "vitest";
import {
  validateDirectUrl,
  validatePort,
  validateSshUsername,
} from "./validation";

describe("validateDirectUrl", () => {
  it("accepts a well-formed https URL", () => {
    expect(validateDirectUrl("https://pi.example.com")).toEqual({ ok: true });
  });

  it("accepts a loopback http URL", () => {
    expect(validateDirectUrl("http://127.0.0.1:30142")).toEqual({ ok: true });
  });

  it("rejects empty input", () => {
    expect(validateDirectUrl("   ")).toEqual({
      ok: false,
      reason: "URL 不能为空",
    });
  });

  it("rejects non-http schemes", () => {
    expect(validateDirectUrl("ftp://example.com").ok).toBe(false);
    expect(validateDirectUrl("file:///etc/hosts").ok).toBe(false);
  });

  it("rejects malformed URLs", () => {
    expect(validateDirectUrl("not a url").ok).toBe(false);
  });
});

describe("validatePort", () => {
  it.each([1, 22, 30142, 65535])("accepts %i", (port) => {
    expect(validatePort(port)).toEqual({ ok: true });
  });

  it.each([0, -1, 65536])("rejects %i as out of range", (port) => {
    expect(validatePort(port).ok).toBe(false);
  });

  it("rejects non-integer values", () => {
    expect(validatePort(22.5).ok).toBe(false);
  });
});

describe("validateSshUsername", () => {
  it("accepts a non-empty username", () => {
    expect(validateSshUsername("ubuntu")).toEqual({ ok: true });
  });

  it("rejects whitespace-only input", () => {
    expect(validateSshUsername("  ").ok).toBe(false);
  });
});
