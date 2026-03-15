<script lang="ts">
  import { ChevronRight, ArrowUpRight } from "@lucide/svelte";
  import { parseLinkDisplay } from "$lib/utils/linkParser";
  import Self from "./NestedObjectDisplay.svelte";

  interface Props {
    data: Record<string, unknown>;
    onNavigateLink?: (link: string) => void;
    depth?: number;
  }

  let { data, onNavigateLink, depth = 0 }: Props = $props();

  // Track collapsed state per key
  let collapsedKeys = $state<Record<string, boolean>>({});

  function toggleKey(key: string) {
    collapsedKeys[key] = !(collapsedKeys[key] ?? true);
  }

  function isCollapsed(key: string): boolean {
    return collapsedKeys[key] ?? true;
  }

  function isObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function formatKey(key: string): string {
    return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }
</script>

<div class="space-y-1 {depth > 0 ? 'pl-3 border-l border-border' : ''}">
  {#each Object.entries(data) as [key, value]}
    {#if isObject(value)}
      <!-- Nested object: collapsible group -->
      <button
        type="button"
        class="flex items-center gap-1 text-xs text-muted-foreground cursor-pointer hover:text-foreground w-full"
        onclick={() => toggleKey(key)}
      >
        <ChevronRight class="size-4 md:size-3 transition-transform {isCollapsed(key) ? '' : 'rotate-90'}" />
        <span class="font-medium">{formatKey(key)}</span>
      </button>
      {#if !isCollapsed(key)}
        <Self data={value} {onNavigateLink} depth={depth + 1} />
      {/if}
    {:else if Array.isArray(value)}
      <!-- Array: inline badges -->
      <div class="space-y-0.5">
        <span class="text-xs text-muted-foreground font-medium">{formatKey(key)}</span>
        <div class="flex flex-wrap gap-1">
          {#each value as item}
            <span class="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground">
              {String(item)}
            </span>
          {/each}
        </div>
      </div>
    {:else}
      <!-- Scalar value -->
      {@const linkParsed = typeof value === "string" ? parseLinkDisplay(value) : null}
      <div class="flex items-start gap-1 text-xs">
        <span class="text-muted-foreground font-medium shrink-0">{formatKey(key)}:</span>
        {#if linkParsed && onNavigateLink}
          <button
            type="button"
            class="inline-flex items-center gap-0.5 text-foreground hover:underline cursor-pointer"
            onclick={() => onNavigateLink?.(value as string)}
            title={linkParsed.path}
          >
            <ArrowUpRight class="size-4 md:size-3" />
            {linkParsed.title || linkParsed.path}
          </button>
        {:else if typeof value === "boolean"}
          <span class="text-foreground">{value ? "Yes" : "No"}</span>
        {:else if value === null || value === undefined}
          <span class="text-muted-foreground italic">null</span>
        {:else}
          <span class="text-foreground">{String(value)}</span>
        {/if}
      </div>
    {/if}
  {/each}
</div>
