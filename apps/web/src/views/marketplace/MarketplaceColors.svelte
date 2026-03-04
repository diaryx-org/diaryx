<script lang="ts">
  import { Upload, Download } from "@lucide/svelte";
  import { Label } from "$lib/components/ui/label";
  import { Button } from "$lib/components/ui/button";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import { PRESET_LIST } from "$lib/stores/appearance.presets";
  import ThemePresetCard from "$lib/settings/ThemePresetCard.svelte";
  import AccentHuePicker from "$lib/settings/AccentHuePicker.svelte";

  const store = getAppearanceStore();

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

<div class="space-y-3 px-3 py-2">
  <!-- Import / Export -->
  <div class="flex gap-2 pt-1">
    <Button variant="outline" size="sm" class="text-xs flex-1" onclick={handleImport}>
      <Upload class="size-3 mr-1" />
      Import
    </Button>
    <Button variant="outline" size="sm" class="text-xs flex-1" onclick={handleExport}>
      <Download class="size-3 mr-1" />
      Export
    </Button>
  </div>

  <!-- Theme Presets -->
  <div>
    <Label class="text-sm mb-2 block">Theme</Label>
    <div class="grid grid-cols-1 gap-2">
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
  <div class="space-y-1.5">
    <Label class="text-sm">Accent color</Label>
    <AccentHuePicker
      value={store.accentHue}
      onchange={(hue) => store.setAccentHue(hue)}
    />
  </div>
</div>
