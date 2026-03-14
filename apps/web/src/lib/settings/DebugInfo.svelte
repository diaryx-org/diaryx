<script lang="ts">
  /**
   * DebugInfo - App paths and config debug information
   *
   * Extracted from SettingsDialog for modularity.
   * Uses the Backend interface to get config and app paths.
   */
  import { FileText, FolderOpen, Info, Loader2, RefreshCw, Settings } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { getBackend } from "../backend";
  import type { Config } from "../backend/interface";

  // Config info state
  let config: Config | null = $state(null);
  let appPaths: Record<string, string | boolean | null> | null = $state(null);
  let canRevealLogFile = $state(false);
  let canReadLogFile = $state(false);
  let logContents = $state<string | null>(null);
  let logError = $state<string | null>(null);
  let logLoading = $state(false);

  // Load config on mount
  $effect(() => {
    loadConfig();
  });

  async function loadConfig() {
    try {
      const backend = await getBackend();
      // Use the Backend interface methods
      config = backend.getConfig();
      appPaths = backend.getAppPaths();
      canRevealLogFile = typeof backend.revealInFileManager === "function";
      canReadLogFile = typeof backend.readLogFile === "function";
      if (canReadLogFile && typeof appPaths?.log_file === "string" && appPaths.log_file) {
        await loadLogFile(backend);
      } else {
        logContents = null;
        logError = null;
      }
    } catch (e) {
      console.warn("[DebugInfo] Failed to load config:", e);
      config = null;
      appPaths = null;
      canRevealLogFile = false;
      canReadLogFile = false;
      logContents = null;
      logError = null;
    }
  }

  async function revealLogFile() {
    const logFile = appPaths?.log_file;
    if (!canRevealLogFile || typeof logFile !== "string" || !logFile) return;

    try {
      const backend = await getBackend();
      await backend.revealInFileManager?.(logFile);
    } catch (e) {
      console.warn("[DebugInfo] Failed to reveal log file:", e);
    }
  }

  async function loadLogFile(existingBackend?: Awaited<ReturnType<typeof getBackend>>) {
    if (!canReadLogFile) return;

    logLoading = true;
    logError = null;

    try {
      const backend = existingBackend ?? await getBackend();
      const contents = await backend.readLogFile?.();
      logContents = contents ?? "";
    } catch (e) {
      console.warn("[DebugInfo] Failed to read log file:", e);
      logContents = null;
      logError = e instanceof Error ? e.message : "Failed to read log file";
    } finally {
      logLoading = false;
    }
  }
</script>

<!-- Path Info -->
{#if appPaths}
  <div class="space-y-2">
    <h3 class="font-medium flex items-center gap-2">
      <Info class="size-4" />App Paths
    </h3>
    <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
      {#each Object.entries(appPaths) as [key, value]}
        <div class="flex gap-2">
          <span class="text-muted-foreground min-w-[120px]">{key}:</span>
          <span class="break-all">{value}</span>
        </div>
      {/each}
    </div>
    {#if canRevealLogFile && typeof appPaths.log_file === "string" && appPaths.log_file}
      <Button variant="outline" size="sm" class="w-full" onclick={revealLogFile}>
        <FolderOpen class="size-4 mr-2" />
        Reveal Log File
      </Button>
    {/if}
  </div>
{/if}

{#if canReadLogFile && appPaths && typeof appPaths.log_file === "string" && appPaths.log_file}
  <div class="space-y-2">
    <div class="flex items-center justify-between gap-2">
      <h3 class="font-medium flex items-center gap-2">
        <FileText class="size-4" />Log Contents
      </h3>
      <Button variant="outline" size="sm" onclick={() => void loadLogFile()} disabled={logLoading}>
        {#if logLoading}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Loading...
        {:else}
          <RefreshCw class="size-4 mr-2" />
          Reload Log
        {/if}
      </Button>
    </div>

    {#if logError}
      <div class="rounded border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive">
        {logError}
      </div>
    {:else}
      <div class="bg-muted rounded p-3 text-[11px] font-mono max-h-80 overflow-auto">
        <pre class="whitespace-pre-wrap break-words">{logContents?.length ? logContents : "Log file is empty."}</pre>
      </div>
    {/if}
  </div>
{/if}

{#if config}
  <div class="space-y-2">
    <h3 class="font-medium flex items-center gap-2">
      <Settings class="size-4" />Config
    </h3>
    <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
      {#each Object.entries(config) as [key, value]}
        <div class="flex gap-2">
          <span class="text-muted-foreground min-w-[120px]">{key}:</span>
          <span class="break-all"
            >{typeof value === "object"
              ? JSON.stringify(value)
              : String(value ?? "null")}</span
          >
        </div>
      {/each}
    </div>
  </div>
{/if}
