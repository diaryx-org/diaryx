<script lang="ts">
  /**
   * FormatImportSettings - Import from Day One or Markdown Directory
   *
   * Day One: Parses Day One JSON/ZIP exports using WASM parsers,
   * then writes them into the workspace with proper hierarchy links.
   *
   * Markdown Directory: Writes raw files from a directory (FSAPI or ZIP)
   * into the workspace, then calls ImportDirectoryInPlace to add hierarchy metadata.
   */
  import { Button } from "$lib/components/ui/button";
  import FilePickerPopover from "$lib/components/FilePickerPopover.svelte";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Progress } from "$lib/components/ui/progress";
  import {
    FileDown,
    FolderOpen,
    FileArchive,
    Loader2,
    Check,
    AlertCircle,
  } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import { createApi } from "../backend/api";
  import { importFilesFromZip } from "./zipUtils";

  interface Props {
    workspacePath?: string | null;
  }

  // workspacePath reserved for future use (e.g. multi-workspace support)
  let { workspacePath: _workspacePath = null }: Props = $props();

  type ImportFormat = "dayone" | "markdown";

  const FORMAT_OPTIONS: Array<{
    value: ImportFormat;
    label: string;
    description: string;
  }> = [
    {
      value: "dayone",
      label: "Day One",
      description: "Import from a Day One export (ZIP with media or JSON)",
    },
    {
      value: "markdown",
      label: "Markdown Directory",
      description: "Import a directory of markdown files (folder or ZIP)",
    },
  ];

  type MdDestination = "root" | "subfolder";

  /** Directories to skip when reading from FSAPI. Matches Rust SKIP_DIRS. */
  const SKIP_DIRS = new Set([
    "node_modules", "target", ".git", ".svn", "dist", "build",
    "__pycache__", ".next", ".nuxt", "vendor", ".cargo", ".obsidian",
    ".trash", ".diaryx",
  ]);

  // Whether the File System Access API is available
  const hasFsApi = typeof window !== "undefined" && "showDirectoryPicker" in window;

  // ── Shared state ─────────────────────────────────────────────────────
  let selectedFormat: ImportFormat = $state("dayone");
  let isImporting: boolean = $state(false);
  let importStatusText: string | null = $state(null);
  let importProgressPercent = $state(0);
  let importResult: {
    success: boolean;
    imported: number;
    skipped: number;
    errors: string[];
    attachment_count: number;
  } | null = $state(null);
  let showConfirmDialog: boolean = $state(false);

  // ── Day One state ────────────────────────────────────────────────────
  let selectedParent: { path: string; name: string } | null = $state(null);
  let fileInputRef: HTMLInputElement | null = $state(null);
  let selectedFiles: File[] = $state([]);
  let parsedEntryCount = $state(0);
  let parseErrorCount = $state(0);
  let resolvedFolderName: string | null = $state(null);

  // ── Markdown Directory state ─────────────────────────────────────────
  let mdDestination: MdDestination = $state("subfolder");
  let mdSubfolderName: string = $state("imported");
  let mdSourceLabel: string = $state(""); // display name for selected source
  let mdDirectoryHandle: FileSystemDirectoryHandle | null = $state(null);
  let mdZipFile: File | null = $state(null);
  let zipInputRef: HTMLInputElement | null = $state(null);

  let currentFormatOption = $derived(
    FORMAT_OPTIONS.find((o) => o.value === selectedFormat)!,
  );

  let defaultFolderName = $derived(
    selectedFormat === "dayone" ? "journal" : "imported",
  );
  let folderName = $derived(resolvedFolderName ?? defaultFolderName);
  let displayPath = $derived.by(() => {
    const parent = selectedParent;
    const folder = resolvedFolderName ?? defaultFolderName;
    return parent ? `${parent.name}/${folder}` : folder;
  });

  let mdDisplayDestination = $derived(
    (mdDestination as string) === "root" ? "workspace root" : mdSubfolderName || "imported",
  );

  function handleFormatChange(value: string | undefined) {
    if (value) {
      selectedFormat = value as ImportFormat;
      resetState();
    }
  }

  function handleMdDestinationChange(value: string | undefined) {
    if (value) {
      mdDestination = value as MdDestination;
    }
  }

  function resetState() {
    selectedFiles = [];
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;
    parsedEntryCount = 0;
    parseErrorCount = 0;
    resolvedFolderName = null;
    showConfirmDialog = false;
    mdDirectoryHandle = null;
    mdZipFile = null;
    mdSourceLabel = "";
  }

  // ── Day One file picker ──────────────────────────────────────────────

  function triggerDayOneFileInput() {
    if (fileInputRef) {
      fileInputRef.accept = ".json,.zip";
      fileInputRef.multiple = false;
      fileInputRef.click();
    }
  }

  function handleDayOneFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    selectedFiles = Array.from(files);
    input.value = "";
    showConfirmDialog = true;
  }

  // ── Markdown Directory source pickers ────────────────────────────────

  async function handleSelectFolder() {
    try {
      const handle = await (window as any).showDirectoryPicker({ mode: "read" });
      mdDirectoryHandle = handle;
      mdZipFile = null;
      mdSourceLabel = handle.name;
      // Use directory name as default subfolder name
      if (mdDestination === "subfolder" && handle.name) {
        mdSubfolderName = handle.name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, "-")
          .replace(/^-|-$/g, "") || "imported";
      }
      showConfirmDialog = true;
    } catch (e) {
      // User cancelled the picker
      if ((e as Error).name !== "AbortError") {
        console.error("Directory picker failed:", e);
      }
    }
  }

  function triggerZipInput() {
    if (zipInputRef) {
      zipInputRef.click();
    }
  }

  function handleZipSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    mdZipFile = files[0];
    mdDirectoryHandle = null;
    mdSourceLabel = files[0].name;
    input.value = "";

    // Use ZIP filename (without extension) as default subfolder name
    const zipName = files[0].name.replace(/\.zip$/i, "");
    if (mdDestination === "subfolder" && zipName) {
      mdSubfolderName = zipName
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "") || "imported";
    }

    showConfirmDialog = true;
  }

  // ── Dialog reset ─────────────────────────────────────────────────────

  function resetDialog() {
    showConfirmDialog = false;
    selectedFiles = [];
    parsedEntryCount = 0;
    parseErrorCount = 0;
    if (!isImporting) {
      importResult = null;
      importStatusText = null;
      importProgressPercent = 0;
    }
  }

  // ── Day One import handler ───────────────────────────────────────────

  async function handleConfirmDayOneImport() {
    if (selectedFiles.length === 0) return;

    isImporting = true;
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;

    try {
      const backend = await getBackend();
      importStatusText = "Parsing files...";
      importProgressPercent = 10;

      type ImportedEntry = {
        title: string;
        date: string | null;
        body: string;
        metadata: Record<string, unknown>;
        attachments: Array<{
          filename: string;
          content_type: string;
          data: number[];
        }>;
      };

      let allEntries: ImportedEntry[] = [];
      let allErrors: string[] = [];

      const file = selectedFiles[0];
      const bytes = new Uint8Array(await file.arrayBuffer());
      const resultJson = await (backend as unknown as { parseDayOneJson(bytes: Uint8Array): Promise<string> }).parseDayOneJson(bytes);
      const parsed = JSON.parse(resultJson) as {
        entries: ImportedEntry[];
        errors: string[];
        journal_name: string | null;
      };
      allEntries = parsed.entries;
      allErrors = parsed.errors;
      if (parsed.journal_name) {
        resolvedFolderName = parsed.journal_name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, "-")
          .replace(/^-|-$/g, "") || null;
      }

      parsedEntryCount = allEntries.length;
      parseErrorCount = allErrors.length;

      if (allEntries.length === 0) {
        importResult = {
          success: false,
          imported: 0,
          skipped: allErrors.length,
          errors: allErrors,
          attachment_count: 0,
        };
        return;
      }

      importStatusText = `Writing ${allEntries.length} entries...`;
      importProgressPercent = 40;

      const entriesJson = JSON.stringify(allEntries);
      const responseJson = await backend.execute({
        type: "ImportEntries",
        params: {
          entries_json: entriesJson,
          folder: folderName,
          parent_path: selectedParent?.path ?? null,
        },
      });

      importProgressPercent = 90;

      const response =
        typeof responseJson === "string"
          ? JSON.parse(responseJson)
          : responseJson;

      if (response.type === "ImportResult") {
        const data = response.data as {
          imported: number;
          skipped: number;
          errors: string[];
          attachment_count: number;
        };
        importResult = {
          success: data.imported > 0,
          imported: data.imported,
          skipped: data.skipped + allErrors.length,
          errors: [...allErrors, ...data.errors],
          attachment_count: data.attachment_count,
        };
      } else {
        importResult = {
          success: true,
          imported: allEntries.length,
          skipped: allErrors.length,
          errors: allErrors,
          attachment_count: 0,
        };
      }

      importProgressPercent = 100;
      importStatusText = "Import complete";

      if (importResult.success) {
        window.dispatchEvent(
          new CustomEvent("import:complete", { detail: importResult }),
        );
      }
    } catch (e) {
      console.error("Day One import failed:", e);
      importResult = {
        success: false,
        imported: 0,
        skipped: selectedFiles.length,
        errors: [e instanceof Error ? e.message : String(e)],
        attachment_count: 0,
      };
      importStatusText = "Import failed";
      importProgressPercent = 0;
    } finally {
      isImporting = false;
    }
  }

  // ── Markdown Directory import handler ────────────────────────────────

  /**
   * Recursively read a directory handle and return all files as
   * { relativePath: string, file: File } entries.
   */
  async function readDirectoryRecursively(
    handle: FileSystemDirectoryHandle,
    prefix: string = "",
  ): Promise<Array<{ relativePath: string; file: File }>> {
    const results: Array<{ relativePath: string; file: File }> = [];

    for await (const [name, entry] of (handle as any).entries()) {
      // Skip hidden files/dirs
      if (name.startsWith(".")) continue;

      if (entry.kind === "directory") {
        if (SKIP_DIRS.has(name)) continue;
        const subResults = await readDirectoryRecursively(
          entry as FileSystemDirectoryHandle,
          prefix ? `${prefix}/${name}` : name,
        );
        results.push(...subResults);
      } else {
        const file = await (entry as FileSystemFileHandle).getFile();
        results.push({
          relativePath: prefix ? `${prefix}/${name}` : name,
          file,
        });
      }
    }

    return results;
  }

  async function handleConfirmMarkdownDirectoryImport() {
    if (!mdDirectoryHandle && !mdZipFile) return;

    isImporting = true;
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const destPrefix = mdDestination === "subfolder" ? (mdSubfolderName || "imported") : "";

      let filesWritten = 0;
      let filesSkipped = 0;
      const errors: string[] = [];

      if (mdDirectoryHandle) {
        // ── FSAPI directory import ─────────────────────────────────
        importStatusText = "Reading directory...";
        importProgressPercent = 5;

        const entries = await readDirectoryRecursively(mdDirectoryHandle);
        const total = entries.length;

        importStatusText = `Writing ${total} files...`;
        importProgressPercent = 10;

        for (let i = 0; i < entries.length; i++) {
          const { relativePath, file } = entries[i];
          const destPath = destPrefix
            ? `${destPrefix}/${relativePath}`
            : relativePath;

          try {
            if (file.name.endsWith(".md") || file.name.endsWith(".MD")) {
              const content = await file.text();
              await api.writeFile(destPath, content);
            } else {
              const buf = await file.arrayBuffer();
              await api.writeBinary(destPath, new Uint8Array(buf));
            }
            filesWritten++;
          } catch (e) {
            errors.push(`${relativePath}: ${e instanceof Error ? e.message : String(e)}`);
            filesSkipped++;
          }

          importProgressPercent = 10 + Math.round((i / total) * 60);
        }
      } else if (mdZipFile) {
        // ── ZIP import ─────────────────────────────────────────────
        importStatusText = "Extracting ZIP...";
        importProgressPercent = 5;

        const JSZip = (await import("jszip")).default;
        const zipData = await mdZipFile.arrayBuffer();
        const zip = await JSZip.loadAsync(zipData);

        importProgressPercent = 10;
        importStatusText = "Writing files...";

        const result = await importFilesFromZip(
          zip,
          destPrefix || ".",
          {
            writeText: (path, content) => api.writeFile(path, content),
            writeBinary: (path, data) => api.writeBinary(path, data),
          },
          (done, total) => {
            importProgressPercent = 10 + Math.round((done / total) * 60);
          },
        );

        filesWritten = result.files_imported;
        filesSkipped = result.files_skipped;
      }

      if (filesWritten === 0) {
        importResult = {
          success: false,
          imported: 0,
          skipped: filesSkipped,
          errors: errors.length > 0 ? errors : ["No files found to import"],
          attachment_count: 0,
        };
        return;
      }

      // ── Run ImportDirectoryInPlace to add hierarchy metadata ────
      importStatusText = "Building hierarchy...";
      importProgressPercent = 75;

      const responseJson = await backend.execute({
        type: "ImportDirectoryInPlace",
        params: {
          path: destPrefix || null,
        },
      });

      importProgressPercent = 95;

      const response =
        typeof responseJson === "string"
          ? JSON.parse(responseJson)
          : responseJson;

      if (response.type === "ImportResult") {
        const data = response.data as {
          imported: number;
          skipped: number;
          errors: string[];
          attachment_count: number;
        };
        importResult = {
          success: true,
          imported: filesWritten,
          skipped: filesSkipped + data.skipped,
          errors: [...errors, ...data.errors],
          attachment_count: data.attachment_count,
        };
      } else {
        importResult = {
          success: true,
          imported: filesWritten,
          skipped: filesSkipped,
          errors,
          attachment_count: 0,
        };
      }

      importProgressPercent = 100;
      importStatusText = "Import complete";

      if (importResult.success) {
        window.dispatchEvent(
          new CustomEvent("import:complete", { detail: importResult }),
        );
      }
    } catch (e) {
      console.error("Markdown directory import failed:", e);
      importResult = {
        success: false,
        imported: 0,
        skipped: 0,
        errors: [e instanceof Error ? e.message : String(e)],
        attachment_count: 0,
      };
      importStatusText = "Import failed";
      importProgressPercent = 0;
    } finally {
      isImporting = false;
    }
  }

  function handleConfirmImport() {
    if (selectedFormat === "dayone") {
      handleConfirmDayOneImport();
    } else {
      handleConfirmMarkdownDirectoryImport();
    }
  }
</script>

<input
  type="file"
  class="hidden"
  bind:this={fileInputRef}
  onchange={handleDayOneFileSelected}
/>
<input
  type="file"
  class="hidden"
  accept=".zip"
  bind:this={zipInputRef}
  onchange={handleZipSelected}
/>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <FileDown class="size-4" />
    Import from App
  </h3>
  <div class="px-1 space-y-3">
    <p class="text-xs text-muted-foreground">
      Import entries from Day One or a directory of markdown files into your workspace.
    </p>

    <!-- Format selector -->
    <div class="space-y-1.5">
      <Label for="import-format" class="text-xs text-muted-foreground">
        Format
      </Label>
      <Select.Root
        type="single"
        value={selectedFormat}
        onValueChange={handleFormatChange}
      >
        <Select.Trigger id="import-format" class="w-full">
          {currentFormatOption.label}
        </Select.Trigger>
        <Select.Content>
          {#each FORMAT_OPTIONS as option}
            <Select.Item value={option.value}>
              <div class="flex flex-col gap-0.5">
                <span>{option.label}</span>
                <span class="text-xs text-muted-foreground">
                  {option.description}
                </span>
              </div>
            </Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    {#if selectedFormat === "dayone"}
      <!-- ═══ Day One specific controls ═══ -->

      <!-- Parent entry picker -->
      <div class="space-y-1.5">
        <Label class="text-xs text-muted-foreground">
          Import under
        </Label>
        <FilePickerPopover
          onSelect={(file) => { selectedParent = file; }}
          placeholder="Search entries..."
        >
          <Button variant="outline" size="sm" class="w-full justify-start text-sm h-8 font-normal">
            {#if selectedParent}
              {selectedParent.name}
            {:else}
              Workspace root
            {/if}
          </Button>
        </FilePickerPopover>
        {#if selectedParent}
          <button
            class="text-xs text-muted-foreground hover:text-foreground underline"
            onclick={() => { selectedParent = null; }}
          >
            Reset to workspace root
          </button>
        {/if}
      </div>

      <!-- Select Day One file -->
      <Button
        variant="outline"
        size="sm"
        onclick={triggerDayOneFileInput}
        disabled={isImporting}
      >
        {#if isImporting}
          <Loader2 class="size-4 mr-2 animate-spin" />
          {importStatusText ?? "Importing..."}
        {:else}
          Select Day One File...
        {/if}
      </Button>

    {:else}
      <!-- ═══ Markdown Directory specific controls ═══ -->

      <!-- Destination -->
      <div class="space-y-1.5">
        <Label class="text-xs text-muted-foreground">
          Destination
        </Label>
        <Select.Root
          type="single"
          value={mdDestination}
          onValueChange={handleMdDestinationChange}
        >
          <Select.Trigger class="w-full">
            {mdDestination === "root" ? "Workspace root" : "Subfolder"}
          </Select.Trigger>
          <Select.Content>
            <Select.Item value="subfolder">
              <div class="flex flex-col gap-0.5">
                <span>Subfolder</span>
                <span class="text-xs text-muted-foreground">
                  Import into a new subfolder in the workspace
                </span>
              </div>
            </Select.Item>
            <Select.Item value="root">
              <div class="flex flex-col gap-0.5">
                <span>Workspace root</span>
                <span class="text-xs text-muted-foreground">
                  Import directly into the workspace root
                </span>
              </div>
            </Select.Item>
          </Select.Content>
        </Select.Root>
      </div>

      {#if mdDestination === "subfolder"}
        <div class="space-y-1.5">
          <Label class="text-xs text-muted-foreground">
            Folder name
          </Label>
          <Input
            bind:value={mdSubfolderName}
            placeholder="imported"
            class="h-8 text-sm"
          />
        </div>
      {/if}

      <!-- Source picker buttons -->
      <div class="flex gap-2">
        {#if hasFsApi}
          <Button
            variant="outline"
            size="sm"
            onclick={handleSelectFolder}
            disabled={isImporting}
          >
            <FolderOpen class="size-4 mr-2" />
            Select Folder...
          </Button>
        {/if}
        <Button
          variant="outline"
          size="sm"
          onclick={triggerZipInput}
          disabled={isImporting}
        >
          <FileArchive class="size-4 mr-2" />
          Select ZIP...
        </Button>
      </div>

      {#if mdSourceLabel && !showConfirmDialog}
        <p class="text-xs text-muted-foreground">
          Selected: {mdSourceLabel}
        </p>
      {/if}
    {/if}

    <!-- Progress bar during import -->
    {#if isImporting}
      <div class="space-y-1">
        <div
          class="flex items-center justify-between text-xs text-muted-foreground"
        >
          <span>{importStatusText ?? "Importing..."}</span>
          <span>{importProgressPercent}%</span>
        </div>
        <Progress value={importProgressPercent} class="h-2" />
      </div>
    {/if}

    <!-- Import result -->
    {#if importResult && !showConfirmDialog}
      {#if importResult.success}
        <div
          class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
        >
          <Check class="size-4" />
          <span
            >Imported {importResult.imported} {selectedFormat === "dayone" ? "entries" : "files"}{importResult.attachment_count >
            0
              ? ` with ${importResult.attachment_count} attachments`
              : ""}.</span
          >
        </div>
      {:else}
        <div
          class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
        >
          <AlertCircle class="size-4" />
          <span
            >{importResult.errors[0] || "Import failed"}</span
          >
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
        <FileDown class="size-5" />
        Import {currentFormatOption.label}
      </Dialog.Title>
      <Dialog.Description>
        {#if selectedFormat === "dayone"}
          {#if selectedFiles.length === 1}
            Import from <span class="font-medium">{selectedFiles[0].name}</span> into
            <span class="font-mono text-xs">{displayPath}/</span>.
          {:else}
            Import {selectedFiles.length} files into
            <span class="font-mono text-xs">{displayPath}/</span>.
          {/if}
        {:else}
          Import from <span class="font-medium">{mdSourceLabel}</span> into
          <span class="font-mono text-xs">{mdDisplayDestination}/</span>.
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    {#if isImporting}
      <div class="space-y-3 py-2">
        <div class="space-y-1">
          <div
            class="flex items-center justify-between text-xs text-muted-foreground"
          >
            <span>{importStatusText ?? "Importing..."}</span>
            <span>{importProgressPercent}%</span>
          </div>
          <Progress value={importProgressPercent} class="h-2" />
        </div>
        {#if parsedEntryCount > 0}
          <p class="text-xs text-muted-foreground">
            Parsed {parsedEntryCount} entries{parseErrorCount > 0 ? `, ${parseErrorCount} errors` : ""}.
          </p>
        {/if}
      </div>
    {:else if importResult}
      <div class="space-y-3 py-2">
        {#if importResult.success}
          <div
            class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
          >
            <Check class="size-4" />
            <span>Imported {importResult.imported} {selectedFormat === "dayone" ? "entries" : "files"}.</span>
          </div>
          {#if importResult.skipped > 0}
            <p class="text-xs text-muted-foreground">
              {importResult.skipped} {selectedFormat === "dayone" ? "entries" : "files"} skipped.
            </p>
          {/if}
        {:else}
          <div
            class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
          >
            <AlertCircle class="size-4" />
            <span>{importResult.errors[0] || "Import failed"}</span>
          </div>
        {/if}
      </div>
    {:else}
      <div class="space-y-2 py-2">
        <p class="text-sm text-muted-foreground">
          {#if selectedFormat === "dayone"}
            Entries will be parsed from Day One's JSON format and organized
            by date into <span class="font-mono text-xs">{displayPath}/</span>.
          {:else}
            Files will be written to <span class="font-mono text-xs">{mdDisplayDestination}/</span>
            and hierarchy metadata (contents/part_of) will be added automatically.
          {/if}
        </p>
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
        <Button onclick={resetDialog}>Done</Button>
      {:else}
        <Button variant="outline" onclick={resetDialog}>Cancel</Button>
        <Button onclick={handleConfirmImport}>Import</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
