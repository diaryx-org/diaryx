<script lang="ts">
  /**
   * SettingsDialog - Main settings dialog component
   *
   * Uses tabs to organize settings into categories.
   * Renders as a Drawer on mobile and Dialog on desktop.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Drawer from "$lib/components/ui/drawer";
  import { Settings } from "@lucide/svelte";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import { getAuthState } from "$lib/auth";
  import { getCurrentWorkspaceId, getLocalWorkspace } from "$lib/storage/localWorkspaceRegistry.svelte";

  // Import modular settings components
  import DisplaySettings from "./settings/DisplaySettings.svelte";
  import BillingSettings from "./settings/BillingSettings.svelte";
  import WorkspaceSettings from "./settings/WorkspaceSettings.svelte";
  import LinkSettings from "./settings/LinkSettings.svelte";
  import StorageSettings from "./settings/StorageSettings.svelte";
  import AccountSettings from "./settings/AccountSettings.svelte";
  import BackupSettings from "./settings/BackupSettings.svelte";
  import ImportSettings from "./settings/ImportSettings.svelte";
  import ClearDataSettings from "./settings/ClearDataSettings.svelte";
  import DebugInfo from "./settings/DebugInfo.svelte";
  import WorkspaceManagement from "./settings/WorkspaceManagement.svelte";
  import PluginSettingsTab from "./settings/PluginSettingsTab.svelte";
  import PluginIframe from "./components/PluginIframe.svelte";
  import { getLegacyBuiltinFields } from "./components/pluginBuiltinCompat";
  import UpgradeBanner from "$lib/components/UpgradeBanner.svelte";
  import { getPluginStore } from "../models/stores/pluginStore.svelte";
  import { getPlugin as getBrowserPlugin } from "$lib/plugins/browserPluginManager.svelte";
  import { runPluginUpdateConfigFlow } from "$lib/plugins/configUpdateFlow";
  import { mergeRuntimePluginConfig } from "$lib/plugins/pluginRuntimeConfig";
  import type { Api } from "$lib/backend/api";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

  interface Props {
    open?: boolean;
    focusMode?: boolean;
    workspacePath?: string | null;
    /** Tab to show when the dialog opens */
    initialTab?: string;
    /** Callback to open the sync setup wizard */
    onAddWorkspace?: () => void;
    /** API wrapper for plugin config operations */
    api?: Api | null;
    /** Handler for host actions from plugin iframes (e.g. OAuth) */
    onHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
  }

  let {
    open = $bindable(),
    focusMode = $bindable(true),
    workspacePath = null,
    initialTab,
    onAddWorkspace,
    api = null,
    onHostAction,
  }: Props = $props();

  const mobileState = getMobileState();
  let authState = $derived(getAuthState());

  // Current workspace info for per-workspace settings
  let currentWorkspaceId = $derived(getCurrentWorkspaceId() ?? '');
  let currentWorkspaceName = $derived(getLocalWorkspace(currentWorkspaceId)?.name ?? 'My Journal');

  // Plugin store — all plugin-contributed settings tabs are rendered dynamically.
  // Builtin ComponentRef mappings are reserved for host-owned plugin UIs (for example storage settings).
  const pluginStore = getPluginStore();
  const pluginSettingsTabs = $derived(pluginStore.settingsTabs);

  // Plugin config state: keyed by pluginId
  let pluginConfigs = $state<Record<string, Record<string, JsonValue>>>({});

  function isManagedMode(config: Record<string, JsonValue>): boolean {
    const mode = config.provider_mode;
    return typeof mode === "string" && mode.toLowerCase() === "managed";
  }

  function getServerHostname(): string | null {
    const serverUrl = authState.serverUrl;
    if (!serverUrl) return null;
    try {
      return new URL(serverUrl).hostname;
    } catch {
      return null;
    }
  }

  function getBuiltinFieldsForTab(tab: (typeof pluginSettingsTabs)[number]) {
    return tab.contribution.component?.type === "Builtin"
      ? getLegacyBuiltinFields(tab.contribution.component.component_id)
      : null;
  }

  async function loadPluginConfig(pluginId: string) {
    try {
      // Browser-loaded plugins store config via their own get/setConfig
      const browserPlugin = getBrowserPlugin(pluginId);
      if (browserPlugin) {
        const raw = await browserPlugin.getConfig();
        pluginConfigs = {
          ...pluginConfigs,
          [pluginId]: mergeRuntimePluginConfig(
            pluginId,
            ((raw as Record<string, JsonValue>) ?? {}),
          ),
        };
        return;
      }
      if (!api) return;
      const raw = await api.getPluginConfig(pluginId);
      pluginConfigs = {
        ...pluginConfigs,
        [pluginId]: mergeRuntimePluginConfig(
          pluginId,
          ((raw as Record<string, JsonValue>) ?? {}),
        ),
      };
    } catch {
      pluginConfigs = { ...pluginConfigs, [pluginId]: {} };
    }
  }

  async function handlePluginConfigChange(pluginId: string, key: string, value: JsonValue) {
    const current = pluginConfigs[pluginId] ?? {};
    const updated = { ...current, [key]: value };
    pluginConfigs = { ...pluginConfigs, [pluginId]: updated };
    try {
      // Browser-loaded plugins store config via their own get/setConfig
      const browserPlugin = getBrowserPlugin(pluginId);
      if (browserPlugin) {
        await browserPlugin.setConfig(updated as Record<string, unknown>);
      } else {
        if (!api) return;
        await api.setPluginConfig(pluginId, updated);
      }

      await runPluginUpdateConfigFlow({
        pluginId,
        api,
        workspacePath,
        params: {
          source: "plugin_config",
          config: updated,
          ...(pluginId === "diaryx.ai" && isManagedMode(updated) && getServerHostname()
            ? { server_hostname: getServerHostname() }
            : {}),
        },
      });
    } catch (e) {
      console.error(`[Settings] Failed to save plugin config for ${pluginId}:`, e);
    }
  }

  // Track active tab
  let activeTab = $state("general");

  // Eagerly load plugin config when a plugin settings tab is active.
  // This replaces the old {#await} pattern so the UI renders immediately.
  $effect(() => {
    const match = pluginSettingsTabs.find(
      (t) => activeTab === `plugin-${t.contribution.id}`,
    );
    if (match && !(match.pluginId in pluginConfigs)) {
      void loadPluginConfig(match.pluginId);
    }
  });

  const settingsTabs = $derived([
    { id: "general", label: "General" },
    { id: "workspace", label: "Workspace" },
    ...pluginSettingsTabs.map(t => ({ id: `plugin-${t.contribution.id}`, label: t.contribution.label })),
    { id: "account", label: "Account" },
    { id: "billing", label: "Billing" },
    { id: "data", label: "Data" },
    { id: "debug", label: "Debug" },
  ]);

  // Switch to initialTab when the dialog opens
  $effect(() => {
    if (open && initialTab) {
      activeTab = initialTab;
    }
  });
</script>

{#snippet settingsContent()}
  <div class="flex h-full min-h-0 flex-col">
    <!-- Content -->
    <div class="flex-1 min-h-0 overflow-y-auto pr-2" data-settings-scroll-container>
      {#if activeTab === "general"}
        <div class="space-y-4">
          <DisplaySettings bind:focusMode />
        </div>
      {:else if activeTab === "workspace"}
        <div class="space-y-4">
          <WorkspaceSettings workspaceRootIndex={workspacePath} />
          <LinkSettings workspaceRootIndex={workspacePath} />
          <StorageSettings workspaceId={currentWorkspaceId} workspaceName={currentWorkspaceName} />
        </div>
      {:else if activeTab === "account"}
        <div class="space-y-4">
          <AccountSettings {onAddWorkspace} />
          <WorkspaceManagement />
        </div>
      {:else if activeTab === "billing"}
        <div class="space-y-4">
          <BillingSettings />
        </div>
      {:else if activeTab === "data"}
        <div class="space-y-4">
          <BackupSettings {workspacePath} />
          <ImportSettings {workspacePath} />
          <ClearDataSettings />
        </div>
      {:else if activeTab === "debug"}
        <div class="space-y-4">
          <DebugInfo />
        </div>
      {:else}
        {#each pluginSettingsTabs as tab}
          {#if activeTab === `plugin-${tab.contribution.id}`}
            <div class="space-y-4">
              {#if tab.contribution.component?.type === "Iframe"}
                <div class="h-[320px]">
                  <PluginIframe
                    pluginId={tab.pluginId as unknown as string}
                    componentId={tab.contribution.component.component_id}
                    {api}
                    {onHostAction}
                  />
                </div>
              {:else if tab.contribution.fields.length > 0 || getBuiltinFieldsForTab(tab)}
                  {#if tab.pluginId === "diaryx.ai" && isManagedMode(pluginConfigs[tab.pluginId] ?? {}) && authState.tier !== "plus"}
                    <UpgradeBanner
                      feature="Managed AI"
                      description="Upgrade to Diaryx Plus to use managed AI without your own API key."
                    />
                  {/if}
                  <PluginSettingsTab
                    pluginId={tab.pluginId}
                    fields={tab.contribution.fields.length > 0 ? tab.contribution.fields : (getBuiltinFieldsForTab(tab) ?? [])}
                    config={pluginConfigs[tab.pluginId] ?? {}}
                    onConfigChange={(key, value) => handlePluginConfigChange(tab.pluginId, key, value)}
                    {api}
                    {onHostAction}
                  />
              {:else}
                <p class="text-sm text-muted-foreground">
                  No configurable settings for this plugin.
                </p>
              {/if}
            </div>
          {/if}
        {/each}
      {/if}
    </div>

    <!-- Bottom Tab Bar -->
    <div class="px-3 pt-1 pb-1 shrink-0">
      <div class="flex items-center gap-1 bg-muted rounded-md p-0.5 overflow-x-auto">
        {#each settingsTabs as tab (tab.id)}
          <button
            type="button"
            class="shrink-0 px-3 md:px-2 py-2.5 md:py-1.5 text-sm md:text-xs font-medium rounded transition-colors {activeTab === tab.id ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => (activeTab = tab.id)}
          >
            {tab.label}
          </button>
        {/each}
      </div>
    </div>
  </div>
{/snippet}

{#if mobileState.isMobile}
  <!-- Mobile: Use Drawer -->
  <Drawer.Root bind:open>
    <Drawer.Content>
      <div class="mx-auto flex h-[70vh] min-h-0 w-full max-w-md flex-col">
        <Drawer.Header class="shrink-0">
          <Drawer.Title class="flex items-center gap-2">
            <Settings class="size-5" />
            Settings
          </Drawer.Title>
          <Drawer.Description>
            Configure your workspace and preferences.
          </Drawer.Description>
        </Drawer.Header>
        <div class="flex min-h-0 flex-1 flex-col px-4 pb-[calc(env(safe-area-inset-bottom)+0.75rem)]">
          {@render settingsContent()}
        </div>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <!-- Desktop: Use Dialog -->
  <Dialog.Root bind:open>
    <Dialog.Content class="sm:max-w-[550px] h-[550px] overflow-hidden flex flex-col">
      <Dialog.Header>
        <Dialog.Title class="flex items-center gap-2">
          <Settings class="size-5" />
          Settings
        </Dialog.Title>
        <Dialog.Description>
          Configure your workspace and preferences.
        </Dialog.Description>
      </Dialog.Header>
      <div class="flex-1 min-h-0 flex flex-col py-4">
        {@render settingsContent()}
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
