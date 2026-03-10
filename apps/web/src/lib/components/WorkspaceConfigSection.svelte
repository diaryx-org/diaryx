<script lang="ts">
  import { Input } from "$lib/components/ui/input";
  import { Switch } from "$lib/components/ui/switch";
  import * as Select from "$lib/components/ui/select";
  import { AlertCircle } from "@lucide/svelte";
  import { getWorkspaceConfigStore } from "$lib/stores/workspaceConfigStore.svelte";

  interface Props {
    rootIndexPath: string;
  }

  let { rootIndexPath }: Props = $props();

  const configStore = getWorkspaceConfigStore();

  // Local state for default_audience input (save on blur/Enter)
  let defaultAudience = $state("");
  let dailyEntryFolder = $state("");

  // Load config when root index path changes
  $effect(() => {
    if (rootIndexPath) {
      configStore.load(rootIndexPath);
    }
  });

  // Sync default audience input from config
  $effect(() => {
    if (configStore.config) {
      defaultAudience = configStore.config.default_audience ?? "";
      dailyEntryFolder = configStore.config.daily_entry_folder ?? "";
    }
  });

  function saveDefaultAudience() {
    configStore.setField("default_audience", defaultAudience.trim());
  }

  function saveDailyEntryFolder() {
    configStore.setField("daily_entry_folder", dailyEntryFolder.trim());
  }

  const LINK_FORMAT_OPTIONS = [
    { value: "markdown_root", label: "Markdown (root)", example: "[Title](/path.md)" },
    { value: "markdown_relative", label: "Markdown (relative)", example: "[Title](../path.md)" },
    { value: "plain_relative", label: "Plain (relative)", example: "../path.md" },
    { value: "plain_canonical", label: "Plain (canonical)", example: "path/to/file.md" },
  ];

  const FILENAME_STYLE_OPTIONS = [
    { value: "preserve", label: "Preserve" },
    { value: "kebab_case", label: "kebab-case" },
    { value: "snake_case", label: "snake_case" },
    { value: "screaming_snake_case", label: "SCREAMING_SNAKE" },
  ];

  function getLinkFormatLabel(value: string): string {
    return LINK_FORMAT_OPTIONS.find((o) => o.value === value)?.label ?? value;
  }

  function getFilenameStyleLabel(value: string): string {
    return FILENAME_STYLE_OPTIONS.find((o) => o.value === value)?.label ?? value;
  }
</script>

<div class="space-y-3">
  <!-- Link Format -->
  <div class="space-y-1">
    <span class="text-xs text-muted-foreground">Link format</span>
    <Select.Root
      type="single"
      value={configStore.config?.link_format ?? "markdown_root"}
      onValueChange={(value) => { if (value) configStore.setField("link_format", value); }}
      disabled={configStore.loading}
    >
      <Select.Trigger class="h-7 text-xs w-full">
        {getLinkFormatLabel(configStore.config?.link_format ?? "markdown_root")}
      </Select.Trigger>
      <Select.Content>
        {#each LINK_FORMAT_OPTIONS as option}
          <Select.Item value={option.value}>
            <div class="flex flex-col">
              <span class="text-xs">{option.label}</span>
              <span class="text-[10px] text-muted-foreground font-mono">{option.example}</span>
            </div>
          </Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  <!-- Default Audience -->
  <div class="space-y-1">
    <span class="text-xs text-muted-foreground">Default audience tag</span>
    <Input
      type="text"
      bind:value={defaultAudience}
      placeholder="e.g., public"
      class="h-7 text-xs"
      disabled={configStore.loading}
      onblur={saveDefaultAudience}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          saveDefaultAudience();
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  </div>

  <!-- Daily Folder -->
  <div class="space-y-1">
    <span class="text-xs text-muted-foreground">Daily folder</span>
    <Input
      type="text"
      bind:value={dailyEntryFolder}
      placeholder="e.g., Daily or Journal/Daily"
      class="h-7 text-xs"
      disabled={configStore.loading}
      onblur={saveDailyEntryFolder}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          saveDailyEntryFolder();
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
    <p class="text-[10px] text-muted-foreground">
      Used by the Daily plugin for yearly/monthly/daily entry creation.
    </p>
  </div>

  <!-- Filename Style -->
  <div class="space-y-1">
    <span class="text-xs text-muted-foreground">Filename style</span>
    <Select.Root
      type="single"
      value={configStore.config?.filename_style ?? "preserve"}
      onValueChange={(value) => { if (value) configStore.setField("filename_style", value); }}
      disabled={configStore.loading}
    >
      <Select.Trigger class="h-7 text-xs w-full">
        {getFilenameStyleLabel(configStore.config?.filename_style ?? "preserve")}
      </Select.Trigger>
      <Select.Content>
        {#each FILENAME_STYLE_OPTIONS as option}
          <Select.Item value={option.value}>
            <span class="text-xs">{option.label}</span>
          </Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  <!-- Auto-update timestamp -->
  <div class="flex items-center justify-between gap-2">
    <span class="text-xs">Auto-update timestamp</span>
    <Switch
      checked={configStore.config?.auto_update_timestamp ?? true}
      onCheckedChange={(checked) => configStore.setField("auto_update_timestamp", String(checked))}
      disabled={configStore.loading}
      class="scale-75 origin-right"
    />
  </div>

  {#if configStore.error}
    <div class="flex items-center gap-1.5 text-[10px] text-destructive">
      <AlertCircle class="size-3" />
      <span>{configStore.error}</span>
    </div>
  {/if}
</div>
