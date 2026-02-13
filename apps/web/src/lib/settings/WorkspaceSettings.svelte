<script lang="ts">
  /**
   * WorkspaceSettings - Workspace folder and behavior configuration
   *
   * Shows the current workspace path, allows changing it (Tauri only),
   * configures the daily entry folder, and manages entry behavior settings
   * like auto-update timestamp, sync title to heading, auto-rename, and filename style.
   *
   * Daily entry folder and behavior settings are stored in workspace config
   * (root index frontmatter) so they sync across devices.
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import * as Select from "$lib/components/ui/select";
  import { FolderOpen, RefreshCw, Calendar, Check, Settings2, AlertCircle } from "@lucide/svelte";
  import { getBackend, isTauri } from "../backend";
  import { getWorkspaceConfigStore } from "../stores/workspaceConfigStore.svelte";

  interface Props {
    workspaceRootIndex?: string | null;
  }

  let { workspaceRootIndex = null }: Props = $props();

  const configStore = getWorkspaceConfigStore();

  // Current workspace path
  let workspacePath = $state<string | null>(null);
  let isChanging = $state(false);
  let error = $state<string | null>(null);

  // Daily entry folder (bound to input, synced from config store)
  let dailyEntryFolder = $state("");
  let dailyFolderSaved = $state(false);

  // Load workspace path on mount
  $effect(() => {
    loadWorkspacePath();
  });

  // Load workspace config when root index changes
  $effect(() => {
    if (workspaceRootIndex) {
      configStore.load(workspaceRootIndex);
    }
  });

  // Sync daily entry folder input from config store + migrate from localStorage
  $effect(() => {
    if (configStore.config) {
      const configValue = configStore.config.daily_entry_folder ?? "";
      const localValue = typeof window !== "undefined"
        ? localStorage.getItem("diaryx-daily-entry-folder") || ""
        : "";

      if (configValue) {
        // Workspace config has a value — use it
        dailyEntryFolder = configValue;
        // Clear localStorage if it was set (migration complete)
        if (localValue && typeof window !== "undefined") {
          localStorage.removeItem("diaryx-daily-entry-folder");
        }
      } else if (localValue) {
        // localStorage has a value but workspace config doesn't — migrate
        dailyEntryFolder = localValue;
        configStore.setField("daily_entry_folder", localValue);
        localStorage.removeItem("diaryx-daily-entry-folder");
      } else {
        dailyEntryFolder = "";
      }
    }
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

  async function saveDailyEntryFolder() {
    const folder = dailyEntryFolder.trim();
    await configStore.setField("daily_entry_folder", folder);
    dailyFolderSaved = true;
    setTimeout(() => {
      dailyFolderSaved = false;
    }, 2000);
  }

  function clearDailyEntryFolder() {
    dailyEntryFolder = "";
    configStore.setField("daily_entry_folder", "");
  }

  // Filename style options
  const FILENAME_STYLE_OPTIONS = [
    { value: "preserve", label: "Preserve", description: "Keep original casing and spacing" },
    { value: "kebab_case", label: "kebab-case", description: "lowercase-with-dashes" },
    { value: "snake_case", label: "snake_case", description: "lowercase_with_underscores" },
    { value: "screaming_snake_case", label: "SCREAMING_SNAKE_CASE", description: "UPPERCASE_WITH_UNDERSCORES" },
  ];

  function getFilenameStyleLabel(value: string): string {
    return FILENAME_STYLE_OPTIONS.find((o) => o.value === value)?.label ?? value;
  }
</script>

<div class="space-y-4">
  <!-- Workspace Location -->
  <div class="space-y-3">
    <h3 class="font-medium flex items-center gap-2">
      <FolderOpen class="size-4" />
      Workspace Location
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

  <!-- Daily Entry Folder -->
  <div class="space-y-3 pt-2 border-t">
    <h3 class="font-medium flex items-center gap-2">
      <Calendar class="size-4" />
      Daily Entries
    </h3>

    <p class="text-xs text-muted-foreground px-1">
      Configure where daily journal entries are created. Leave empty to create them at the workspace root.
    </p>

    <div class="space-y-2 px-1">
      <Label for="daily-entry-folder" class="text-xs text-muted-foreground">
        Daily Entry Folder
      </Label>
      <div class="flex gap-2">
        <Input
          id="daily-entry-folder"
          type="text"
          bind:value={dailyEntryFolder}
          placeholder="e.g., Daily or Journal/Daily"
          class="text-sm"
          disabled={configStore.loading || !workspaceRootIndex}
          onkeydown={(e) => e.key === "Enter" && saveDailyEntryFolder()}
        />
        <Button
          variant="secondary"
          size="sm"
          onclick={saveDailyEntryFolder}
          disabled={configStore.loading || !workspaceRootIndex}
        >
          {#if dailyFolderSaved}
            <Check class="size-4 text-green-600" />
          {:else}
            Save
          {/if}
        </Button>
      </div>
      <p class="text-xs text-muted-foreground">
        Daily entries will be organized as: <code class="bg-muted px-1 rounded">{dailyEntryFolder || "workspace"}/2026/01/2026-01-17.md</code>
      </p>

      {#if dailyEntryFolder}
        <Button
          variant="ghost"
          size="sm"
          class="text-xs text-muted-foreground h-7"
          onclick={clearDailyEntryFolder}
        >
          Clear folder (use workspace root)
        </Button>
      {/if}
    </div>
  </div>

  <!-- Entry Behavior -->
  <div class="space-y-3 pt-2 border-t">
    <h3 class="font-medium flex items-center gap-2">
      <Settings2 class="size-4" />
      Entry Behavior
    </h3>

    <div class="space-y-4 px-1">
      <!-- Auto-update timestamp -->
      <div class="flex items-center justify-between gap-4">
        <Label for="auto-update-timestamp" class="text-sm flex flex-col gap-0.5">
          <span>Auto-update timestamp</span>
          <span class="font-normal text-xs text-muted-foreground">
            Automatically update the <code class="bg-muted px-1 rounded">updated</code> field when saving.
          </span>
        </Label>
        <Switch
          id="auto-update-timestamp"
          checked={configStore.config?.auto_update_timestamp ?? true}
          onCheckedChange={(checked) => configStore.setField("auto_update_timestamp", String(checked))}
          disabled={configStore.loading || !workspaceRootIndex}
        />
      </div>

      <!-- Sync title to heading -->
      <div class="flex items-center justify-between gap-4">
        <Label for="sync-title-heading" class="text-sm flex flex-col gap-0.5">
          <span>Sync title to heading</span>
          <span class="font-normal text-xs text-muted-foreground">
            When changing a title, also update the first H1 heading in the body.
          </span>
        </Label>
        <Switch
          id="sync-title-heading"
          checked={configStore.config?.sync_title_to_heading ?? false}
          onCheckedChange={(checked) => configStore.setField("sync_title_to_heading", String(checked))}
          disabled={configStore.loading || !workspaceRootIndex}
        />
      </div>

      <!-- Auto-rename to title -->
      <div class="flex items-center justify-between gap-4">
        <Label for="auto-rename-title" class="text-sm flex flex-col gap-0.5">
          <span>Auto-rename to title</span>
          <span class="font-normal text-xs text-muted-foreground">
            Automatically rename files when the title changes.
          </span>
        </Label>
        <Switch
          id="auto-rename-title"
          checked={configStore.config?.auto_rename_to_title ?? true}
          onCheckedChange={(checked) => configStore.setField("auto_rename_to_title", String(checked))}
          disabled={configStore.loading || !workspaceRootIndex}
        />
      </div>

      <!-- Filename Style -->
      {#if configStore.config?.auto_rename_to_title !== false}
        <div class="space-y-2">
          <Label for="filename-style" class="text-sm flex flex-col gap-0.5">
            <span>Filename style</span>
            <span class="font-normal text-xs text-muted-foreground">
              How filenames are generated from entry titles when renaming.
            </span>
          </Label>
          <Select.Root
            type="single"
            value={configStore.config?.filename_style ?? "preserve"}
            onValueChange={(value) => { if (value) configStore.setField("filename_style", value); }}
            disabled={configStore.loading || !workspaceRootIndex}
          >
            <Select.Trigger id="filename-style" class="w-full">
              {getFilenameStyleLabel(configStore.config?.filename_style ?? "preserve")}
            </Select.Trigger>
            <Select.Content>
              {#each FILENAME_STYLE_OPTIONS as option}
                <Select.Item value={option.value}>
                  <div class="flex flex-col gap-0.5">
                    <span>{option.label}</span>
                    <span class="text-xs text-muted-foreground font-mono">
                      {option.description}
                    </span>
                  </div>
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>
      {/if}
    </div>

    {#if configStore.error}
      <div class="flex items-center gap-2 text-xs text-destructive px-1">
        <AlertCircle class="size-3" />
        <span>{configStore.error}</span>
      </div>
    {/if}
  </div>
</div>
