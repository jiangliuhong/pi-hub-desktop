export const PI_HUB_HOST_EXTENSION_CHANNEL = "pi-hub-host-extension";
export const PI_HUB_HOST_EXTENSION_VERSION = 1;

export const CLIENT_MENU_EXTENSION_ID = "pi-hub-client-menu";
export const RETURN_TO_SERVICES_ITEM_ID = "return_to_services";
export const OPEN_SETTINGS_ITEM_ID = "open_settings";

export interface PiHubHostExtensionRegistration {
  channel: typeof PI_HUB_HOST_EXTENSION_CHANNEL;
  protocolVersion: typeof PI_HUB_HOST_EXTENSION_VERSION;
  type: "register_extensions";
  revision: 1;
  extensions: readonly [
    {
      id: typeof CLIENT_MENU_EXTENSION_ID;
      slot: "sidebar.header.after_refresh";
      kind: "menu";
      icon: "more_horizontal";
      ariaLabel: "Pi Hub Client 菜单";
      items: readonly [
        {
          id: typeof RETURN_TO_SERVICES_ITEM_ID;
          label: "返回列表";
        },
        {
          id: typeof OPEN_SETTINGS_ITEM_ID;
          label: "设置";
        },
      ];
    },
  ];
}

export interface PiHubHostExtensionEvent {
  channel: typeof PI_HUB_HOST_EXTENSION_CHANNEL;
  protocolVersion: typeof PI_HUB_HOST_EXTENSION_VERSION;
  type: "extension_event";
  extensionId: typeof CLIENT_MENU_EXTENSION_ID;
  itemId: typeof RETURN_TO_SERVICES_ITEM_ID | typeof OPEN_SETTINGS_ITEM_ID;
  event: "activate";
}

/**
 * Declarative controls only. Pi Hub owns all DOM, styling, focus handling and
 * responsive layout; Desktop owns the declarations and resulting actions.
 */
export function createHostExtensionRegistration(): PiHubHostExtensionRegistration {
  return {
    channel: PI_HUB_HOST_EXTENSION_CHANNEL,
    protocolVersion: PI_HUB_HOST_EXTENSION_VERSION,
    type: "register_extensions",
    revision: 1,
    extensions: [
      {
        id: CLIENT_MENU_EXTENSION_ID,
        slot: "sidebar.header.after_refresh",
        kind: "menu",
        icon: "more_horizontal",
        ariaLabel: "Pi Hub Client 菜单",
        items: [
          { id: RETURN_TO_SERVICES_ITEM_ID, label: "返回列表" },
          { id: OPEN_SETTINGS_ITEM_ID, label: "设置" },
        ],
      },
    ],
  };
}

export function parsePiHubHostExtensionEvent(
  value: unknown,
): PiHubHostExtensionEvent | null {
  if (!isRecord(value)) return null;
  if (
    value.channel !== PI_HUB_HOST_EXTENSION_CHANNEL ||
    value.protocolVersion !== PI_HUB_HOST_EXTENSION_VERSION ||
    value.type !== "extension_event" ||
    value.event !== "activate"
  ) {
    return null;
  }
  if (
    value.extensionId !== CLIENT_MENU_EXTENSION_ID ||
    (value.itemId !== RETURN_TO_SERVICES_ITEM_ID &&
      value.itemId !== OPEN_SETTINGS_ITEM_ID)
  )
    return null;
  return value as unknown as PiHubHostExtensionEvent;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
