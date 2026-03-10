import type { SettingsField } from "$lib/backend/generated";

export function getLegacyBuiltinFields(componentId: string): SettingsField[] | null {
  if (componentId !== "publish.panel") {
    return null;
  }

  return [
    {
      type: "HostWidget",
      widget_id: "publish.site-panel",
    },
    {
      type: "Section",
      label: "Export",
      description: "Export this workspace to markdown, HTML, or converter-based formats.",
    },
    {
      type: "HostActionButton",
      label: "Export Workspace",
      action_type: "open-export-dialog",
      variant: "outline",
    },
  ];
}
