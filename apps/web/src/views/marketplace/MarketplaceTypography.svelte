<script lang="ts">
  import { Label } from "$lib/components/ui/label";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import type { FontFamily, ContentWidth } from "$lib/stores/appearance.types";

  const store = getAppearanceStore();

  const fontOptions: { value: FontFamily; label: string }[] = [
    { value: "inter", label: "Inter" },
    { value: "system", label: "System" },
    { value: "serif", label: "Serif" },
    { value: "mono", label: "Mono" },
  ];

  const fontSizeOptions = [14, 15, 16, 17, 18, 19, 20];

  const lineHeightOptions: { value: number; label: string }[] = [
    { value: 1.4, label: "Compact" },
    { value: 1.5, label: "Snug" },
    { value: 1.6, label: "Default" },
    { value: 1.8, label: "Relaxed" },
    { value: 2.0, label: "Loose" },
  ];

  const widthOptions: { value: ContentWidth; label: string }[] = [
    { value: "narrow", label: "Narrow" },
    { value: "medium", label: "Medium" },
    { value: "wide", label: "Wide" },
    { value: "full", label: "Full" },
  ];
</script>

<div class="space-y-3 px-3 py-2">
  <!-- Font Family -->
  <div class="flex items-center justify-between gap-4">
    <Label for="mp-editor-font" class="text-sm cursor-pointer">Editor font</Label>
    <select
      id="mp-editor-font"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.fontFamily}
      onchange={(e) => store.setFontFamily((e.target as HTMLSelectElement).value as FontFamily)}
    >
      {#each fontOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Font Size -->
  <div class="flex items-center justify-between gap-4">
    <Label for="mp-editor-font-size" class="text-sm cursor-pointer">Font size</Label>
    <select
      id="mp-editor-font-size"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.baseFontSize}
      onchange={(e) => store.setBaseFontSize(parseInt((e.target as HTMLSelectElement).value, 10))}
    >
      {#each fontSizeOptions as size}
        <option value={size}>{size}px</option>
      {/each}
    </select>
  </div>

  <!-- Line Height -->
  <div class="flex items-center justify-between gap-4">
    <Label for="mp-editor-line-height" class="text-sm cursor-pointer">Line spacing</Label>
    <select
      id="mp-editor-line-height"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.typography.lineHeight}
      onchange={(e) => store.setLineHeight(parseFloat((e.target as HTMLSelectElement).value))}
    >
      {#each lineHeightOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Content Width -->
  <div class="flex items-center justify-between gap-4">
    <Label for="mp-content-width" class="text-sm cursor-pointer">Content width</Label>
    <select
      id="mp-content-width"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={store.layout.contentWidth}
      onchange={(e) => store.setContentWidth((e.target as HTMLSelectElement).value as ContentWidth)}
    >
      {#each widthOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>
</div>
