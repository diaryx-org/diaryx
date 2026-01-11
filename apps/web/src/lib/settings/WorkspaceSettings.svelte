<script lang="ts">
  /**
   * WorkspaceSettings - Workspace folder configuration
   *
   * Shows the current workspace path and allows changing it (Tauri only).
   */
  import { Button } from "$lib/components/ui/button";
  import { FolderOpen, RefreshCw } from "@lucide/svelte";
  import { getBackend, isTauri } from "../backend";

  // Current workspace path
  let workspacePath = $state<string | null>(null);
  let isChanging = $state(false);
  let error = $state<string | null>(null);

  // Load workspace path on mount
  $effect(() => {
    loadWorkspacePath();
  });

  async function loadWorkspacePath() {
    try {
      const backend = await getBackend();
      const appPaths = backend.getAppPaths();
      if (appPaths?.default_workspace) {
        workspacePath = appPaths.default_workspace as string;
      }
    } catch (e) {
      console.warn("[WorkspaceSettings] Failed to load workspace path:", e);
    }
  }

  async function pickFolder() {
    if (!isTauri()) return;

    isChanging = true;
    error = null;

    try {
      // Dynamically import Tauri API
      const { invoke } = await import("@tauri-apps/api/core");

      // Call the pick_workspace_folder command
      const result = await invoke<{ default_workspace: string } | null>("pick_workspace_folder");

      if (result) {
        workspacePath = result.default_workspace;
        // Reload the page to use the new workspace
        window.location.reload();
      }
      // If result is null, user cancelled - do nothing
    } catch (e) {
      console.error("[WorkspaceSettings] Failed to pick folder:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isChanging = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <FolderOpen class="size-4" />
    Workspace
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    {#if isTauri()}
      Your workspace is stored locally on your device.
    {:else}
      Your workspace is stored in your browser's storage.
    {/if}
  </p>

  <div class="space-y-2 px-1">
    {#if workspacePath}
      <div class="flex items-start gap-3 p-3 rounded-lg border border-border">
        <FolderOpen class="size-5 text-muted-foreground mt-0.5 shrink-0" />
        <div class="flex-1 min-w-0">
          <div class="font-medium text-sm">Current Workspace</div>
          <p class="text-xs text-muted-foreground mt-0.5 break-all font-mono">
            {workspacePath}
          </p>
        </div>
      </div>
    {/if}

    {#if isTauri()}
      <Button
        variant="outline"
        size="sm"
        class="w-full"
        onclick={pickFolder}
        disabled={isChanging}
      >
        {#if isChanging}
          <RefreshCw class="size-4 mr-2 animate-spin" />
          Changing...
        {:else}
          <FolderOpen class="size-4 mr-2" />
          Change Workspace Folder
        {/if}
      </Button>
      <p class="text-xs text-muted-foreground">
        Choose a different folder for your workspace. The app will reload after changing.
      </p>
    {/if}

    {#if error}
      <p class="text-xs text-destructive">{error}</p>
    {/if}
  </div>
</div>
