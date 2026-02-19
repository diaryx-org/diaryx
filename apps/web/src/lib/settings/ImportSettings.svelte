<script lang="ts">
  /**
   * ImportSettings - Import from zip settings section
   *
   * Extracted from SettingsDialog for modularity.
   * When sync is enabled, imports are routed through the server snapshot
   * endpoint to avoid OOM from per-file CRDT updates over WebSocket.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Progress } from "$lib/components/ui/progress";
  import type { Backend } from "$lib/backend/interface";
  import { Upload, Loader2, Check, AlertCircle, AlertTriangle } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import {
    isSyncEnabled,
    isAuthenticated,
    uploadWorkspaceSnapshot,
    getCurrentWorkspace,
    getServerUrl,
  } from "$lib/auth";
  import {
    getCurrentWorkspaceId,
    getLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import {
    disconnectWorkspace,
    waitForInitialSync,
    setWorkspaceServer,
    setWorkspaceId,
    markAllCrdtFilesAsDeleted,
  } from "$lib/crdt/workspaceCrdtBridge";

  interface Props {
    workspacePath?: string | null;
    launcherOnly?: boolean;
  }

  let { workspacePath = null, launcherOnly = false }: Props = $props();

  // Import state
  let isImporting: boolean = $state(false);
  let importStatusText: string | null = $state(null);
  let importResult: {
    success: boolean;
    files_imported: number;
    error?: string;
  } | null = $state(null);
  let importProgressPercent = $state(0);

  // Confirmation dialog state
  let showConfirmDialog: boolean = $state(false);
  let deleteExisting: boolean = $state(false);
  let selectedFile: File | null = $state(null);

  // Reference to hidden file input
  let fileInputRef: HTMLInputElement | null = $state(null);

  // Derived: sync import path is only active for authenticated, sync-enabled,
  // server-backed workspaces. Local-only workspaces should import locally.
  let syncActive = $derived.by(() => {
    if (!isSyncEnabled() || !isAuthenticated()) {
      return false;
    }

    const currentWorkspaceId = getCurrentWorkspaceId();
    if (!currentWorkspaceId) {
      // Be conservative: unknown workspace selection should never force server import.
      return false;
    }

    const localWorkspace = getLocalWorkspace(currentWorkspaceId);
    if (localWorkspace?.isLocal) {
      return false;
    }

    const serverWorkspace = getCurrentWorkspace();
    if (!serverWorkspace) {
      return false;
    }

    return serverWorkspace.id === currentWorkspaceId;
  });

  function triggerFileInput() {
    fileInputRef?.click();
  }

  // Allow parent components (e.g. empty-workspace CTA) to open the picker directly.
  export function openImportFilePicker() {
    triggerFileInput();
  }

  function resetImportDialogState(clearResult: boolean = launcherOnly) {
    showConfirmDialog = false;
    selectedFile = null;
    deleteExisting = false;
    if (clearResult) {
      importResult = null;
    }
    importStatusText = null;
    importProgressPercent = 0;
  }

  // If dialog closes via overlay/escape, clear transient dialog state.
  $effect(() => {
    if (!showConfirmDialog && !isImporting) {
      selectedFile = null;
      deleteExisting = false;
      importStatusText = null;
      importProgressPercent = 0;
      if (launcherOnly) {
        importResult = null;
      }
    }
  });

  function setImportProgress(percent: number, status?: string) {
    importProgressPercent = Math.max(0, Math.min(100, Math.round(percent)));
    if (status !== undefined) {
      importStatusText = status;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    const value = bytes / Math.pow(1024, index);
    return `${value.toFixed(value < 10 && index > 0 ? 1 : 0)} ${units[index]}`;
  }

  /**
   * Resolve a workspace directory from either an index file path or directory.
   * Returns "." for current workspace root when path is empty/relative root.
   */
  function toWorkspaceDir(path: string | null | undefined): string | null {
    if (!path) return null;

    const trimmed = path.trim();
    if (!trimmed) return null;

    const normalized = trimmed.replace(/[\\/]+$/, "");
    if (!normalized || normalized === ".") return ".";

    if (normalized.endsWith(".md")) {
      const lastSlash = Math.max(normalized.lastIndexOf("/"), normalized.lastIndexOf("\\"));
      return lastSlash >= 0 ? normalized.substring(0, lastSlash) || "." : ".";
    }

    return normalized;
  }

  async function discoverWorkspaceDir(
    backend: Backend & { getDefaultWorkspacePath?: () => Promise<string> },
  ): Promise<string | null> {
    if (backend.getDefaultWorkspacePath) {
      try {
        const discovered = toWorkspaceDir(await backend.getDefaultWorkspacePath());
        if (discovered && discovered !== ".") {
          return discovered;
        }
      } catch {
        // Fall through to command-based discovery.
      }
    }

    try {
      const response = await backend.execute({
        type: "FindRootIndex",
        params: { directory: "." },
      });
      if (
        response &&
        typeof response === "object" &&
        (response as { type?: string }).type === "String"
      ) {
        const rootPath = (response as { data?: unknown }).data;
        if (typeof rootPath !== "string") {
          return null;
        }
        const rootDir = toWorkspaceDir(rootPath);
        if (rootDir && rootDir !== ".") {
          return rootDir;
        }
      }
    } catch {
      // Ignore discovery errors and fall back to backend defaults.
    }

    return null;
  }

  /**
   * Prefer explicit workspace path when it points to a concrete directory.
   * Fall back to backend discovery. Returns null when unknown.
   */
  async function resolveImportWorkspaceDir(
    backend: Backend & { getDefaultWorkspacePath?: () => Promise<string> },
  ): Promise<string | null> {
    const fromProp = toWorkspaceDir(workspacePath);
    const fromBackend = toWorkspaceDir(backend.getWorkspacePath());

    if (fromProp && fromProp !== ".") return fromProp;
    if (fromBackend) return fromBackend;
    return await discoverWorkspaceDir(backend);
  }

  function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    // Store file and show confirmation dialog
    selectedFile = file;
    deleteExisting = false;
    showConfirmDialog = true;

    // Reset the input so the same file can be re-selected
    input.value = "";
  }

  async function handleConfirmImport() {
    if (!selectedFile) return;

    isImporting = true;
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;
    setImportProgress(2, "Preparing import...");

    try {
      if (syncActive) {
        await importViaServer(selectedFile);
      } else {
        await importLocally(selectedFile);
      }
    } catch (e) {
      console.error("Import failed:", e);
      setImportProgress(0, "Import failed");
      importResult = {
        success: false,
        files_imported: 0,
        error: e instanceof Error ? e.message : String(e),
      };
    } finally {
      isImporting = false;
    }
  }

  /**
   * Sync-enabled path: upload ZIP to server, then resync CRDT state.
   */
  async function importViaServer(file: File) {
    const workspace = getCurrentWorkspace();
    if (!workspace) throw new Error("No active workspace — cannot upload to server");

    const mode = deleteExisting ? 'replace' : 'merge';
    setImportProgress(8, "Preparing sync import...");

    // Step 1: Tombstone local CRDT entries if replacing
    if (deleteExisting) {
      setImportProgress(14, "Clearing existing files...");
      const tombstoned = await markAllCrdtFilesAsDeleted();
      console.log(`[Import] Tombstoned ${tombstoned} local CRDT entries`);
    }

    // Step 2: Upload ZIP to server
    setImportProgress(30, "Uploading to sync server...");
    const result = await uploadWorkspaceSnapshot(
      workspace.id,
      file,
      mode,
      true,
      (uploadedBytes, totalBytes) => {
        const ratio = totalBytes > 0 ? uploadedBytes / totalBytes : 0;
        const percent = 30 + ratio * 32;
        setImportProgress(
          percent,
          totalBytes > 0
            ? `Uploading to sync server (${formatBytes(uploadedBytes)} / ${formatBytes(totalBytes)})...`
            : "Uploading to sync server...",
        );
      },
    );
    if (!result) throw new Error("Upload failed — server returned no result");

    console.log(`[Import] Server imported ${result.files_imported} files`);
    setImportProgress(62, "Snapshot uploaded");

    // Step 3: Disconnect and reconnect to pull fresh CRDT state
    setImportProgress(72, "Reconnecting sync...");
    const serverUrl = getServerUrl();
    disconnectWorkspace();

    if (deleteExisting) {
      setImportProgress(80, "Applying imported snapshot locally...");
      try {
        const backend = await getBackend();
        const workspaceDir = await resolveImportWorkspaceDir(backend);
        if (workspaceDir) {
          await backend.execute({ type: 'ClearDirectory', params: { path: workspaceDir } });
        } else {
          console.warn("[Import] Could not resolve workspace root for local clear; skipping clear step");
        }
      } catch (e) {
        console.warn("[Import] Failed to clear local workspace before resync:", e);
      }
    }

    await setWorkspaceId(workspace.id);
    if (serverUrl) {
      await setWorkspaceServer(serverUrl);
    }

    // Step 4: Wait for files to arrive
    setImportProgress(90, "Syncing files from server...");
    const synced = await waitForInitialSync(30000);
    if (!synced) {
      console.warn("[Import] Sync timed out, continuing in background");
    }
    setImportProgress(100, "Import complete");

    importResult = {
      success: true,
      files_imported: result.files_imported,
    };

    window.dispatchEvent(
      new CustomEvent("import:complete", { detail: importResult }),
    );
  }

  /**
   * Local-only path: extract ZIP and write files directly.
   */
  async function importLocally(file: File) {
    const backend = await getBackend();
    const currentWorkspaceId = getCurrentWorkspaceId();
    const localWorkspace = currentWorkspaceId
      ? getLocalWorkspace(currentWorkspaceId)
      : null;
    if (localWorkspace?.isLocal) {
      // Defensive: ensure no stale sync transport remains connected for local-only workspaces.
      disconnectWorkspace();
      await setWorkspaceId(null);
    }
    const workspaceDir = await resolveImportWorkspaceDir(backend);
    setImportProgress(8, "Preparing local import...");

    // Delete existing files if requested
    if (deleteExisting && workspaceDir) {
      setImportProgress(14, "Clearing existing files...");
      try {
        await backend.execute({ type: 'ClearDirectory', params: { path: workspaceDir } });
      } catch (e) {
        console.warn("[Import] Failed to clear existing files:", e);
      }
    }
    setImportProgress(22, "Importing files...");

    const result = await backend.importFromZip(
      file,
      workspaceDir ?? undefined,
      (uploaded, total) => {
        const ratio = total > 0 ? uploaded / total : 0;
        const percent = 22 + ratio * 72;
        setImportProgress(percent, "Importing files...");
        if (uploaded % (10 * 1024 * 1024) < 1024 * 1024) {
          console.log(
            `[Import] Progress: ${(uploaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`,
          );
        }
      },
    );

    importResult = result;

    if (result.success) {
      setImportProgress(100, "Import complete");
      window.dispatchEvent(
        new CustomEvent("import:complete", { detail: result }),
      );
    }
  }
</script>

<input
  type="file"
  accept=".zip"
  class="hidden"
  bind:this={fileInputRef}
  onchange={handleFileSelected}
/>

{#if !launcherOnly}
  <div class="space-y-3">
    <h3 class="font-medium flex items-center gap-2">
      <Upload class="size-4" />
      Import
    </h3>
    <div class="px-1 space-y-2">
      <p class="text-xs text-muted-foreground">
        Import entries from a zip backup.
      </p>

      <Button
        variant="outline"
        size="sm"
        onclick={triggerFileInput}
        disabled={isImporting}
      >
        {#if isImporting}
          <Loader2 class="size-4 mr-2 animate-spin" />
          {importStatusText ?? "Importing..."}
        {:else}
          Select Zip File...
        {/if}
      </Button>

      {#if isImporting}
        <div class="space-y-1">
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{importStatusText ?? "Importing..."}</span>
            <span>{importProgressPercent}%</span>
          </div>
          <Progress value={importProgressPercent} class="h-2" />
        </div>
      {/if}

      {#if importResult}
        {#if importResult.success}
          <div
            class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
          >
            <Check class="size-4" />
            <span>Imported {importResult.files_imported} files.</span>
          </div>
        {:else}
          <div
            class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
          >
            <AlertCircle class="size-4" />
            <span>{importResult.error || "Import failed"}</span>
          </div>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<!-- Import Confirmation Dialog -->
<Dialog.Root bind:open={showConfirmDialog}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Upload class="size-5" />
        Import from Zip
      </Dialog.Title>
      <Dialog.Description>
        {#if selectedFile}
          Import files from <span class="font-medium">{selectedFile.name}</span> into your workspace.
          {#if syncActive}
            The ZIP will be uploaded to the sync server and files will be synced to your device.
          {/if}
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    {#if isImporting}
      <div class="space-y-3 py-2">
        <div class="space-y-1">
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{importStatusText ?? "Importing..."}</span>
            <span>{importProgressPercent}%</span>
          </div>
          <Progress value={importProgressPercent} class="h-2" />
        </div>
      </div>
    {:else if importResult}
      <div class="space-y-3 py-2">
        {#if importResult.success}
          <div
            class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
          >
            <Check class="size-4" />
            <span>Imported {importResult.files_imported} files.</span>
          </div>
        {:else}
          <div
            class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
          >
            <AlertCircle class="size-4" />
            <span>{importResult.error || "Import failed"}</span>
          </div>
        {/if}
      </div>
    {:else}
      <div class="space-y-3 py-2">
        <label class="flex items-start gap-3 cursor-pointer">
          <Checkbox bind:checked={deleteExisting} class="mt-0.5" />
          <div>
            <span class="text-sm font-medium">Delete existing files first</span>
            <p class="text-xs text-muted-foreground mt-0.5">
              Removes all current workspace files before importing. Use this to fully replace your workspace with the zip contents.
            </p>
          </div>
        </label>

        {#if deleteExisting}
          <div class="flex items-start gap-2 text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-950/20 p-2 rounded">
            <AlertTriangle class="size-4 mt-0.5 shrink-0" />
            <span>All existing files in your workspace will be permanently deleted before importing.</span>
          </div>
        {/if}
      </div>
    {/if}

    <Dialog.Footer class="gap-2 sm:gap-0">
      {#if isImporting}
        <Button variant="outline" disabled>
          <Loader2 class="size-4 mr-2 animate-spin" />
          Importing...
        </Button>
      {:else if importResult}
        {#if !importResult.success}
          <Button variant="outline" onclick={handleConfirmImport}>
            Retry
          </Button>
        {/if}
        <Button onclick={() => resetImportDialogState()}>
          Done
        </Button>
      {:else}
        <Button
          variant="outline"
          onclick={() => resetImportDialogState()}
        >
          Cancel
        </Button>
        <Button onclick={handleConfirmImport}>
          {#if deleteExisting}
            Replace & Import
          {:else}
            Import
          {/if}
        </Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
