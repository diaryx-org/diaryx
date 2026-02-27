<script lang="ts">
  import type { ThemeDefinition } from "$lib/stores/appearance.types";
  import { Check } from "@lucide/svelte";

  interface Props {
    preset: ThemeDefinition;
    active: boolean;
    onclick: () => void;
  }

  let { preset, active, onclick }: Props = $props();

  /** Pick 5 representative swatch colors from a palette. */
  function getSwatches(palette: ThemeDefinition["colors"]["light"]) {
    return [
      palette.background,
      palette.primary,
      palette.accent,
      palette.muted,
      palette.foreground,
    ];
  }

  let lightSwatches = $derived(getSwatches(preset.colors.light));
  let darkSwatches = $derived(getSwatches(preset.colors.dark));
</script>

<button
  type="button"
  class="relative flex flex-col gap-1.5 rounded-lg border p-3 text-left transition-colors cursor-pointer
    {active ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-primary/40'}"
  {onclick}
>
  {#if active}
    <div class="absolute top-1.5 right-1.5 rounded-full bg-primary p-0.5">
      <Check class="size-3 text-primary-foreground" />
    </div>
  {/if}

  <span class="text-sm font-medium">{preset.name}</span>

  <!-- Light mode swatches -->
  <div class="flex gap-1">
    {#each lightSwatches as color}
      <span
        class="size-4 rounded-full border border-black/10"
        style:background={color}
      ></span>
    {/each}
  </div>

  <!-- Dark mode swatches -->
  <div class="flex gap-1">
    {#each darkSwatches as color}
      <span
        class="size-4 rounded-full border border-white/10"
        style:background={color}
      ></span>
    {/each}
  </div>
</button>
