import type { SettingsField } from "$lib/backend/generated";

export function getLegacyBuiltinFields(componentId: string): SettingsField[] | null {
  if (componentId !== "publish.panel") {
    return null;
  }

  return [
    {
      type: "HostWidget",
      widget_id: "namespace.guard",
      sign_in_action: {
        action_type: "open-settings",
        payload: { tab: "account" },
      },
    },
    {
      type: "HostWidget",
      widget_id: "namespace.site-url",
      sign_in_action: null,
    },
    {
      type: "HostWidget",
      widget_id: "namespace.subdomain",
      sign_in_action: null,
    },
    {
      type: "HostWidget",
      widget_id: "namespace.custom-domains",
      sign_in_action: null,
    },
    {
      type: "HostWidget",
      widget_id: "namespace.audiences",
      sign_in_action: null,
    },
    {
      type: "HostWidget",
      widget_id: "namespace.publish-button",
      sign_in_action: null,
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
