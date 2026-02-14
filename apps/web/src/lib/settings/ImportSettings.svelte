<script lang="ts">
  /**
   * ImportSettings - Import from zip settings section
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Upload, Loader2, Check, AlertCircle, AlertTriangle } from "@lucide/svelte";
  import { getBackend } from "../backend";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  // Import state
  let isImporting: boolean = $state(false);
  let importResult: {
    success: boolean;
    files_imported: number;
    error?: string;
  } | null = $state(null);

  // Confirmation dialog state
  let showConfirmDialog: boolean = $state(false);
  let deleteExisting: boolean = $state(false);
  let selectedFile: File | null = $state(null);

  // Reference to hidden file input
  let fileInputRef: HTMLInputElement | null = $state(null);

  function triggerFileInput() {
    fileInputRef?.click();
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

    showConfirmDialog = false;
    isImporting = true;
    importResult = null;

    try {
      const backend = await getBackend();
      const workspaceDir = workspacePath
        ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
        : undefined;

      // Delete existing files if requested
      if (deleteExisting && workspaceDir) {
        try {
          await backend.execute({ type: 'ClearDirectory', params: { path: workspaceDir } });
        } catch (e) {
          console.warn("[Import] Failed to clear existing files:", e);
        }
      }

      const result = await backend.importFromZip(
        selectedFile,
        workspaceDir,
        (uploaded, total) => {
          if (uploaded % (10 * 1024 * 1024) < 1024 * 1024) {
            console.log(
              `[Import] Progress: ${(uploaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`,
            );
          }
        },
      );

      importResult = result;

      if (result.success) {
        window.dispatchEvent(
          new CustomEvent("import:complete", { detail: result }),
        );
      }
    } catch (e) {
      console.error("Import failed:", e);
      importResult = {
        success: false,
        files_imported: 0,
        error: e instanceof Error ? e.message : String(e),
      };
    } finally {
      isImporting = false;
      selectedFile = null;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Upload class="size-4" />
    Import
  </h3>
  <div class="px-1 space-y-2">
    <p class="text-xs text-muted-foreground">
      Import entries from a zip backup.
    </p>
    <input
      type="file"
      accept=".zip"
      class="hidden"
      bind:this={fileInputRef}
      onchange={handleFileSelected}
    />

    <Button
      variant="outline"
      size="sm"
      onclick={triggerFileInput}
      disabled={isImporting}
    >
      {#if isImporting}
        <Loader2 class="size-4 mr-2 animate-spin" />
        Importing...
      {:else}
        Select Zip File...
      {/if}
    </Button>

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
        {/if}
      </Dialog.Description>
    </Dialog.Header>

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

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button
        variant="outline"
        onclick={() => { showConfirmDialog = false; selectedFile = null; }}
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
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
