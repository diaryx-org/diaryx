import type { SettingsField } from "$lib/backend/generated";

export function getLegacyBuiltinFields(componentId: string): SettingsField[] | null {
  if (componentId !== "publish.panel") {
    return null;
  }

  return [
    {
      type: "HostWidget",
      widget_id: "namespace.guard",
    },
    {
      type: "HostWidget",
      widget_id: "namespace.site-url",
    },
    {
      type: "HostWidget",
      widget_id: "namespace.subdomain",
    },
    {
      type: "HostWidget",
      widget_id: "namespace.custom-domains",
    },
    {
      type: "HostWidget",
      widget_id: "namespace.audiences",
    },
    {
      type: "HostWidget",
      widget_id: "namespace.publish-button",
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
