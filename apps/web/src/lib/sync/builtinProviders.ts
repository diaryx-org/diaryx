import { getBackendSync, isTauri } from "$lib/backend";
import type { ProviderCapabilities, WorkspaceProviderDescriptor } from "./providerTypes";

export const BUILTIN_ICLOUD_PROVIDER_ID = "builtin.icloud";

function getAppleTauriCapabilities(): ProviderCapabilities {
  return {
    available: true,
    canLink: true,
    canDownload: false,
    canListRemote: false,
  };
}

export function isAppleTauriRuntime(): boolean {
  if (!isTauri()) {
    return false;
  }

  try {
    return getBackendSync().getAppPaths()?.is_apple_build === true;
  } catch {
    return false;
  }
}

export function getBuiltinWorkspaceProviders(): WorkspaceProviderDescriptor[] {
  if (!isAppleTauriRuntime()) {
    return [];
  }

  return [
    {
      pluginId: BUILTIN_ICLOUD_PROVIDER_ID,
      contribution: {
        id: BUILTIN_ICLOUD_PROVIDER_ID,
        label: "iCloud Drive",
        description: "Store this workspace in iCloud Drive on Apple devices.",
      },
      source: "builtin",
      capabilities: getAppleTauriCapabilities(),
    },
  ];
}

export function isBuiltinProvider(providerId: string): boolean {
  return providerId.startsWith("builtin.");
}

export function getBuiltinProvider(providerId: string): WorkspaceProviderDescriptor | null {
  return getBuiltinWorkspaceProviders().find(
    (provider) => provider.contribution.id === providerId,
  ) ?? null;
}
