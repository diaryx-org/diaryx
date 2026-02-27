<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { RotateCcw } from "@lucide/svelte";

  interface Props {
    value: number | null;
    onchange: (hue: number | null) => void;
  }

  let { value, onchange }: Props = $props();

  let displayValue = $derived(value ?? 0);

  function handleInput(e: Event) {
    const hue = parseInt((e.target as HTMLInputElement).value, 10);
    onchange(hue);
  }

  function handleReset() {
    onchange(null);
  }
</script>

<div class="flex items-center gap-3">
  <div class="relative flex-1">
    <input
      type="range"
      min="0"
      max="360"
      step="1"
      value={displayValue}
      oninput={handleInput}
      class="w-full h-3 rounded-full appearance-none cursor-pointer accent-hue-slider"
    />
  </div>
  <Button
    variant="ghost"
    size="sm"
    class="h-7 px-2 text-xs"
    onclick={handleReset}
    disabled={value === null}
  >
    <RotateCcw class="size-3 mr-1" />
    Reset
  </Button>
</div>

<style>
  .accent-hue-slider {
    background: linear-gradient(
      to right,
      oklch(0.7 0.15 0),
      oklch(0.7 0.15 60),
      oklch(0.7 0.15 120),
      oklch(0.7 0.15 180),
      oklch(0.7 0.15 240),
      oklch(0.7 0.15 300),
      oklch(0.7 0.15 360)
    );
  }

  .accent-hue-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: white;
    border: 2px solid var(--border);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    cursor: pointer;
  }

  .accent-hue-slider::-moz-range-thumb {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: white;
    border: 2px solid var(--border);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    cursor: pointer;
  }
</style>
