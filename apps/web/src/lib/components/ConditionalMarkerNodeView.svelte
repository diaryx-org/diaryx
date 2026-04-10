<script lang="ts">
  /**
   * ConditionalMarkerNodeView — Block-level marker for conditional blocks.
   *
   * Renders as a subtle label/divider showing the condition for open/else/close
   * markers in {{#if}}/{{#for-audience}} conditional blocks.
   *
   * For-audience "open" markers receive an enhanced "BlockHeader" treatment:
   *   • Users icon + colored accent dot + bold audience name + subtle Lock icon
   *   • Accent background tint matching the primary color
   *   • Filter-aware dimming: reads templateContextStore.previewAudience and
   *     applies opacity-45 when a filter is active but this block doesn't match
   */

  import { Users, Lock } from "@lucide/svelte";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";

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

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();

  /** Persistent Tailwind bg class for the audience dot. */
  const audienceDotClass = $derived(
    getAudienceColor(condition, colorStore.audienceColors),
  );

  /** True when the user has selected a specific audience to filter by */
  const isFilterActive = $derived(templateContextStore.previewAudience !== null);

  /**
   * True when a filter is active AND this for-audience block does not match it.
   * Drives opacity dimming + faded-lock visual on the BlockHeader.
   */
  const isFilteredOut = $derived(
    helperType === "for-audience" &&
      variant === "open" &&
      isFilterActive &&
      !templateContextStore.previewAudience?.some(
        (a) => a.toLowerCase() === condition.toLowerCase(),
      ),
  );

  const isAudienceOpen = $derived(
    helperType === "for-audience" && variant === "open",
  );

  let label = $derived.by(() => {
    switch (variant) {
      case "open":
        // Enhanced: just the name for audience blocks; keep "if X" for if-blocks
        if (helperType === "for-audience") return condition;
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
  class:audience-open={isAudienceOpen}
  class:filtered-out={isFilteredOut}
>
  {#if isAudienceOpen}
    <!-- ── Enhanced BlockHeader for for-audience open markers ── -->
    <Users class="audience-users-icon" aria-hidden="true" />
    <span class="audience-dot {audienceDotClass}" aria-hidden="true"></span>
    <span class="audience-name">{label}</span>
    <!-- Lock sits on the far right; brightens when filtered out -->
    <span
      class="audience-lock"
      class:lock-active={isFilteredOut}
      aria-label={isFilteredOut ? "Not visible to current audience filter" : "Audience restricted"}
    >
      <Lock class="lock-icon" aria-hidden="true" />
    </span>
  {:else}
    <!-- ── Standard pill for if / else / close markers ── -->
    <span class="conditional-marker-icon">{icon}</span>
    <span class="conditional-marker-label">{label}</span>
  {/if}

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
  /* ── Base pill ───────────────────────────────────────────────────── */
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
    transition: opacity 0.2s ease, transform 0.15s ease, box-shadow 0.15s ease;
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

  /* ── For-audience open marker — enhanced BlockHeader ────────────── */
  .conditional-marker-pill.audience-open {
    gap: 6px;
    padding: 3px 10px 3px 8px;
    /* Faint primary background tint */
    background: color-mix(
      in oklch,
      var(--primary) 7%,
      color-mix(in oklch, var(--muted) 60%, transparent)
    );
    border: 1px solid color-mix(in oklch, var(--primary) 25%, transparent);
    border-left: 3px solid var(--primary);
  }

  .conditional-marker-pill.audience-open:hover {
    transform: scale(1.01);
    box-shadow: 0 2px 10px color-mix(in oklch, var(--primary) 12%, transparent);
    border-color: color-mix(in oklch, var(--primary) 45%, transparent);
    border-left-color: var(--primary);
  }

  /* Colored accent dot — color comes from the Tailwind class set dynamically */
  .audience-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    opacity: 0.85;
    flex-shrink: 0;
  }

  /* Users icon — left of dot, accent-tinted */
  :global(.audience-users-icon) {
    width: 13px;
    height: 13px;
    color: var(--primary);
    opacity: 0.8;
    flex-shrink: 0;
  }

  /* Audience name — bolder and slightly more prominent */
  .audience-name {
    font-weight: 600;
    color: var(--foreground);
    letter-spacing: 0.01em;
  }

  /* Lock icon — right end, very subtle until hovered or filtered */
  .audience-lock {
    margin-left: auto;
    padding-left: 8px;
    display: flex;
    align-items: center;
    opacity: 0.25;
    transition: opacity 0.15s ease;
  }

  .conditional-marker-pill.audience-open:hover .audience-lock {
    opacity: 0.55;
  }

  /* When filtered out: lock brightens as a hint */
  .audience-lock.lock-active {
    opacity: 0.65;
    color: var(--muted-foreground);
  }

  :global(.lock-icon) {
    width: 11px;
    height: 11px;
  }

  /* ── Filter-mismatch dimming ─────────────────────────────────────── */
  /*
   * Applied when templateContextStore.previewAudience is set to audience X
   * and this block's condition is NOT X.
   * Dims the entire marker pill; the Lock icon brightens (handled above).
   */
  .conditional-marker-pill.filtered-out {
    opacity: 0.45;
  }

  /* ── Standard if/else/close label + icon ────────────────────────── */
  .conditional-marker-icon {
    font-size: 0.8em;
    opacity: 0.6;
  }

  .conditional-marker-label {
    font-weight: 500;
  }

  /* ── Delete button ───────────────────────────────────────────────── */
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
