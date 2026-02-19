/**
 * Shared UI status for workspace sync actions initiated from settings.
 *
 * This is used to surface "Start syncing" progress in both:
 * - Account tab (`WorkspaceManagement`)
 * - Sync tab (`SyncSettings`)
 */

export type SyncActionTone = "info" | "success" | "error";

export interface SyncActionStatus {
  active: boolean;
  workspaceId: string | null;
  workspaceName: string | null;
  progress: number;
  message: string | null;
  tone: SyncActionTone;
}

let syncActionStatus = $state<SyncActionStatus>({
  active: false,
  workspaceId: null,
  workspaceName: null,
  progress: 0,
  message: null,
  tone: "info",
});

let clearTimer: ReturnType<typeof setTimeout> | null = null;

function cancelClearTimer() {
  if (clearTimer) {
    clearTimeout(clearTimer);
    clearTimer = null;
  }
}

export function getSyncActionStatus(): SyncActionStatus {
  return syncActionStatus;
}

export function setSyncActionStatus(status: Partial<SyncActionStatus>): void {
  cancelClearTimer();
  syncActionStatus = {
    ...syncActionStatus,
    ...status,
  };
}

export function resetSyncActionStatus(): void {
  cancelClearTimer();
  syncActionStatus = {
    active: false,
    workspaceId: null,
    workspaceName: null,
    progress: 0,
    message: null,
    tone: "info",
  };
}

export function completeSyncActionStatus(
  tone: Extract<SyncActionTone, "success" | "error">,
  message: string,
  autoClearMs = 5000,
): void {
  cancelClearTimer();
  syncActionStatus = {
    ...syncActionStatus,
    active: false,
    progress: 100,
    tone,
    message,
  };
  clearTimer = setTimeout(() => {
    resetSyncActionStatus();
  }, autoClearMs);
}
