<script lang="ts">
  /**
   * DisplaySettings - Display and theme settings section
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Eye, Sun, Moon, Monitor } from "@lucide/svelte";
  import { getThemeStore, type ThemeMode } from "../stores/theme.svelte";
  import { getAppearanceStore } from "../stores/appearance.svelte";

  interface Props {
    focusMode?: boolean;
  }

  let {
    focusMode = $bindable(true),
  }: Props = $props();

  const themeStore = getThemeStore();
  const appearanceStore = getAppearanceStore();
</script>

<div class="space-y-3">
  <!-- Diaryx Branding -->
  <div class="flex items-center gap-3 px-1 pb-2 border-b border-border">
    <img
      src={themeStore.resolvedTheme === 'dark' ? '/icon-dark.png' : '/icon.png'}
      alt="Diaryx"
      class="size-8 rounded"
    />
    <div>
      <span class="text-sm font-semibold">Diaryx</span>
      <span class="text-xs text-muted-foreground ml-1.5">v{__APP_VERSION__}</span>
    </div>
  </div>

  <h3 class="font-medium flex items-center gap-2">
    <Eye class="size-4" />
    Display
  </h3>

  <!-- Theme Selection -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="theme-mode" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Theme</span>
      <span class="font-normal text-xs text-muted-foreground">
        Choose light, dark, or follow system preference.
      </span>
    </Label>
    <select
      id="theme-mode"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={themeStore.mode}
      onchange={(e) => themeStore.setMode((e.target as HTMLSelectElement).value as ThemeMode)}
    >
      <option value="system">
        <Monitor class="size-3" /> System
      </option>
      <option value="light">
        <Sun class="size-3" /> Light
      </option>
      <option value="dark">
        <Moon class="size-3" /> Dark
      </option>
    </select>
  </div>

  <!-- High Contrast Editor -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="high-contrast-editor" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>High contrast editor</span>
      <span class="font-normal text-xs text-muted-foreground">
        Use pure black and white for editor text and background, regardless of theme.
      </span>
    </Label>
    <Switch
      id="high-contrast-editor"
      checked={appearanceStore.highContrastEditor}
      onCheckedChange={(checked) => appearanceStore.setHighContrastEditor(checked)}
    />
  </div>

  <!-- Focus Mode -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="focus-mode" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Focus mode</span>
      <span class="font-normal text-xs text-muted-foreground">
        Fade the editor chrome when both sidebars are closed. Hover to reveal.
      </span>
    </Label>
    <Switch id="focus-mode" bind:checked={focusMode} />
  </div>
</div>
