import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { getBackend } from "$lib/backend";
import {
  BUILTIN_ICLOUD_PROVIDER_ID,
  isAppleTauriRuntime,
  makeLocalIcloudRemoteId,
} from "./builtinProviders";
import type { ProviderCapabilities } from "./providerTypes";

type ProviderCommandParams = Record<string, JsonValue>;

interface IcloudWorkspaceInfo {
  isAvailable: boolean;
  hasWorkspace: boolean;
  workspacePath?: string | null;
  workspaceName?: string | null;
  active: boolean;
}

interface IcloudWorkspaceRecord {
  workspaceId: string;
  workspaceName: string;
  workspacePath: string;
  active: boolean;
}

function unsupported(message: string): never {
  throw new Error(message);
}

async function invokeIcloud<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

async function getWorkspaceInfo(): Promise<IcloudWorkspaceInfo> {
  if (!isAppleTauriRuntime()) {
    return {
      isAvailable: false,
      hasWorkspace: false,
      workspacePath: null,
      workspaceName: null,
      active: false,
    };
  }

  return await invokeIcloud<IcloudWorkspaceInfo>("get_icloud_workspace_info");
}

async function listNativeWorkspaces(): Promise<IcloudWorkspaceRecord[]> {
  if (!isAppleTauriRuntime()) {
    return [];
  }

  return await invokeIcloud<IcloudWorkspaceRecord[]>("list_icloud_workspaces");
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
    canDownload: true,
    canListRemote: true,
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
        const result = await getWorkspaceInfo();
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
      {
        const workspaces = await listNativeWorkspaces();
        return {
          workspaces: workspaces.map((workspace) => ({
            id: workspace.workspaceId,
            name: workspace.workspaceName,
          })),
        } as T;
      }

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
        const activePath = typeof appPaths.icloud_workspace === "string"
          ? appPaths.icloud_workspace
          : "";
        const workspaceKey = activePath.split("/").filter(Boolean).at(-1) ?? "";
        const remoteId = workspaceKey
          ? makeLocalIcloudRemoteId(workspaceKey)
          : BUILTIN_ICLOUD_PROVIDER_ID;
        return {
          remote_id: remoteId,
          created_remote: false,
          snapshot_uploaded: false,
        } as T;
      }

      const requestedRemoteId =
        typeof args.params?.remote_id === "string" ? args.params.remote_id : null;
      const workspaceName =
        typeof args.params?.name === "string" ? args.params.name : null;
      const result = await invokeIcloud<Record<string, string | boolean | null>>(
        "link_icloud_workspace",
        {
          workspaceId: requestedRemoteId,
          workspaceName,
        },
      );
      const resultPath = typeof result.icloud_workspace === "string" ? result.icloud_workspace : "";
      const resultKey = resultPath.split("/").filter(Boolean).at(-1) ?? "";
      const remoteId = requestedRemoteId
        ?? (resultKey ? makeLocalIcloudRemoteId(resultKey) : BUILTIN_ICLOUD_PROVIDER_ID);

      return {
        remote_id: remoteId,
        created_remote: false,
        snapshot_uploaded: false,
      } as T;
    }

    case "UnlinkWorkspace":
      await invokeIcloud("set_icloud_enabled", { enabled: false });
      return undefined as T;

    case "DownloadWorkspace": {
      const requestedRemoteId =
        typeof args.params?.remote_id === "string" ? args.params.remote_id : null;
      await invokeIcloud("restore_icloud_workspace", {
        workspaceId: requestedRemoteId,
      });
      return {
        files_imported: 0,
      } as T;
    }

    default:
      unsupported(`Unsupported built-in iCloud provider command: ${command}`);
  }
}
