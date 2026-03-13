import { toast } from "svelte-sonner";

import type { Backend, AppUpdateInfo } from "$lib/backend/interface";

import { showLoading } from "./toastService";

function getUpdateDescription(body: string | null): string {
  const normalized = body?.trim();
  if (!normalized) {
    return "Download and restart to install the latest Diaryx release.";
  }

  const firstLine = normalized.split(/\r?\n/, 1)[0]?.trim() ?? "";
  return firstLine.length > 160 ? `${firstLine.slice(0, 157)}...` : firstLine;
}

async function readAvailableUpdate(backend: Backend): Promise<AppUpdateInfo | null> {
  if (typeof backend.checkForAppUpdate !== "function") {
    return null;
  }

  return await backend.checkForAppUpdate();
}

export async function checkForAppUpdatesInBackground(backend: Backend): Promise<void> {
  try {
    const update = await readAvailableUpdate(backend);
    if (!update) {
      return;
    }

    toast.info(`Diaryx ${update.version} is ready to install`, {
      description: getUpdateDescription(update.body),
      duration: 15000,
      action: {
        label: "Install",
        onClick: () => {
          void installAvailableAppUpdate(backend, update.version);
        },
      },
    });
  } catch (error) {
    console.warn("[updaterService] Failed to check for app updates:", error);
  }
}

export async function installAvailableAppUpdate(
  backend: Backend,
  versionLabel?: string,
): Promise<boolean> {
  if (typeof backend.installAppUpdate !== "function") {
    return false;
  }

  const loading = showLoading(
    versionLabel
      ? `Installing Diaryx ${versionLabel}...`
      : "Installing the latest Diaryx update...",
  );

  try {
    const installed = await backend.installAppUpdate();
    if (!installed) {
      loading.dismiss();
      return false;
    }

    loading.update("Restarting Diaryx...");
    return true;
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "Failed to install app update";
    loading.error(message);
    return false;
  }
}
