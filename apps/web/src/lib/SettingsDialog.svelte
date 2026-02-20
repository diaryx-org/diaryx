<script lang="ts">
  /**
   * SettingsDialog - Main settings dialog component
   *
   * Uses tabs to organize settings into categories.
   * Renders as a Drawer on mobile and Dialog on desktop.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Drawer from "$lib/components/ui/drawer";
  import * as Tabs from "$lib/components/ui/tabs";
  import { Button } from "$lib/components/ui/button";
  import { Settings, Eye, FolderOpen, FileText, RefreshCw, Database, Bug, User, CreditCard } from "@lucide/svelte";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import { getAuthState } from "$lib/auth";
  import { getCurrentWorkspaceId, getLocalWorkspace } from "$lib/storage/localWorkspaceRegistry.svelte";

  // Import modular settings components
  import DisplaySettings from "./settings/DisplaySettings.svelte";
  import BillingSettings from "./settings/BillingSettings.svelte";
  import FormattingSettings from "./settings/FormattingSettings.svelte";
  import WorkspaceSettings from "./settings/WorkspaceSettings.svelte";
  import LinkSettings from "./settings/LinkSettings.svelte";
  import StorageSettings from "./settings/StorageSettings.svelte";
  import SyncSettings from "./settings/SyncSettings.svelte";
  import AccountSettings from "./settings/AccountSettings.svelte";
  import BackupSettings from "./settings/BackupSettings.svelte";
  import ImportSettings from "./settings/ImportSettings.svelte";
  import FormatImportSettings from "./settings/FormatImportSettings.svelte";
  import CloudBackupSettings from "./settings/CloudBackupSettings.svelte";
  import ClearDataSettings from "./settings/ClearDataSettings.svelte";
  import DebugInfo from "./settings/DebugInfo.svelte";
  import TemplateSettings from "./settings/TemplateSettings.svelte";
  import WorkspaceManagement from "./settings/WorkspaceManagement.svelte";

  interface Props {
    open?: boolean;
    showUnlinkedFiles?: boolean;
    showHiddenFiles?: boolean;
    showEditorTitle?: boolean;
    showEditorPath?: boolean;
    readableLineLength?: boolean;
    focusMode?: boolean;
    workspacePath?: string | null;
    /** Tab to show when the dialog opens */
    initialTab?: string;
    /** Callback to open the sync setup wizard */
    onAddWorkspace?: () => void;
  }

  let {
    open = $bindable(),
    showUnlinkedFiles = $bindable(),
    showHiddenFiles = $bindable(false),
    showEditorTitle = $bindable(false),
    showEditorPath = $bindable(false),
    readableLineLength = $bindable(true),
    focusMode = $bindable(true),
    workspacePath = null,
    initialTab,
    onAddWorkspace,
  }: Props = $props();

  const mobileState = getMobileState();
  let authState = $derived(getAuthState());

  // Current workspace info for per-workspace settings
  let currentWorkspaceId = $derived(getCurrentWorkspaceId() ?? '');
  let currentWorkspaceName = $derived(getLocalWorkspace(currentWorkspaceId)?.name ?? 'My Journal');

  // Track active tab
  let activeTab = $state("general");

  // Switch to initialTab when the dialog opens
  $effect(() => {
    if (open && initialTab) {
      activeTab = initialTab;
    }
  });
</script>

{#snippet settingsContent()}
  <Tabs.Root bind:value={activeTab} class="w-full">
    <Tabs.List class="w-full flex gap-1 overflow-x-auto mb-4">
      <Tabs.Trigger value="general" class="shrink-0">
        <Eye class="size-4 mr-1.5 hidden sm:inline" />
        General
      </Tabs.Trigger>
      <Tabs.Trigger value="workspace" class="shrink-0">
        <FolderOpen class="size-4 mr-1.5 hidden sm:inline" />
        Workspace
      </Tabs.Trigger>
      <Tabs.Trigger value="templates" class="shrink-0">
        <FileText class="size-4 mr-1.5 hidden sm:inline" />
        Templates
      </Tabs.Trigger>
      <Tabs.Trigger value="sync" class="shrink-0">
        <RefreshCw class="size-4 mr-1.5 hidden sm:inline" />
        Sync
        <span class="text-[9px] font-semibold uppercase ml-1 px-1 py-0.5 rounded-full bg-blue-500/15 text-blue-600 dark:text-blue-400">Beta</span>
      </Tabs.Trigger>
      <Tabs.Trigger value="account" class="shrink-0">
        <User class="size-4 mr-1.5 hidden sm:inline" />
        Account
      </Tabs.Trigger>
      {#if authState.isAuthenticated}
        <Tabs.Trigger value="billing" class="shrink-0">
          <CreditCard class="size-4 mr-1.5 hidden sm:inline" />
          Billing
        </Tabs.Trigger>
      {/if}
      <Tabs.Trigger value="data" class="shrink-0">
        <Database class="size-4 mr-1.5 hidden sm:inline" />
        Data
      </Tabs.Trigger>
      <Tabs.Trigger value="debug" class="shrink-0">
        <Bug class="size-4 mr-1.5 hidden sm:inline" />
        Debug
      </Tabs.Trigger>
    </Tabs.List>

    <Tabs.Content value="general">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <DisplaySettings
          bind:showUnlinkedFiles
          bind:showHiddenFiles
          bind:showEditorTitle
          bind:showEditorPath
          bind:readableLineLength
          bind:focusMode
        />
        <FormattingSettings />
      </div>
    </Tabs.Content>

    <Tabs.Content value="workspace">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <WorkspaceSettings workspaceRootIndex={workspacePath} />
        <LinkSettings workspaceRootIndex={workspacePath} />
        <StorageSettings workspaceId={currentWorkspaceId} workspaceName={currentWorkspaceName} />
      </div>
    </Tabs.Content>

    <Tabs.Content value="templates">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <TemplateSettings workspaceRootIndex={workspacePath} />
      </div>
    </Tabs.Content>

    <Tabs.Content value="sync">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <SyncSettings {onAddWorkspace} />
      </div>
    </Tabs.Content>

    <Tabs.Content value="account">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <AccountSettings {onAddWorkspace} />
        <WorkspaceManagement />
      </div>
    </Tabs.Content>

    <Tabs.Content value="billing">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <BillingSettings />
      </div>
    </Tabs.Content>

    <Tabs.Content value="data">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <BackupSettings {workspacePath} />
        <ImportSettings {workspacePath} />
        <FormatImportSettings {workspacePath} />
        <CloudBackupSettings {workspacePath} />
        <ClearDataSettings />
      </div>
    </Tabs.Content>

    <Tabs.Content value="debug">
      <div class="space-y-4 h-[350px] overflow-y-auto pr-2">
        <DebugInfo />
      </div>
    </Tabs.Content>
  </Tabs.Root>

  <div class="flex justify-end pt-4 border-t mt-4">
    <Button variant="outline" onclick={() => (open = false)}>Close</Button>
  </div>
{/snippet}

{#if mobileState.isMobile}
  <!-- Mobile: Use Drawer -->
  <Drawer.Root bind:open>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md">
        <Drawer.Header>
          <Drawer.Title class="flex items-center gap-2">
            <Settings class="size-5" />
            Settings
          </Drawer.Title>
          <Drawer.Description>
            Configure your workspace and preferences.
          </Drawer.Description>
        </Drawer.Header>
        <div class="px-4 pb-8 overflow-y-auto max-h-[70vh]">
          {@render settingsContent()}
        </div>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <!-- Desktop: Use Dialog -->
  <Dialog.Root bind:open>
    <Dialog.Content class="sm:max-w-[550px] h-[550px] overflow-hidden">
      <Dialog.Header>
        <Dialog.Title class="flex items-center gap-2">
          <Settings class="size-5" />
          Settings
        </Dialog.Title>
        <Dialog.Description>
          Configure your workspace and preferences.
        </Dialog.Description>
      </Dialog.Header>
      <div class="py-4 overflow-hidden">
        {@render settingsContent()}
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
