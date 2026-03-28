import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { getBackend } from "$lib/backend";
import { BUILTIN_ICLOUD_PROVIDER_ID, isAppleTauriRuntime } from "./builtinProviders";
import type { ProviderCapabilities } from "./providerTypes";

type ProviderCommandParams = Record<string, JsonValue>;

function unsupported(message: string): never {
  throw new Error(message);
}

async function invokeIcloud<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

async function getCapabilities(): Promise<ProviderCapabilities> {
  if (!isAppleTauriRuntime()) {
    return {
      available: false,
      canLink: false,
      canDownload: false,
      canListRemote: false,
      unavailableReason: "iCloud Drive is only available in Apple Tauri builds.",
    };
  }

  return {
    available: true,
    canLink: true,
    canDownload: false,
    canListRemote: false,
  };
}

export async function executeBuiltinIcloudProviderCommand<T = JsonValue>(args: {
  api: Api;
  command: string;
  params?: ProviderCommandParams;
}): Promise<T> {
  const { command } = args;

  switch (command) {
    case "GetProviderStatus": {
      const capabilities = await getCapabilities();
      if (!capabilities.available) {
        return {
          ready: false,
          message: capabilities.unavailableReason ?? "iCloud Drive is unavailable.",
        } as T;
      }

      try {
        const result = await invokeIcloud<{ isAvailable: boolean }>(
          "plugin:icloud|check_icloud_available",
        );
        return {
          ready: result.isAvailable,
          message: result.isAvailable
            ? null
            : "iCloud is not available on this device. Sign in to iCloud in Settings.",
        } as T;
      } catch (error) {
        return {
          ready: false,
          message: error instanceof Error ? error.message : String(error),
        } as T;
      }
    }

    case "ListRemoteWorkspaces":
      return { workspaces: [] } as T;

    case "LinkWorkspace": {
      const capabilities = await getCapabilities();
      if (!capabilities.canLink) {
        unsupported(
          capabilities.unavailableReason ?? "This platform cannot link iCloud workspaces.",
        );
      }

      const backend = await getBackend();
      const appPaths = backend.getAppPaths();
      if (appPaths?.icloud_active === true) {
        const remoteId = String(appPaths.icloud_workspace ?? "Diaryx");
        return {
          remote_id: remoteId,
          created_remote: false,
          snapshot_uploaded: false,
        } as T;
      }

      const result = await invokeIcloud<Record<string, string | boolean | null>>(
        "set_icloud_enabled",
        { enabled: true },
      );

      return {
        remote_id: String(result.icloud_workspace ?? result.default_workspace ?? BUILTIN_ICLOUD_PROVIDER_ID),
        created_remote: false,
        snapshot_uploaded: false,
      } as T;
    }

    case "UnlinkWorkspace":
      await invokeIcloud("set_icloud_enabled", { enabled: false });
      return undefined as T;

    case "DownloadWorkspace":
      unsupported("iCloud Drive workspaces cannot be downloaded on this platform.");

    default:
      unsupported(`Unsupported built-in iCloud provider command: ${command}`);
  }
}
