import { isTauri } from "$lib/backend/interface";
import {
  getPluginInstallPath,
  readWorkspaceBinary,
  writeWorkspaceBinary,
} from "$lib/workspace/workspaceAssetStorage";

export async function captureProviderPluginForTransfer(
  pluginId: string | null,
): Promise<Uint8Array | null> {
  if (!pluginId || isTauri()) {
    return null;
  }

  return await readWorkspaceBinary(getPluginInstallPath(pluginId));
}

export async function installCapturedProviderPlugin(
  pluginId: string | null,
  wasmBytes: Uint8Array | null,
): Promise<void> {
  if (!pluginId || !wasmBytes || isTauri()) {
    return;
  }

  const installPath = getPluginInstallPath(pluginId);
  await writeWorkspaceBinary(installPath, wasmBytes);
}
