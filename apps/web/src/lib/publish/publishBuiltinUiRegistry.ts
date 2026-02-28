import type { ComponentRef, UiContribution } from "$lib/backend/generated";

export const PUBLISH_PLUGIN_ID = "publish";

export const PUBLISH_BUILTIN_TABS = {
  publish: {
    tabId: "publish-panel",
    side: "Left" as const,
    componentId: "publish.panel",
  },
} as const;

export type PublishBuiltinTabKey = keyof typeof PUBLISH_BUILTIN_TABS;

type SidebarTabContribution = Extract<UiContribution, { slot: "SidebarTab" }>;

export function isPublishPluginId(pluginId: unknown): boolean {
  return String(pluginId) === PUBLISH_PLUGIN_ID;
}

export function isBuiltinComponentId(
  component: ComponentRef,
  componentId: string,
): boolean {
  return component.type === "Builtin" && component.component_id === componentId;
}

export function isPublishBuiltinSidebarTab(
  contribution: SidebarTabContribution,
  key: PublishBuiltinTabKey,
): boolean {
  const def = PUBLISH_BUILTIN_TABS[key];
  return (
    contribution.id === def.tabId &&
    contribution.side === def.side &&
    isBuiltinComponentId(contribution.component, def.componentId)
  );
}

export function getPublishBuiltinTabKeyByComponentId(
  componentId: string,
): PublishBuiltinTabKey | null {
  const entry = (
    Object.entries(PUBLISH_BUILTIN_TABS) as Array<
      [
        PublishBuiltinTabKey,
        (typeof PUBLISH_BUILTIN_TABS)[PublishBuiltinTabKey],
      ]
    >
  ).find(([, value]) => value.componentId === componentId);
  return entry?.[0] ?? null;
}
