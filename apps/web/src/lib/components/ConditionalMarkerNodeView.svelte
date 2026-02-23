<script lang="ts">
  /**
   * ConditionalMarkerNodeView — Block-level marker for conditional blocks.
   *
   * Renders as a subtle label/divider showing the condition for open/else/close
   * markers in {{#if}}/{{#for-audience}} conditional blocks.
   */

  interface Props {
    variant: "open" | "else" | "close";
    helperType: string;
    condition: string;
    readonly?: boolean;
    onDelete?: () => void;
  }

  let {
    variant,
    helperType,
    condition,
    readonly = false,
    onDelete,
  }: Props = $props();

  let label = $derived.by(() => {
    switch (variant) {
      case "open":
        if (helperType === "for-audience") return `for-audience "${condition}"`;
        return `if ${condition}`;
      case "else":
        return "else";
      case "close":
        if (helperType === "for-audience") return "end for-audience";
        return `end ${helperType || "if"}`;
    }
  });

  let icon = $derived.by(() => {
    switch (variant) {
      case "open":
        return "\u25B6";
      case "else":
        return "\u25C7";
      case "close":
        return "\u25C0";
    }
  });
</script>

<div
  class="conditional-marker-pill"
  class:marker-open={variant === "open"}
  class:marker-else={variant === "else"}
  class:marker-close={variant === "close"}
>
  <span class="conditional-marker-icon">{icon}</span>
  <span class="conditional-marker-label">{label}</span>
  {#if !readonly && onDelete}
    <button
      class="conditional-marker-delete"
      onclick={onDelete}
      title="Delete this marker"
    >
      &times;
    </button>
  {/if}
</div>

<style>
  .conditional-marker-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 10px;
    border-radius: 4px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.8em;
    line-height: 1.4;
    color: var(--muted-foreground);
    background: color-mix(in oklch, var(--muted) 60%, transparent);
    border: 1px dashed var(--border);
    cursor: default;
    user-select: none;
  }

  .conditional-marker-pill.marker-open {
    border-left: 3px solid var(--primary);
  }

  .conditional-marker-pill.marker-else {
    border-left: 3px solid
      color-mix(in oklch, var(--primary) 50%, var(--muted-foreground));
  }

  .conditional-marker-pill.marker-close {
    border-left: 3px solid var(--border);
  }

  .conditional-marker-icon {
    font-size: 0.8em;
    opacity: 0.6;
  }

  .conditional-marker-label {
    font-weight: 500;
  }

  .conditional-marker-delete {
    background: none;
    border: none;
    color: var(--muted-foreground);
    cursor: pointer;
    padding: 0 2px;
    font-size: 1em;
    opacity: 0.5;
    transition: opacity 0.15s;
  }

  .conditional-marker-delete:hover {
    opacity: 1;
    color: var(--destructive, oklch(0.577 0.245 27.325));
  }
</style>
