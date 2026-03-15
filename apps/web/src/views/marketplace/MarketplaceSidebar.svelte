<script lang="ts">
  import MarketplaceThemes from "./MarketplaceThemes.svelte";
  import MarketplaceTypography from "./MarketplaceTypography.svelte";
  import MarketplacePlugins from "./MarketplacePlugins.svelte";
  import MarketplaceBundles from "./MarketplaceBundles.svelte";
  import MarketplaceTemplates from "./MarketplaceTemplates.svelte";
  import MarketplaceStarters from "./MarketplaceStarters.svelte";

  type Section = "themes" | "typography" | "plugins" | "bundles" | "templates" | "starters";
  let activeSection = $state<Section>("themes");

  const sections: { id: Section; label: string }[] = [
    { id: "themes", label: "Themes" },
    { id: "typography", label: "Type" },
    { id: "plugins", label: "Plugins" },
    { id: "bundles", label: "Bundles" },
    { id: "templates", label: "Templates" },
    { id: "starters", label: "Starters" },
  ];
</script>

<div class="flex flex-col h-full">
  <!-- Content -->
  <div class="flex-1 min-h-0 overflow-y-auto">
    {#if activeSection === "themes"}
      <MarketplaceThemes />
    {:else if activeSection === "typography"}
      <MarketplaceTypography />
    {:else if activeSection === "plugins"}
      <MarketplacePlugins />
    {:else if activeSection === "bundles"}
      <MarketplaceBundles />
    {:else if activeSection === "templates"}
      <MarketplaceTemplates />
    {:else if activeSection === "starters"}
      <MarketplaceStarters />
    {/if}
  </div>

  <!-- Section Tabs -->
  <div class="px-3 pt-1 pb-1 shrink-0">
    <div class="flex items-center gap-1 bg-muted rounded-md p-0.5 overflow-x-auto">
      {#each sections as section (section.id)}
        <button
          type="button"
          class="shrink-0 px-2 md:px-1 py-2.5 md:py-1.5 text-sm md:text-xs font-medium rounded transition-colors {activeSection === section.id ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => (activeSection = section.id)}
        >
          {section.label}
        </button>
      {/each}
    </div>
  </div>
</div>
