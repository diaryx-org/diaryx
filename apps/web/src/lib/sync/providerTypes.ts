import type { PluginId } from "$lib/backend/generated";

export type ProviderId = string;

export interface ProviderCapabilities {
  available: boolean;
  canLink: boolean;
  canDownload: boolean;
  canListRemote: boolean;
  unavailableReason?: string;
}

export interface ProviderContribution {
  id: ProviderId;
  label: string;
  description?: string | null;
}

export interface WorkspaceProviderDescriptor {
  pluginId: PluginId | string;
  contribution: ProviderContribution;
  source: "plugin" | "builtin";
  capabilities?: ProviderCapabilities;
}
