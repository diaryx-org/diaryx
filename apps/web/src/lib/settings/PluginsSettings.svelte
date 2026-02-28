<script lang="ts">
  import { Puzzle } from "@lucide/svelte";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getBrowserPluginSupport,
    getBrowserPluginSupportError,
  } from "$lib/plugins/browserPluginManager.svelte";

  const pluginStore = getPluginStore();

  const plugins = $derived.by(() =>
    [...pluginStore.allManifests].sort((a, b) => {
      const aName = String(a.name ?? a.id);
      const bName = String(b.name ?? b.id);
      return aName.localeCompare(bName);
    }),
  );

  function isEnabled(pluginId: string): boolean {
    return pluginStore.isPluginEnabled(pluginId);
  }

  function setEnabled(pluginId: string, enabled: boolean): void {
    pluginStore.setPluginEnabled(pluginId, enabled);
  }

  const browserPluginSupport = $derived(getBrowserPluginSupport());
  const browserPluginSupportError = $derived(getBrowserPluginSupportError());
</script>

<div class="space-y-4">
  <h3 class="font-medium flex items-center gap-2">
    <Puzzle class="size-4" />
    Plugins
  </h3>

  {#if !browserPluginSupport.supported}
    <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2">
      <p class="text-xs text-amber-700 dark:text-amber-300">
        {browserPluginSupportError ?? browserPluginSupport.reason ?? 'Browser plugins are unavailable in this browser.'}
      </p>
    </div>
  {/if}

  {#if plugins.length === 0}
    <p class="text-sm text-muted-foreground px-1">No plugins are installed.</p>
  {:else}
    <div class="space-y-3">
      {#each plugins as plugin}
        {@const pluginId = String(plugin.id)}
        <div class="flex items-center justify-between gap-4 px-1">
          <Label for={`plugin-enabled-${pluginId}`} class="text-sm cursor-pointer flex flex-col gap-0.5">
            <span>{plugin.name}</span>
            <span class="font-normal text-xs text-muted-foreground">
              {plugin.description || pluginId}
            </span>
          </Label>
          <Switch
            id={`plugin-enabled-${pluginId}`}
            checked={isEnabled(pluginId)}
            onCheckedChange={(checked) => setEnabled(pluginId, checked)}
            disabled={!browserPluginSupport.supported}
          />
        </div>
      {/each}
    </div>
  {/if}
</div>
