<script lang="ts">
  /**
   * PluginSidebarPanel - Renders a plugin-contributed sidebar tab.
   *
   * For Builtin component refs, renders the matching built-in component.
   * For Declarative component refs, renders a form using PluginSettingsTab.
   * For Iframe component refs, renders plugin HTML in a sandboxed iframe.
   */
  import type { ComponentRef, PluginId } from "$lib/backend/generated";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
  import type { Api } from "$lib/backend/api";
  import type { EntryData } from "$lib/backend/interface";
  import PluginSettingsTab from "$lib/settings/PluginSettingsTab.svelte";
  import PluginIframe from "$lib/components/PluginIframe.svelte";
  import { getPlugin as getBrowserPlugin } from "$lib/plugins/browserPluginManager.svelte";

  interface Props {
    pluginId: PluginId;
    component: ComponentRef;
    api: Api;
    entry?: EntryData | null;
    onHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
  }

  let { pluginId, component, api, entry = null, onHostAction }: Props = $props();

  let config = $state<Record<string, JsonValue>>({});

  $effect(() => {
    // Only load config for Declarative components that actually use it.
    // Iframe components communicate via postMessage; loading config here
    // would race with the iframe's handle_command calls on the same WASM instance.
    if (component.type !== "Iframe") {
      loadConfig();
    }
  });

  async function loadConfig() {
    try {
      const browserPlugin = getBrowserPlugin(pluginId as unknown as string);
      if (browserPlugin) {
        const raw = await browserPlugin.getConfig();
        config = (raw as Record<string, JsonValue>) ?? {};
        return;
      }
      const raw = await api.getPluginConfig(pluginId);
      config = (raw as Record<string, JsonValue>) ?? {};
    } catch {
      config = {};
    }
  }

  async function handleConfigChange(key: string, value: JsonValue) {
    config = { ...config, [key]: value };
    try {
      const browserPlugin = getBrowserPlugin(pluginId as unknown as string);
      if (browserPlugin) {
        await browserPlugin.setConfig(config as Record<string, unknown>);
        return;
      }
      await api.setPluginConfig(pluginId, config);
    } catch (e) {
      console.error(`[PluginSidebarPanel] Failed to save config for ${pluginId}:`, e);
    }
  }
</script>

{#if component.type === "Iframe"}
  <PluginIframe
    pluginId={pluginId as unknown as string}
    componentId={component.component_id}
    {entry}
    {onHostAction}
  />
{:else}
  <div class="p-4 space-y-4">
    {#if component.type === "Declarative"}
      <PluginSettingsTab
        pluginId={pluginId as unknown as string}
        fields={component.fields}
        {config}
        onConfigChange={handleConfigChange}
      />
    {:else if component.type === "Builtin"}
      <p class="text-sm text-muted-foreground">
        Built-in component: {component.component_id}
      </p>
    {/if}
  </div>
{/if}
