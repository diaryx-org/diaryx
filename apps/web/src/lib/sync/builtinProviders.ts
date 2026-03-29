import { getBackendSync, isTauri } from "$lib/backend";
import type { ProviderCapabilities, WorkspaceProviderDescriptor } from "./providerTypes";

export const BUILTIN_ICLOUD_PROVIDER_ID = "builtin.icloud";
export const ICLOUD_NAMESPACE_PREFIX = "workspace:icloud:";
export const ICLOUD_LOCAL_PREFIX = `${BUILTIN_ICLOUD_PROVIDER_ID}:`;
const ICLOUD_LABEL = "iCloud Drive";
const ICLOUD_DESCRIPTION = "Store this workspace in iCloud Drive on Apple devices.";
const ICLOUD_UNAVAILABLE_REASON =
  "Available only on Diaryx for Apple devices with iCloud Drive support.";

function getAppleTauriCapabilities(): ProviderCapabilities {
  return {
    available: true,
    canLink: true,
    canDownload: true,
    canListRemote: true,
  };
}

function getUnavailableBuiltinCapabilities(reason: string): ProviderCapabilities {
  return {
    available: false,
    canLink: false,
    canDownload: false,
    canListRemote: false,
    unavailableReason: reason,
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
        label: ICLOUD_LABEL,
        description: ICLOUD_DESCRIPTION,
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

export function getProviderDisplayLabel(providerId: string): string | null {
  if (providerId === BUILTIN_ICLOUD_PROVIDER_ID) {
    return ICLOUD_LABEL;
  }
  return null;
}

export function getProviderCapabilities(providerId: string): ProviderCapabilities | null {
  if (providerId === BUILTIN_ICLOUD_PROVIDER_ID) {
    return isAppleTauriRuntime()
      ? getAppleTauriCapabilities()
      : getUnavailableBuiltinCapabilities(ICLOUD_UNAVAILABLE_REASON);
  }
  return null;
}

export function isProviderAvailableHere(providerId: string): boolean {
  return getProviderCapabilities(providerId)?.available ?? true;
}

export function getProviderUnavailableReason(providerId: string): string | null {
  return getProviderCapabilities(providerId)?.unavailableReason ?? null;
}

export function makeIcloudNamespaceId(workspaceKey: string): string {
  return `${ICLOUD_NAMESPACE_PREFIX}${workspaceKey}`;
}

export function makeLocalIcloudRemoteId(workspaceKey: string): string {
  return `${ICLOUD_LOCAL_PREFIX}${workspaceKey}`;
}

export function getIcloudWorkspaceKeyFromRemoteId(remoteId: string | null | undefined): string | null {
  if (!remoteId) return null;
  if (remoteId.startsWith(ICLOUD_NAMESPACE_PREFIX)) {
    return remoteId.slice(ICLOUD_NAMESPACE_PREFIX.length) || null;
  }
  if (remoteId.startsWith(ICLOUD_LOCAL_PREFIX)) {
    return remoteId.slice(ICLOUD_LOCAL_PREFIX.length) || null;
  }
  return null;
}
