import { describe, expect, it } from "vitest";
import {
  createHostExtensionRegistration,
  parsePiHubHostExtensionEvent,
} from "./bridge";

describe("Pi Hub host extension bridge", () => {
  it("registers a declarative menu in the sidebar header slot", () => {
    expect(createHostExtensionRegistration()).toEqual({
      channel: "pi-hub-host-extension",
      protocolVersion: 1,
      type: "register_extensions",
      revision: 1,
      extensions: [
        {
          id: "pi-hub-client-menu",
          slot: "sidebar.header.after_refresh",
          kind: "menu",
          icon: "more_horizontal",
          ariaLabel: "Pi Hub Client 菜单",
          items: [
            { id: "return_to_services", label: "返回列表" },
            { id: "open_settings", label: "设置" },
          ],
        },
      ],
    });
  });

  it("accepts the allowlisted menu activation event", () => {
    expect(
      parsePiHubHostExtensionEvent({
        channel: "pi-hub-host-extension",
        protocolVersion: 1,
        type: "extension_event",
        extensionId: "pi-hub-client-menu",
        itemId: "return_to_services",
        event: "activate",
      }),
    ).not.toBeNull();
  });

  it("accepts the allowlisted settings activation event", () => {
    expect(
      parsePiHubHostExtensionEvent({
        channel: "pi-hub-host-extension",
        protocolVersion: 1,
        type: "extension_event",
        extensionId: "pi-hub-client-menu",
        itemId: "open_settings",
        event: "activate",
      }),
    ).not.toBeNull();
  });

  it.each([
    null,
    [],
    {
      channel: "other",
      protocolVersion: 1,
      type: "extension_event",
      extensionId: "pi-hub-client-menu",
      itemId: "return_to_services",
      event: "activate",
    },
    {
      channel: "pi-hub-host-extension",
      protocolVersion: 2,
      type: "extension_event",
      extensionId: "pi-hub-client-menu",
      itemId: "return_to_services",
      event: "activate",
    },
    {
      channel: "pi-hub-host-extension",
      protocolVersion: 1,
      type: "extension_event",
      extensionId: "unknown-extension",
      itemId: "return_to_services",
      event: "activate",
    },
    {
      channel: "pi-hub-host-extension",
      protocolVersion: 1,
      type: "extension_event",
      extensionId: "pi-hub-client-menu",
      itemId: "execute",
      event: "activate",
    },
  ])("rejects unsupported event payloads", (payload) => {
    expect(parsePiHubHostExtensionEvent(payload)).toBeNull();
  });
});
