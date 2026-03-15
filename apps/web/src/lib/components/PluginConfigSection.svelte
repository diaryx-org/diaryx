<script lang="ts">
  import { ChevronRight, Puzzle } from "@lucide/svelte";
  import NestedObjectDisplay from "./NestedObjectDisplay.svelte";

  interface Props {
    plugins: Record<string, unknown>;
    onNavigateLink?: (link: string) => void;
  }

  let { plugins, onNavigateLink }: Props = $props();

  let collapsedPlugins = $state<Record<string, boolean>>({});

  function togglePlugin(name: string) {
    collapsedPlugins[name] = !(collapsedPlugins[name] ?? true);
  }

  function isCollapsed(name: string): boolean {
    return collapsedPlugins[name] ?? true;
  }

  function isObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  const pluginEntries = $derived(Object.entries(plugins));
</script>

{#if pluginEntries.length === 0}
  <p class="text-xs text-muted-foreground">No plugins configured</p>
{:else}
  <div class="space-y-2">
    {#each pluginEntries as [name, config]}
      <div>
        <button
          type="button"
          class="flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer hover:text-foreground w-full"
          onclick={() => togglePlugin(name)}
        >
          <ChevronRight class="size-4 md:size-3 transition-transform {isCollapsed(name) ? '' : 'rotate-90'}" />
          <Puzzle class="size-4 md:size-3" />
          <span class="font-medium">{name}</span>
        </button>
        {#if !isCollapsed(name)}
          <div class="mt-1 pl-5">
            {#if isObject(config)}
              <NestedObjectDisplay data={config} {onNavigateLink} />
            {:else if config === null || config === undefined}
              <span class="text-xs text-muted-foreground italic">No configuration</span>
            {:else}
              <span class="text-xs text-foreground">{String(config)}</span>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}
