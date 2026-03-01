import type { ComponentRef, UiContribution } from "$lib/backend/generated";

export const SYNC_PLUGIN_ID = "sync";

/** Built-in component ID for the sync settings tab. */
export const SYNC_BUILTIN_SETTINGS_COMPONENT_ID = "sync.settings";

export const SYNC_BUILTIN_TABS = {
  share: {
    tabId: "share",
    side: "Left" as const,
    componentId: "sync.share",
  },
  snapshots: {
    tabId: "snapshots",
    side: "Left" as const,
    componentId: "sync.snapshots",
  },
  history: {
    tabId: "history",
    side: "Right" as const,
    componentId: "sync.history",
  },
} as const;

export const SYNC_STATUS_ITEM_IDS = ["sync-status", "sync.status"] as const;

export type SyncBuiltinTabKey = keyof typeof SYNC_BUILTIN_TABS;
export type SyncBuiltinComponentId =
  (typeof SYNC_BUILTIN_TABS)[SyncBuiltinTabKey]["componentId"];

type SidebarTabContribution = Extract<UiContribution, { slot: "SidebarTab" }>;
type StatusBarContribution = Extract<UiContribution, { slot: "StatusBarItem" }>;

export function isSyncPluginId(pluginId: unknown): boolean {
  return String(pluginId) === SYNC_PLUGIN_ID;
}

export function isBuiltinComponentId(
  component: ComponentRef,
  componentId: string,
): boolean {
  return component.type === "Builtin" && component.component_id === componentId;
}

export function isSyncBuiltinSidebarTab(
  contribution: SidebarTabContribution,
  key: SyncBuiltinTabKey,
): boolean {
  const def = SYNC_BUILTIN_TABS[key];
  return (
    contribution.id === def.tabId &&
    contribution.side === def.side &&
    isBuiltinComponentId(contribution.component, def.componentId)
  );
}

export function getSyncBuiltinTabKeyByComponentId(
  componentId: string,
): SyncBuiltinTabKey | null {
  const entry = (Object.entries(SYNC_BUILTIN_TABS) as Array<
    [SyncBuiltinTabKey, (typeof SYNC_BUILTIN_TABS)[SyncBuiltinTabKey]]
  >).find(([, value]) => value.componentId === componentId);
  return entry?.[0] ?? null;
}

export function isSyncStatusBarItem(
  contribution: StatusBarContribution,
): boolean {
  return (SYNC_STATUS_ITEM_IDS as readonly string[]).includes(contribution.id);
}
