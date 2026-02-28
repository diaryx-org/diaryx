<script lang="ts">
  /**
   * PluginSidebarPanel - Renders a plugin-contributed sidebar tab.
   *
   * For Builtin component refs, renders the matching built-in component.
   * For Declarative component refs, renders a form using PluginSettingsTab.
   */
  import type { ComponentRef, PluginId } from "$lib/backend/generated";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
  import type { Api } from "$lib/backend/api";
  import PluginSettingsTab from "$lib/settings/PluginSettingsTab.svelte";

  interface Props {
    pluginId: PluginId;
    component: ComponentRef;
    api: Api;
  }

  let { pluginId, component, api }: Props = $props();

  let config = $state<Record<string, JsonValue>>({});

  $effect(() => {
    loadConfig();
  });

  async function loadConfig() {
    try {
      const raw = await api.getPluginConfig(pluginId);
      config = (raw as Record<string, JsonValue>) ?? {};
    } catch {
      config = {};
    }
  }

  async function handleConfigChange(key: string, value: JsonValue) {
    config = { ...config, [key]: value };
    try {
      await api.setPluginConfig(pluginId, config);
    } catch (e) {
      console.error(`[PluginSidebarPanel] Failed to save config for ${pluginId}:`, e);
    }
  }
</script>

<div class="p-4 space-y-4">
  {#if component.type === "Declarative"}
    <PluginSettingsTab
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
