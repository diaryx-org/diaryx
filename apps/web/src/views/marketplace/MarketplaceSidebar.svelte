<script lang="ts">
  import MarketplaceColors from "./MarketplaceColors.svelte";
  import MarketplaceTypography from "./MarketplaceTypography.svelte";
  import MarketplacePlugins from "./MarketplacePlugins.svelte";
  import MarketplaceBundles from "./MarketplaceBundles.svelte";

  type Section = "colors" | "typography" | "plugins" | "bundles";
  let activeSection = $state<Section>("colors");

  const sections: { id: Section; label: string }[] = [
    { id: "colors", label: "Colors" },
    { id: "typography", label: "Typography" },
    { id: "plugins", label: "Plugins" },
    { id: "bundles", label: "Bundles" },
  ];
</script>

<div class="flex flex-col h-full">
  <!-- Content -->
  <div class="flex-1 min-h-0 overflow-y-auto">
    {#if activeSection === "colors"}
      <MarketplaceColors />
    {:else if activeSection === "typography"}
      <MarketplaceTypography />
    {:else if activeSection === "plugins"}
      <MarketplacePlugins />
    {:else if activeSection === "bundles"}
      <MarketplaceBundles />
    {/if}
  </div>

  <!-- Section Tabs -->
  <div class="px-3 pt-1 pb-1 shrink-0">
    <div class="flex items-center gap-1 bg-muted rounded-md p-0.5">
      {#each sections as section (section.id)}
        <button
          type="button"
          class="flex-1 px-2 py-1.5 text-xs font-medium rounded transition-colors {activeSection === section.id ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => (activeSection = section.id)}
        >
          {section.label}
        </button>
      {/each}
    </div>
  </div>
</div>
