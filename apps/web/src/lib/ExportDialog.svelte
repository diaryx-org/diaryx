<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import NativeSelect from "$lib/components/ui/native-select/native-select.svelte";
  import type { ExportPlan, ExportedFile } from "./backend";
  import type { Api } from "./backend/api";
  import { toast } from "svelte-sonner";
  import {
    Download,
    FileText,
    FolderOpen,
    ChevronRight,
    ChevronDown,
    Loader2,
    Image,
    Paperclip,
  } from "@lucide/svelte";
  import { EXPORT_FORMATS, PandocService, TypstService, type ExportFormat } from "$lib/export";

  interface Props {
    open: boolean;
    rootPath: string;
    api: Api | null;
    onOpenChange: (open: boolean) => void;
  }

  let {
    open = $bindable(),
    rootPath,
    api,
    onOpenChange,
  }: Props = $props();

  let audiences: string[] = $state([]);
  let selectedAudience = $state("all");
  let exportPlan = $state<ExportPlan | null>(null);
  let binaryFiles = $state<{ path: string }[]>([]);
  let isLoading = $state(false);
  let isExporting = $state(false);
  let error: string | null = $state(null);
  let expandedNodes = $state(new Set<string>());
  let selectedFormat = $state<ExportFormat>('markdown');
  let pandocProgress = $state('');

  // Lazy-created services (only instantiated when needed)
  let pandocService: PandocService | null = null;
  let typstService: TypstService | null = null;

  // Load audiences when dialog opens
  $effect(() => {
    if (open && api && rootPath) {
      loadAudiences();
    }
  });

  // Update plan when audience changes
  $effect(() => {
    if (open && api && rootPath && selectedAudience) {
      loadExportPlan();
    }
  });

  async function loadAudiences() {
    if (!api) return;
    try {
      audiences = await api.getAvailableAudiences(rootPath);
    } catch (e) {
      console.error("Failed to load audiences:", e);
      audiences = [];
    }
  }

  // Helper to convert Map to plain object (WASM may return Maps instead of objects)
  function normalizeToObject(value: any): any {
    if (value instanceof Map) {
      const obj: Record<string, any> = {};
      for (const [k, v] of value.entries()) {
        obj[k] = normalizeToObject(v);
      }
      return obj;
    }
    if (Array.isArray(value)) {
      return value.map(normalizeToObject);
    }
    return value;
  }

  async function loadExportPlan() {
    if (!api) return;
    isLoading = true;
    error = null;
    binaryFiles = [];
    try {
      // For "all" audience, we'll pass a special value
      // The api treats empty audience differently - for now use "all" which won't match any audience
      // This means no files are included. We need a different approach for "export all"
      const audience = selectedAudience === "all" ? "*" : selectedAudience;
      console.log("[ExportDialog] planExport called with rootPath:", rootPath, "audience:", audience);
      let rawPlan;
      if (selectedAudience === "all") {
        // For "all" export, we'll skip audience filtering by using a special marker
        rawPlan = await api.planExport(rootPath, "*");
      } else {
        rawPlan = await api.planExport(rootPath, selectedAudience);
      }
      // Normalize Map to plain object (WASM returns Maps)
      exportPlan = normalizeToObject(rawPlan);
      console.log("[ExportDialog] planExport returned:", exportPlan);

      // Also fetch binary attachments for preview (just paths, no data)
      const rawAttachments = await api.exportBinaryAttachments(rootPath, audience);
      const attachments = normalizeToObject(rawAttachments) ?? [];
      binaryFiles = attachments.map((f: any) => ({ path: f.relative_path }));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      exportPlan = null;
      binaryFiles = [];
    } finally {
      isLoading = false;
    }
  }

  async function handleExport() {
    if (!api || !exportPlan || !exportPlan.included || exportPlan.included.length === 0) return;

    isExporting = true;
    error = null;
    pandocProgress = '';
    try {
      const audience = selectedAudience === "all" ? "*" : selectedAudience;
      const formatInfo = EXPORT_FORMATS.find(f => f.id === selectedFormat)!;

      if (formatInfo.requiresPandoc) {
        // Pandoc format — get markdown files, then convert via WASM worker
        const rawFiles = await api.exportToMemory(rootPath, audience);
        const files = normalizeToObject(rawFiles) ?? [];
        const rawBinaryFiles = await api.exportBinaryAttachments(rootPath, audience);
        const binaries = normalizeToObject(rawBinaryFiles) ?? [];

        pandocProgress = 'Loading pandoc (first time may take a moment)...';
        if (!pandocService) pandocService = new PandocService();
        await pandocService.ensureReady();

        // For PDF, also load the Typst compiler (pandoc produces typst source, typst compiles to PDF)
        const isPdf = selectedFormat === 'pdf';
        if (isPdf) {
          pandocProgress = 'Loading typst compiler (first time may take a moment)...';
          if (!typstService) typstService = new TypstService();
          await typstService.ensureReady();
        }

        // Build resource map for embedded images
        const resources: Record<string, Uint8Array> = {};
        for (const info of binaries) {
          try {
            const data = await api.readBinary(info.source_path);
            resources[info.relative_path] = data;
          } catch (e) {
            console.warn(`[Export] Failed to read binary ${info.source_path}:`, e);
          }
        }

        // Convert each file
        const convertedFiles: { path: string; data: Uint8Array | string }[] = [];
        for (let i = 0; i < files.length; i++) {
          pandocProgress = `Converting ${i + 1}/${files.length}: ${files[i].path}`;
          const result = await pandocService.convert(files[i].content, selectedFormat, resources);
          const newPath = files[i].path.replace(/\.md$/, formatInfo.extension);

          if (isPdf) {
            // PDF: pandoc returns typst source in stdout, compile to PDF with typst
            const typstSource = result.stdout ?? '';
            pandocProgress = `Generating PDF ${i + 1}/${files.length}: ${files[i].path}`;
            const pdfBytes = await typstService!.compile(typstSource);
            convertedFiles.push({ path: newPath, data: pdfBytes });
          } else if (formatInfo.binary) {
            // Binary output (docx, epub, odt)
            const outputKey = result.outputFilename ?? Object.keys(result.files ?? {})[0];
            const outputFile = outputKey ? result.files?.[outputKey] : null;
            if (outputFile) {
              convertedFiles.push({ path: newPath, data: outputFile });
            } else {
              console.warn(`[Export] No binary output for ${files[i].path}`, result);
            }
          } else {
            // Text output (latex, rst)
            convertedFiles.push({ path: newPath, data: result.stdout ?? '' });
          }
        }

        pandocProgress = '';
        await downloadConvertedAsZip(convertedFiles, formatInfo.binary ? [] : binaries);
      } else if (selectedFormat === 'html') {
        // Use existing HTML pipeline
        const rawFiles = await api.exportToHtml(rootPath, audience);
        const files = normalizeToObject(rawFiles) ?? [];
        const rawBinaryFiles = await api.exportBinaryAttachments(rootPath, audience);
        const binaries = normalizeToObject(rawBinaryFiles) ?? [];
        await downloadAsZip(files, binaries);
      } else {
        // Markdown — existing pipeline
        const rawFiles = await api.exportToMemory(rootPath, audience);
        const files = normalizeToObject(rawFiles) ?? [];
        const rawBinaryFiles = await api.exportBinaryAttachments(rootPath, audience);
        const binaries = normalizeToObject(rawBinaryFiles) ?? [];
        await downloadAsZip(files, binaries);
      }

      open = false;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      pandocProgress = '';
    } finally {
      isExporting = false;
    }
  }

  async function downloadAsZip(files: ExportedFile[], binaryFileInfos: { source_path: string; relative_path: string }[] = []) {
    // Use JSZip library - dynamically import since it's optional
    const JSZip = (await import("jszip")).default;
    const zip = new JSZip();

    // Add text files
    for (const file of files) {
      zip.file(file.path, file.content);
    }

    // Add binary files (fetch data for each file separately to avoid JSON bloat)
    for (const info of binaryFileInfos) {
      try {
        const data = await api!.readBinary(info.source_path);
        zip.file(info.relative_path, data, { binary: true });
      } catch (e) {
        console.warn(`[Export] Failed to read binary file ${info.source_path}:`, e);
      }
    }

    const blob = await zip.generateAsync({ type: "blob" });
    const url = URL.createObjectURL(blob);

    const a = document.createElement("a");
    a.href = url;
    const baseName = rootPath.split("/").pop()?.replace(".md", "") || "export";
    const filename = `${baseName}-export.zip`;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    // Show success toast
    toast.success(`Saved to ${filename}`);
  }

  async function downloadConvertedAsZip(
    convertedFiles: { path: string; data: Uint8Array | string }[],
    binaryFileInfos: { source_path: string; relative_path: string }[] = [],
  ) {
    const JSZip = (await import("jszip")).default;
    const zip = new JSZip();

    for (const file of convertedFiles) {
      if (file.data instanceof Uint8Array) {
        zip.file(file.path, file.data, { binary: true });
      } else {
        zip.file(file.path, file.data);
      }
    }

    // For text formats, also include binary attachments
    for (const info of binaryFileInfos) {
      try {
        const data = await api!.readBinary(info.source_path);
        zip.file(info.relative_path, data, { binary: true });
      } catch (e) {
        console.warn(`[Export] Failed to read binary file ${info.source_path}:`, e);
      }
    }

    const blob = await zip.generateAsync({ type: "blob" });
    const url = URL.createObjectURL(blob);

    const a = document.createElement("a");
    a.href = url;
    const baseName = rootPath.split("/").pop()?.replace(".md", "") || "export";
    const filename = `${baseName}-export.zip`;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    toast.success(`Saved to ${filename}`);
  }

  function toggleNode(path: string) {
    if (expandedNodes.has(path)) {
      expandedNodes.delete(path);
    } else {
      expandedNodes.add(path);
    }
    expandedNodes = new Set(expandedNodes);
  }

  interface TreeNode {
    name: string;
    path: string;
    children: TreeNode[];
    isFile: boolean;
    isBinary?: boolean;
  }

  // Build a tree structure from flat paths
  function buildTree(files: { path: string; relative_path: string }[], binaries: { path: string }[] = []): TreeNode[] {
    const root: TreeNode[] = [];

    // Add markdown files
    for (const file of files) {
      const parts = file.relative_path.split("/");
      let current = root;

      for (let i = 0; i < parts.length; i++) {
        const name = parts[i];
        const isFile = i === parts.length - 1;
        const partPath = parts.slice(0, i + 1).join("/");

        let existing = current.find(n => n.name === name);
        if (!existing) {
          existing = { name, path: partPath, children: [], isFile };
          current.push(existing);
        }
        current = existing.children;
      }
    }

    // Add binary files (attachments)
    for (const file of binaries) {
      const parts = file.path.split("/");
      let current = root;

      for (let i = 0; i < parts.length; i++) {
        const name = parts[i];
        const isFile = i === parts.length - 1;
        const partPath = parts.slice(0, i + 1).join("/");

        let existing = current.find(n => n.name === name);
        if (!existing) {
          existing = { name, path: partPath, children: [], isFile, isBinary: isFile };
          current.push(existing);
        }
        current = existing.children;
      }
    }

    return root;
  }

  const fileTree = $derived(exportPlan?.included ? buildTree(exportPlan.included, binaryFiles) : []);
</script>

<Dialog.Root bind:open onOpenChange={onOpenChange}>
  <Dialog.Content class="sm:max-w-[500px] max-h-[80vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Download class="size-5" />
        Export
      </Dialog.Title>
      <Dialog.Description>
        Export files starting from: <code class="bg-muted px-1 rounded">{rootPath.split("/").pop()}</code>
      </Dialog.Description>
    </Dialog.Header>

    <div class="flex-1 overflow-hidden flex flex-col gap-4 py-4">
      <!-- Audience Selector -->
      <div class="flex items-center gap-2">
        <label for="audience-select" class="text-sm font-medium w-20">Audience:</label>
        <NativeSelect id="audience-select" bind:value={selectedAudience} class="flex-1">
          <option value="all">All (no filter)</option>
          {#each audiences as audience}
            <option value={audience}>{audience}</option>
          {/each}
        </NativeSelect>
      </div>

      <!-- Format Selector -->
      <div class="flex items-center gap-2">
        <label for="format-select" class="text-sm font-medium w-20">Format:</label>
        <NativeSelect id="format-select" bind:value={selectedFormat} class="flex-1">
          {#each EXPORT_FORMATS as fmt}
            <option value={fmt.id}>{fmt.label}</option>
          {/each}
        </NativeSelect>
      </div>

      <!-- Pandoc loading indicator -->
      {#if pandocProgress}
        <div class="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 class="size-4 animate-spin" />
          <span>{pandocProgress}</span>
        </div>
      {/if}

      <!-- Preview Tree -->
      <div class="flex-1 overflow-y-auto border rounded-md p-2 min-h-[200px]">
        <div class="text-xs text-muted-foreground mb-2">
          Files to export ({(exportPlan?.included?.length ?? 0) + binaryFiles.length}):
        </div>

        {#if isLoading}
          <div class="flex items-center justify-center py-8">
            <Loader2 class="size-5 animate-spin text-muted-foreground" />
          </div>
        {:else if error}
          <div class="text-sm text-destructive p-2">{error}</div>
        {:else if fileTree.length === 0}
          <div class="text-sm text-muted-foreground p-2 text-center">
            No files match the selected audience.
          </div>
        {:else}
          {#snippet renderNode(node: TreeNode, depth: number)}
            <div style="padding-left: {depth * 12}px">
              <button
                class="flex items-center gap-1 w-full text-left py-0.5 px-1 rounded hover:bg-accent text-sm"
                onclick={() => node.children.length > 0 && toggleNode(node.path)}
              >
                {#if node.children.length > 0}
                  {#if expandedNodes.has(node.path)}
                    <ChevronDown class="size-3 text-muted-foreground" />
                  {:else}
                    <ChevronRight class="size-3 text-muted-foreground" />
                  {/if}
                  <FolderOpen class="size-4 text-muted-foreground" />
                {:else}
                  <span class="w-3"></span>
                  {#if node.isBinary}
                    {#if /\.(jpg|jpeg|png|gif|webp|svg|bmp|ico|heic|heif)$/i.test(node.name)}
                      <Image class="size-4 text-blue-500" />
                    {:else}
                      <Paperclip class="size-4 text-amber-500" />
                    {/if}
                  {:else}
                    <FileText class="size-4 text-muted-foreground" />
                  {/if}
                {/if}
                <span class="truncate" class:text-blue-600={node.isBinary && /\.(jpg|jpeg|png|gif|webp|svg|bmp|ico|heic|heif)$/i.test(node.name)} class:text-amber-600={node.isBinary && !/\.(jpg|jpeg|png|gif|webp|svg|bmp|ico|heic|heif)$/i.test(node.name)}>{node.name}</span>
              </button>
              {#if node.children.length > 0 && expandedNodes.has(node.path)}
                {#each node.children as child}
                  {@render renderNode(child, depth + 1)}
                {/each}
              {/if}
            </div>
          {/snippet}

          {#each fileTree as node}
            {@render renderNode(node, 0)}
          {/each}
        {/if}
      </div>

      <!-- Excluded count -->
      {#if exportPlan && exportPlan.excluded?.length > 0}
        <div class="text-xs text-muted-foreground">
          {exportPlan.excluded.length} file(s) excluded based on audience settings.
        </div>
      {/if}
    </div>

    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={() => open = false}>
        Cancel
      </Button>
      <Button
        onclick={handleExport}
        disabled={isExporting || !exportPlan || !exportPlan.included || exportPlan.included.length === 0}
      >
        {#if isExporting}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Exporting...
        {:else}
          <Download class="size-4 mr-2" />
          Download ZIP
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
