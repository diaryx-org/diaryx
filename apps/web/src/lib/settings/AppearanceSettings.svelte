<script lang="ts">
  import { Palette, Upload, Download } from "@lucide/svelte";
  import { Label } from "$lib/components/ui/label";
  import { Button } from "$lib/components/ui/button";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import { PRESET_LIST } from "$lib/stores/appearance.presets";
  import type { FontFamily, ContentWidth } from "$lib/stores/appearance.types";
  import ThemePresetCard from "./ThemePresetCard.svelte";
  import AccentHuePicker from "./AccentHuePicker.svelte";

  const store = getAppearanceStore();

  // Font options
  const fontOptions: { value: FontFamily; label: string }[] = [
    { value: "inter", label: "Inter" },
    { value: "system", label: "System" },
    { value: "serif", label: "Serif" },
    { value: "mono", label: "Mono" },
  ];

  // Font size options
  const fontSizeOptions = [14, 15, 16, 17, 18, 19, 20];

  // Line height options
  const lineHeightOptions: { value: number; label: string }[] = [
    { value: 1.4, label: "Compact" },
    { value: 1.5, label: "Snug" },
    { value: 1.6, label: "Default" },
    { value: 1.8, label: "Relaxed" },
    { value: 2.0, label: "Loose" },
  ];

  // Content width options
  const widthOptions: { value: ContentWidth; label: string }[] = [
    { value: "narrow", label: "Narrow" },
    { value: "medium", label: "Medium" },
    { value: "wide", label: "Wide" },
    { value: "full", label: "Full" },
  ];

  function handleExport() {
    const data = store.exportTheme();
    const json = JSON.stringify(data, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `diaryx-theme-${store.presetId}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function handleImport() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const data = JSON.parse(text);
        if (!store.importTheme(data)) {
          alert("Invalid theme file.");
        }
      } catch {
        alert("Failed to read theme file.");
      }
    };
    input.click();
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Palette class="size-4" />
    Appearance
  </h3>

  <!-- Theme Presets -->
  <div class="px-1">
    <Label class="text-sm mb-2 block">Theme</Label>
    <div class="grid grid-cols-2 gap-2">
      {#each PRESET_LIST as preset}
        <ThemePresetCard
          {preset}
          active={store.presetId === preset.id}
          onclick={() => store.setPreset(preset.id)}
        />
      {/each}
    </div>
  </div>

  <!-- Accent Hue -->
  <div class="px-1 space-y-1.5">
    <Label class="text-sm">Accent color</Label>
    <AccentHuePicker
      value={store.accentHue}
      onchange={(hue) => store.setAccentHue(hue)}
    />
  </div>

  <!-- Typography: Font Family -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="editor-font" class="text-sm cursor-pointer">Editor font</Label>
    <select
      id="editor-font"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.fontFamily}
      onchange={(e) => store.setFontFamily((e.target as HTMLSelectElement).value as FontFamily)}
    >
      {#each fontOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Typography: Font Size -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="editor-font-size" class="text-sm cursor-pointer">Font size</Label>
    <select
      id="editor-font-size"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.baseFontSize}
      onchange={(e) => store.setBaseFontSize(parseInt((e.target as HTMLSelectElement).value, 10))}
    >
      {#each fontSizeOptions as size}
        <option value={size}>{size}px</option>
      {/each}
    </select>
  </div>

  <!-- Typography: Line Height -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="editor-line-height" class="text-sm cursor-pointer">Line spacing</Label>
    <select
      id="editor-line-height"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.lineHeight}
      onchange={(e) => store.setLineHeight(parseFloat((e.target as HTMLSelectElement).value))}
    >
      {#each lineHeightOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Layout: Content Width -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="content-width" class="text-sm cursor-pointer">Content width</Label>
    <select
      id="content-width"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.layout.contentWidth}
      onchange={(e) => store.setContentWidth((e.target as HTMLSelectElement).value as ContentWidth)}
    >
      {#each widthOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Import / Export -->
  <div class="flex gap-2 px-1 pt-1">
    <Button variant="outline" size="sm" class="text-xs" onclick={handleImport}>
      <Upload class="size-3 mr-1" />
      Import theme
    </Button>
    <Button variant="outline" size="sm" class="text-xs" onclick={handleExport}>
      <Download class="size-3 mr-1" />
      Export theme
    </Button>
  </div>
</div>
