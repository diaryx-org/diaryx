<script lang="ts">
  /**
   * FormatImportSettings - Import from Day One or Markdown sources
   *
   * Parses external formats into Diaryx entries using WASM parsers,
   * then writes them into the workspace with proper hierarchy links.
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Progress } from "$lib/components/ui/progress";
  import {
    FileDown,
    Loader2,
    Check,
    AlertCircle,
  } from "@lucide/svelte";
  import { getBackend } from "../backend";

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
    accept: string;
  }> = [
    {
      value: "dayone",
      label: "Day One",
      description: "Import from a Day One Journal.json export file",
      accept: ".json",
    },
    {
      value: "markdown",
      label: "Markdown Files",
      description: "Import markdown files (select one or more .md files)",
      accept: ".md",
    },
  ];

  // State
  let selectedFormat: ImportFormat = $state("dayone");
  let folderName: string = $state("journal");
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

  // File picker
  let fileInputRef: HTMLInputElement | null = $state(null);
  let selectedFiles: File[] = $state([]);
  let showConfirmDialog: boolean = $state(false);

  // Preview counts displayed during import
  let parsedEntryCount = $state(0);
  let parseErrorCount = $state(0);

  let currentFormatOption = $derived(
    FORMAT_OPTIONS.find((o) => o.value === selectedFormat)!,
  );

  function handleFormatChange(value: string | undefined) {
    if (value) {
      selectedFormat = value as ImportFormat;
      folderName = value === "dayone" ? "journal" : "imported";
      resetState();
    }
  }

  function resetState() {
    selectedFiles = [];
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;
    parsedEntryCount = 0;
    parseErrorCount = 0;
    showConfirmDialog = false;
  }

  function triggerFileInput() {
    if (fileInputRef) {
      fileInputRef.accept = currentFormatOption.accept;
      fileInputRef.multiple = selectedFormat === "markdown";
      fileInputRef.click();
    }
  }

  function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    selectedFiles = Array.from(files);
    input.value = "";
    showConfirmDialog = true;
  }

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

  async function handleConfirmImport() {
    if (selectedFiles.length === 0) return;

    isImporting = true;
    importResult = null;
    importStatusText = null;
    importProgressPercent = 0;

    try {
      const backend = await getBackend();
      importStatusText = "Parsing files...";
      importProgressPercent = 10;

      // Step 1: Parse files into ImportedEntry[] using WASM parsers
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

      if (selectedFormat === "dayone") {
        const file = selectedFiles[0];
        const bytes = new Uint8Array(await file.arrayBuffer());
        const resultJson = (backend as unknown as { parseDayOneJson(bytes: Uint8Array): string }).parseDayOneJson(bytes);
        const parsed = JSON.parse(resultJson) as {
          entries: ImportedEntry[];
          errors: string[];
        };
        allEntries = parsed.entries;
        allErrors = parsed.errors;
      } else {
        // Markdown: parse each file individually
        for (const file of selectedFiles) {
          try {
            const bytes = new Uint8Array(await file.arrayBuffer());
            const entryJson = (backend as unknown as { parseMarkdownFile(bytes: Uint8Array, filename: string): string }).parseMarkdownFile(bytes, file.name);
            const entry = JSON.parse(entryJson) as ImportedEntry;
            allEntries.push(entry);
          } catch (e) {
            allErrors.push(
              `${file.name}: ${e instanceof Error ? e.message : String(e)}`,
            );
          }
        }
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

      // Step 2: Send entries to backend for writing
      importStatusText = `Writing ${allEntries.length} entries...`;
      importProgressPercent = 40;

      const entriesJson = JSON.stringify(allEntries);
      const responseJson = await backend.execute({
        type: "ImportEntries",
        params: {
          entries_json: entriesJson,
          folder: folderName,
        },
      });

      importProgressPercent = 90;

      // Parse the response
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
      console.error("Format import failed:", e);
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
</script>

<input
  type="file"
  class="hidden"
  bind:this={fileInputRef}
  onchange={handleFileSelected}
/>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <FileDown class="size-4" />
    Import from App
  </h3>
  <div class="px-1 space-y-3">
    <p class="text-xs text-muted-foreground">
      Import entries from Day One or markdown files into your workspace.
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

    <!-- Folder name -->
    <div class="space-y-1.5">
      <Label for="import-folder" class="text-xs text-muted-foreground">
        Import into folder
      </Label>
      <Input
        id="import-folder"
        bind:value={folderName}
        placeholder="e.g. journal, imported"
        class="h-8 text-sm"
      />
    </div>

    <!-- Select files button -->
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
        Select {currentFormatOption.label} File{selectedFormat === "markdown"
          ? "s"
          : ""}...
      {/if}
    </Button>

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
            >Imported {importResult.imported} entries{importResult.attachment_count >
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
        {#if selectedFiles.length === 1}
          Import from <span class="font-medium">{selectedFiles[0].name}</span> into
          <span class="font-mono text-xs">{folderName}/</span>.
        {:else}
          Import {selectedFiles.length} files into
          <span class="font-mono text-xs">{folderName}/</span>.
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
            <span>Imported {importResult.imported} entries.</span>
          </div>
          {#if importResult.skipped > 0}
            <p class="text-xs text-muted-foreground">
              {importResult.skipped} entries skipped.
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
            by date into <span class="font-mono text-xs">{folderName}/</span>.
          {:else}
            {selectedFiles.length} markdown file{selectedFiles.length > 1
              ? "s"
              : ""} will be imported into
            <span class="font-mono text-xs">{folderName}/</span>.
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
