<script lang="ts">
  /**
   * AddWorkspaceDialog - Workspace creation dialog
   *
   * Local-first: workspaces are always created locally, with an optional
   * provider dropdown to link to a sync provider (e.g., Diaryx Sync).
   *
   * Content Sources: Start fresh (with optional starter template),
   * Import from ZIP, Open existing folder.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import {
    Loader2,
    AlertCircle,
    Upload,
    Plus,
    FolderOpen,
    FolderTree,
    Cloud,
    CloudDownload,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import { untrack } from "svelte";
  import { getBackend, createApi } from "./backend";
  import { isTauri } from "$lib/backend/interface";
  import { authorizeWorkspacePath, pickAuthorizedWorkspaceFolder } from "$lib/backend/workspaceAccess";
  import { isTierLimitError } from "$lib/billing";
  import {
    getLocalWorkspaces,
    createLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import {
    isStorageTypeSupported,
    storeWorkspaceFileSystemHandle,
  } from "$lib/backend/storageType";
  import { switchWorkspace } from "$lib/workspace/switchWorkspace";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getProviderStatus,
    linkWorkspace,
    listRemoteWorkspaces,
    downloadWorkspace,
    type ProviderStatus,
    type RemoteWorkspace,
  } from "$lib/sync/workspaceProviderService";
  import {
    captureProviderPluginForTransfer,
    installCapturedProviderPlugin,
  } from "$lib/sync/browserProviderBootstrap";
  import {
    fetchStarterWorkspaceRegistry,
  } from "$lib/marketplace/starterWorkspaceRegistry";
  import type { StarterWorkspaceRegistryEntry, BundleRegistryEntry } from "$lib/marketplace/types";
  import {
    fetchStarterWorkspaceZip,
  } from "$lib/marketplace/starterWorkspaceApply";

  interface Props {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
    onComplete?: (appliedBundle?: BundleRegistryEntry | null) => void;
    /** Pre-selected bundle to apply after workspace creation (theme, plugins, etc.) */
    selectedBundle?: BundleRegistryEntry | null;
  }

  let {
    open = $bindable(false),
    onOpenChange,
    onComplete,
    selectedBundle = null,
  }: Props = $props();

  const pluginStore = getPluginStore();

  // ========================================================================
  // State
  // ========================================================================

  type ContentSource = 'import_zip' | 'start_fresh' | 'open_folder' | 'download_cloud';

  let newWorkspaceName = $state("");
  let contentSource = $state<ContentSource>('start_fresh');

  // Download from cloud state
  let cloudWorkspaces = $state<RemoteWorkspace[]>([]);
  let cloudLoading = $state(false);
  let selectedRemoteWorkspace = $state<RemoteWorkspace | null>(null);

  // Provider (sync) state
  let selectedProviderId = $state<string | null>(null);
  let providerStatus = $state<ProviderStatus | null>(null);
  let providerStatusLoading = $state(false);

  // Import from ZIP state
  let importZipFile = $state<File | null>(null);
  let importZipFileInput = $state<HTMLInputElement | null>(null);

  // Open folder state
  let selectedFolderPath = $state<string | null>(null);
  let selectedFolderHandle = $state<FileSystemDirectoryHandle | null>(null);
  let selectedFolderName = $state<string | null>(null);

  // Starter workspace state (shown within the "start_fresh" card)
  let starterWorkspaces = $state<StarterWorkspaceRegistryEntry[]>([]);
  let starterLoading = $state(false);
  let selectedStarterId = $state<string | null>(null); // null = empty/fresh

  // Tauri workspace path
  let workspacePath = $state('');
  let workspacePathWasPicked = $state(false);

  // Loading / progress state
  let isInitializing = $state(false);
  let importProgress = $state(0);
  let progressMessage = $state<string | null>(null);

  // Error state
  let error = $state<string | null>(null);

  // ========================================================================
  // Derived
  // ========================================================================

  let workspaceProviders = $derived(pluginStore.workspaceProviders);
  let showOpenFolder = $derived(isTauri() || isStorageTypeSupported('filesystem-access'));
  let selectedStarter = $derived(
    selectedStarterId
      ? starterWorkspaces.find((s) => s.id === selectedStarterId) ?? null
      : null,
  );

  // ========================================================================
  // Effects
  // ========================================================================

  // Tauri: compute default workspace path from name
  $effect(() => {
    if (isTauri() && newWorkspaceName.trim()) {
      untrack(async () => {
        try {
          const backend = await getBackend();
          const appPaths = backend.getAppPaths?.();
          const docDir = typeof appPaths?.document_dir === 'string' ? appPaths.document_dir : '';
          if (!workspacePath || (docDir && workspacePath.startsWith(docDir))) {
            workspacePath = docDir
              ? `${docDir}/${newWorkspaceName.trim()}`
              : newWorkspaceName.trim();
          }
        } catch {
          // Backend not ready yet
        }
      });
    }
  });

  // Initialize dialog state when opened
  $effect(() => {
    if (open) {
      untrack(() => initializeDialog());
    }
  });

  // Cleanup on destroy
  $effect(() => {
    return () => {
      // Nothing to clean up in the simplified version
    };
  });

  // ========================================================================
  // Functions
  // ========================================================================

  function initializeDialog() {
    error = null;
    isInitializing = false;
    importProgress = 0;
    progressMessage = null;
    workspacePath = '';
    workspacePathWasPicked = false;
    importZipFile = null;
    selectedFolderPath = null;
    selectedFolderHandle = null;
    selectedFolderName = null;
    selectedProviderId = null;
    providerStatus = null;
    cloudWorkspaces = [];
    selectedRemoteWorkspace = null;
    selectedStarterId = null;
    newWorkspaceName = getNextLocalWorkspaceName();
    contentSource = 'start_fresh';
    loadStarterWorkspaces();
  }

  function getNextLocalWorkspaceName(): string {
    const existingNames = new Set(
      getLocalWorkspaces().map(ws => ws.name.trim().toLowerCase()),
    );
    const base = "New Workspace";
    if (!existingNames.has(base.toLowerCase())) {
      return base;
    }
    let index = 2;
    while (existingNames.has(`${base} ${index}`.toLowerCase())) {
      index += 1;
    }
    return `${base} ${index}`;
  }

  async function loadStarterWorkspaces() {
    starterLoading = true;
    try {
      const registry = await fetchStarterWorkspaceRegistry();
      starterWorkspaces = registry.starters;
    } catch {
      starterWorkspaces = [];
    } finally {
      starterLoading = false;
    }
  }

  async function handleProviderChange(providerId: string | null) {
    selectedProviderId = providerId;
    if (providerId) {
      providerStatusLoading = true;
      providerStatus = { ready: false, message: "Checking provider..." };
      providerStatus = await getProviderStatus(providerId);
      providerStatusLoading = false;
    } else {
      providerStatus = null;
      providerStatusLoading = false;
    }
  }

  function getSubmitButtonText(): string {
    switch (contentSource) {
      case 'start_fresh': return selectedProviderId ? 'Create & Sync' : 'Create Workspace';
      case 'import_zip': return selectedProviderId ? 'Import & Sync' : 'Import Workspace';
      case 'open_folder': return selectedProviderId ? 'Open & Sync' : 'Open Workspace';
      case 'download_cloud': return 'Download Workspace';
    }
    return 'Create Workspace';
  }

  function isSubmitDisabled(): boolean {
    if (isInitializing) return true;
    if (providerStatusLoading) return true;
    if (selectedProviderId && providerStatus && !providerStatus.ready) return true;
    if (!newWorkspaceName.trim()) return true;
    if (contentSource === 'import_zip' && !importZipFile) return true;
    if (contentSource === 'open_folder' && !selectedFolderPath && !selectedFolderHandle) return true;
    if (contentSource === 'download_cloud' && !selectedRemoteWorkspace) return true;
    return false;
  }

  /** Open a native folder picker for workspace location (Tauri only). */
  async function browseFolder() {
    try {
      const folder = await pickAuthorizedWorkspaceFolder('Select Workspace Location');
      if (folder) {
        workspacePath = folder;
        workspacePathWasPicked = true;
      }
    } catch (e) {
      console.warn('[AddWorkspaceDialog] Browse folder error:', e);
    }
  }

  /** Open a folder picker for the "Open existing folder" content source. */
  async function openFolderPicker() {
    try {
      if (isTauri()) {
        const folder = await pickAuthorizedWorkspaceFolder('Open Existing Folder');
        if (folder) {
          selectedFolderPath = folder;
          const segments = folder.replace(/[/\\]+$/, '').split(/[/\\]/);
          selectedFolderName = segments[segments.length - 1] || 'Folder';
          newWorkspaceName = selectedFolderName;
          contentSource = 'open_folder';
        }
      } else {
        const handle = await (window as any).showDirectoryPicker();
        selectedFolderHandle = handle;
        selectedFolderName = handle.name;
        newWorkspaceName = handle.name;
        contentSource = 'open_folder';
      }
    } catch (e: any) {
      if (e?.name !== 'AbortError') {
        console.warn('[AddWorkspaceDialog] Folder picker error:', e);
      }
    }
  }

  async function resolveWorkspaceDirectoryForCreate(workspaceName: string): Promise<string | undefined> {
    if (!isTauri()) return undefined;

    const configuredPath = workspacePath.trim();
    if (configuredPath) return configuredPath;

    const trimmedName = workspaceName.trim();
    if (!trimmedName) return undefined;

    try {
      const backend = await getBackend();
      const appPaths = backend.getAppPaths?.();
      const docDir = typeof appPaths?.document_dir === 'string' ? appPaths.document_dir : '';
      if (docDir) return `${docDir}/${trimmedName}`;
    } catch (e) {
      console.warn("[AddWorkspace] Failed to resolve default workspace path:", e);
    }

    return trimmedName;
  }

  async function ensureRootIndexForCurrentWorkspace(
    workspaceName: string,
    workspaceDirOverride?: string,
  ): Promise<string | null> {
    const backend = await getBackend();
    const api = createApi(backend);
    const workspaceDir = workspaceDirOverride?.trim()
      || backend.getWorkspacePath().replace(/\/index\.md$/, '').replace(/\/README\.md$/, '');

    try {
      return await api.findRootIndex(workspaceDir);
    } catch {
      // Continue below and create a root index.
    }

    try {
      await api.createWorkspace(workspaceDir, workspaceName);
    } catch (e) {
      if (!(e instanceof Error && e.message.includes('already exists'))) {
        throw e;
      }
    }

    try {
      return await api.findRootIndex(workspaceDir);
    } catch {
      return null;
    }
  }

  /**
   * Unified initialization handler.
   *
   * 1. Create local workspace (always)
   * 2. Import content if needed (ZIP, folder, or starter)
   * 3. If provider selected, link to provider
   * 4. Switch to new workspace, close dialog
   */
  async function handleInitialize() {
    if (!newWorkspaceName.trim()) {
      error = "Please enter a workspace name";
      return;
    }
    if (contentSource === 'import_zip' && !importZipFile) {
      error = "Please select a ZIP file to import";
      return;
    }
    if (contentSource === 'open_folder' && !selectedFolderPath && !selectedFolderHandle) {
      error = "Please select a folder";
      return;
    }

    isInitializing = true;
    error = null;
    importProgress = 0;
    progressMessage = "Creating workspace...";

    try {
      const wsName = newWorkspaceName.trim();
      const capturedProviderPlugin = await captureProviderPluginForTransfer(
        selectedProviderId,
      );

      // Step 1: Create local workspace (or download from cloud)
      if (contentSource === 'download_cloud') {
        await handleDownloadCloud();
        toast.success("Workspace downloaded", { description: `"${wsName}" is ready.` });
        handleClose();
        onComplete?.(selectedBundle);
        return;
      } else if (contentSource === 'open_folder') {
        await handleOpenFolder(wsName);
      } else if (contentSource === 'import_zip') {
        await handleImportZip(wsName);
      } else {
        // start_fresh — may include a starter workspace
        await handleCreateFresh(wsName);
      }

      // Step 2: If provider selected, link to provider
      if (selectedProviderId) {
        await installCapturedProviderPlugin(
          selectedProviderId,
          capturedProviderPlugin,
        );
        const localWs = getLocalWorkspaces().find(w => w.name === wsName);
        if (localWs) {
          await linkWorkspace(
            selectedProviderId,
            { localId: localWs.id, name: wsName },
            (progress) => {
              importProgress = progress.percent;
              progressMessage = progress.message;
            },
          );
        }
      }

      toast.success(
        selectedProviderId ? "Workspace created and syncing" : "Workspace created",
        { description: `"${wsName}" is ready.` },
      );

      handleClose();
      onComplete?.(selectedBundle);
    } catch (e) {
      console.error("[AddWorkspace] Initialization error:", e);
      if (isTierLimitError(e)) {
        error = "Workspace sync limit reached. Free plans can sync one workspace; upgrade to sync more.";
      } else if (e instanceof Error) {
        error = e.message || "Unknown error";
      } else {
        error = String(e) || "Initialization failed";
      }
    } finally {
      isInitializing = false;
    }
  }

  async function handleCreateFresh(wsName: string) {
    let wsPath = await resolveWorkspaceDirectoryForCreate(wsName);
    if (isTauri() && workspacePathWasPicked && wsPath) {
      wsPath = await authorizeWorkspacePath(wsPath);
    }
    const localWs = createLocalWorkspace(wsName, undefined, wsPath);
    await switchWorkspace(localWs.id, localWs.name);

    // If a starter workspace is selected, apply its content by importing the
    // ZIP archive. The ZIP contains markdown files with proper part_of/contents
    // frontmatter already set, so no hierarchy recalculation is needed.
    // Skip ensureRootIndexForCurrentWorkspace — the starter provides its own
    // root index.
    if (selectedStarter) {
      progressMessage = "Downloading starter workspace...";
      const zipBlob = await fetchStarterWorkspaceZip(selectedStarter);
      const zipFile = new File([zipBlob], "starter.zip", { type: "application/zip" });

      progressMessage = "Importing starter content...";
      const backend = await getBackend();
      const workspaceDir = backend.getWorkspacePath()
        .replace(/\/index\.md$/, '')
        .replace(/\/README\.md$/, '');

      await backend.importFromZip(
        zipFile,
        workspaceDir,
        (uploaded, total) => {
          importProgress = total > 0
            ? Math.round((uploaded / total) * (selectedProviderId ? 10 : 100))
            : 0;
        },
      );
    } else {
      await ensureRootIndexForCurrentWorkspace(localWs.name, localWs.path);
    }

    importProgress = selectedProviderId ? 10 : 100;
    progressMessage = selectedProviderId ? "Local workspace created." : "Done.";
  }

  async function handleOpenFolder(wsName: string) {
    let localWs: ReturnType<typeof createLocalWorkspace>;

    if (isTauri()) {
      if (!selectedFolderPath) throw new Error("No folder selected");
      localWs = createLocalWorkspace(wsName, undefined, selectedFolderPath);
      await switchWorkspace(localWs.id, localWs.name);
    } else {
      if (!selectedFolderHandle) throw new Error("No folder selected");
      localWs = createLocalWorkspace(wsName, 'filesystem-access');
      await storeWorkspaceFileSystemHandle(localWs.id, selectedFolderHandle);
      await switchWorkspace(localWs.id, localWs.name);
    }

    await ensureRootIndexForCurrentWorkspace(wsName, localWs.path);

    importProgress = selectedProviderId ? 10 : 100;
    progressMessage = selectedProviderId ? "Folder opened." : "Done.";
  }

  async function handleImportZip(wsName: string) {
    if (!importZipFile) throw new Error("No ZIP file selected");
    const zipFile = importZipFile;

    let wsPath = await resolveWorkspaceDirectoryForCreate(wsName);
    if (isTauri() && workspacePathWasPicked && wsPath) {
      wsPath = await authorizeWorkspacePath(wsPath);
    }
    const localWs = createLocalWorkspace(wsName, undefined, wsPath);
    await switchWorkspace(localWs.id, localWs.name);

    const backend = await getBackend();
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    progressMessage = "Importing ZIP...";

    const result = await backend.importFromZip(
      zipFile,
      workspaceDir,
      (uploaded, total) => {
        importProgress = total > 0
          ? Math.round((uploaded / total) * (selectedProviderId ? 10 : 100))
          : 0;
      },
    );

    if (!result.success) {
      throw new Error(result.error || "Failed to import ZIP");
    }

    await ensureRootIndexForCurrentWorkspace(localWs.name, localWs.path);

    importProgress = selectedProviderId ? 10 : 100;
    progressMessage = selectedProviderId
      ? `Imported ${result.files_imported} files.`
      : "Done.";
  }

  async function loadCloudWorkspaces() {
    const provider = workspaceProviders[0];
    if (!provider) return;
    cloudLoading = true;
    try {
      cloudWorkspaces = await listRemoteWorkspaces(provider.contribution.id);
    } catch (e) {
      console.warn("[AddWorkspaceDialog] Failed to list cloud workspaces:", e);
      cloudWorkspaces = [];
    } finally {
      cloudLoading = false;
    }
  }

  async function handleDownloadCloud() {
    if (!selectedRemoteWorkspace) throw new Error("No remote workspace selected");
    const provider = workspaceProviders[0];
    if (!provider) throw new Error("No workspace provider available");

    progressMessage = `Downloading "${selectedRemoteWorkspace.name}"...`;

    const result = await downloadWorkspace(
      provider.contribution.id,
      {
        remoteId: selectedRemoteWorkspace.id,
        name: selectedRemoteWorkspace.name,
        link: true,
      },
      (progress) => {
        importProgress = progress.percent;
        progressMessage = progress.message;
      },
    );

    importProgress = 100;
    progressMessage = `Downloaded ${result.filesImported} files.`;
  }

  function handleClose() {
    open = false;
    onOpenChange?.(false);
  }
</script>

<Dialog.Root bind:open onOpenChange={(o) => onOpenChange?.(o)}>
  <Dialog.Content class="sm:max-w-[450px]">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        {#if isInitializing}
          <Loader2 class="size-5 animate-spin" />
          Adding Workspace
        {:else}
          <Plus class="size-5" />
          Add Workspace
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if isInitializing}
          {progressMessage ?? "Setting up..."}
        {:else}
          Create a workspace on this device.{#if workspaceProviders.length > 0} Optionally sync with a provider.{/if}
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="py-4 space-y-4 max-h-[60vh] overflow-y-auto">
      <!-- Error message -->
      {#if error}
        <div class="flex items-center gap-2 text-destructive text-sm p-3 bg-destructive/10 rounded-md">
          <AlertCircle class="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      {/if}

      {#if isInitializing}
        <!-- Progress bar during initialization -->
        <div class="space-y-3 py-4">
          <Progress value={importProgress} class="h-2" />
          <p class="text-xs text-muted-foreground text-center">
            {progressMessage ?? "Initializing workspace..."}
          </p>
        </div>
      {:else}
        <div class="space-y-4">
          <!-- Workspace Name -->
          <div class="space-y-2">
            <Label for="workspace-name" class="text-sm">Workspace Name</Label>
            <Input
              id="workspace-name"
              bind:value={newWorkspaceName}
              placeholder="My Workspace"
            />
          </div>

          <!-- Workspace Location (Tauri only, hidden for open_folder since folder IS the location) -->
          {#if isTauri() && contentSource !== 'open_folder'}
            <div class="space-y-2">
              <Label class="text-sm">Location</Label>
              <div class="flex gap-2">
                <Input bind:value={workspacePath} class="flex-1 font-mono text-xs" />
                <Button variant="outline" size="sm" onclick={browseFolder}>
                  <FolderOpen class="size-4 mr-1" />
                  Browse
                </Button>
              </div>
            </div>
          {/if}

          <!-- Sync Provider -->
          {#if workspaceProviders.length > 0}
            <div class="space-y-2">
              <Label class="text-sm">Sync</Label>
              <select
                class="w-full h-9 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                value={selectedProviderId ?? ""}
                onchange={(e) => void handleProviderChange((e.target as HTMLSelectElement).value || null)}
              >
                <option value="">None (local only)</option>
                {#each workspaceProviders as provider (provider.contribution.id)}
                  <option value={provider.contribution.id}>
                    {provider.contribution.label}
                  </option>
                {/each}
              </select>
              {#if selectedProviderId && providerStatus && !providerStatus.ready}
                <p class="text-xs text-muted-foreground">
                  {providerStatus.message ?? "Provider not ready."}
                  Configure in Settings.
                </p>
              {/if}
              <p class="text-xs text-muted-foreground">
                Free includes one synced workspace on up to two devices. Upgrade to Plus for more synced workspaces.
              </p>
            </div>
          {/if}

          <!-- Content Source -->
          <div class="space-y-2">
            <!-- Import from ZIP -->
            <input
              type="file"
              accept=".zip"
              class="hidden"
              bind:this={importZipFileInput}
              onchange={(e) => {
                const input = e.target as HTMLInputElement;
                const file = input.files?.[0];
                if (file) {
                  importZipFile = file;
                  contentSource = 'import_zip';
                  if (!newWorkspaceName.trim()) {
                    newWorkspaceName = getNextLocalWorkspaceName();
                  }
                }
                input.value = "";
              }}
            />
            <button
              type="button"
              class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'import_zip' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
              onclick={() => {
                if (importZipFile) {
                  contentSource = 'import_zip';
                } else {
                  importZipFileInput?.click();
                }
              }}
            >
              <div class="flex items-start gap-3">
                <div class="mt-0.5">
                  <Upload class="size-5 {contentSource === 'import_zip' ? 'text-primary' : 'text-muted-foreground'}" />
                </div>
                <div>
                  <div class="font-medium text-sm">
                    {#if importZipFile}
                      Import "{importZipFile.name}"
                    {:else}
                      Import from ZIP
                    {/if}
                  </div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    {#if importZipFile}
                      <!-- svelte-ignore node_invalid_placement_ssr -->
                      <span
                        role="button"
                        tabindex="0"
                        class="text-primary hover:underline cursor-pointer"
                        onclick={(e: MouseEvent) => { e.stopPropagation(); importZipFileInput?.click(); }}
                        onkeydown={(e: KeyboardEvent) => { if (e.key === 'Enter') { e.stopPropagation(); importZipFileInput?.click(); } }}
                      >Change file</span>
                    {:else}
                      Import a workspace from a ZIP backup
                    {/if}
                  </div>
                </div>
              </div>
            </button>

            <!-- Open existing folder -->
            {#if showOpenFolder}
              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'open_folder' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => {
                  if (selectedFolderName) {
                    contentSource = 'open_folder';
                  } else {
                    openFolderPicker();
                  }
                }}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <FolderOpen class="size-5 {contentSource === 'open_folder' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">
                      {#if selectedFolderName}
                        Open "{selectedFolderName}"
                      {:else}
                        Open existing folder
                      {/if}
                    </div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      {#if selectedFolderName}
                        <!-- svelte-ignore node_invalid_placement_ssr -->
                        <span
                          role="button"
                          tabindex="0"
                          class="text-primary hover:underline cursor-pointer"
                          onclick={(e: MouseEvent) => { e.stopPropagation(); openFolderPicker(); }}
                          onkeydown={(e: KeyboardEvent) => { if (e.key === 'Enter') { e.stopPropagation(); openFolderPicker(); } }}
                        >Change folder</span>
                      {:else}
                        Use an existing folder of markdown files
                      {/if}
                    </div>
                  </div>
                </div>
              </button>
            {/if}

            <!-- Download from Cloud -->
            {#if workspaceProviders.length > 0}
              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'download_cloud' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => {
                  contentSource = 'download_cloud';
                  if (cloudWorkspaces.length === 0) loadCloudWorkspaces();
                }}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <CloudDownload class="size-5 {contentSource === 'download_cloud' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">Download from Cloud</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Download a workspace synced from another device
                    </div>
                  </div>
                </div>
              </button>

              {#if contentSource === 'download_cloud'}
                <div class="space-y-2 pl-8">
                  {#if cloudLoading}
                    <div class="flex items-center gap-2 text-sm text-muted-foreground py-2">
                      <Loader2 class="size-4 animate-spin" />
                      Loading cloud workspaces...
                    </div>
                  {:else if cloudWorkspaces.length === 0}
                    <p class="text-xs text-muted-foreground py-2">
                      No cloud workspaces found.
                    </p>
                  {:else}
                    {#each cloudWorkspaces as remote (remote.id)}
                      <button
                        type="button"
                        class="w-full text-left p-2 rounded-md border transition-colors {selectedRemoteWorkspace?.id === remote.id ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                        onclick={() => {
                          selectedRemoteWorkspace = remote;
                          newWorkspaceName = remote.name;
                        }}
                      >
                        <div class="flex items-center gap-2">
                          <Cloud class="size-3.5 text-muted-foreground shrink-0" />
                          <span class="text-sm truncate">{remote.name}</span>
                        </div>
                      </button>
                    {/each}
                  {/if}
                </div>
              {/if}
            {/if}

            <!-- Start fresh (with starter workspace selector) -->
            <button
              type="button"
              class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'start_fresh' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
              onclick={() => {
                contentSource = 'start_fresh';
                newWorkspaceName = getNextLocalWorkspaceName();
              }}
            >
              <div class="flex items-start gap-3">
                <div class="mt-0.5">
                  <Plus class="size-5 {contentSource === 'start_fresh' ? 'text-primary' : 'text-muted-foreground'}" />
                </div>
                <div>
                  <div class="font-medium text-sm">
                    {#if selectedStarter}
                      Start from "{selectedStarter.name}"
                    {:else}
                      Start fresh
                    {/if}
                  </div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    {#if selectedStarter}
                      {selectedStarter.summary}
                    {:else}
                      Create an empty workspace with a root index file
                    {/if}
                  </div>
                </div>
              </div>
            </button>

            {#if contentSource === 'start_fresh' && (starterWorkspaces.length > 0 || starterLoading)}
              <div class="space-y-1.5 pl-8">
                <p class="text-xs text-muted-foreground font-medium">Workspace starter</p>
                {#if starterLoading}
                  <div class="flex items-center gap-2 text-xs text-muted-foreground py-1">
                    <Loader2 class="size-3.5 animate-spin" />
                    Loading starters...
                  </div>
                {:else}
                  <!-- Empty / fresh option -->
                  <button
                    type="button"
                    class="w-full text-left p-2 rounded-md border transition-colors {selectedStarterId === null ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                    onclick={() => { selectedStarterId = null; }}
                  >
                    <div class="flex items-center gap-2">
                      <Plus class="size-3.5 text-muted-foreground shrink-0" />
                      <div>
                        <span class="text-sm">Empty workspace</span>
                        <p class="text-xs text-muted-foreground">Just a root index file</p>
                      </div>
                    </div>
                  </button>

                  <!-- Starter options from registry -->
                  {#each starterWorkspaces as starter (starter.id)}
                    <button
                      type="button"
                      class="w-full text-left p-2 rounded-md border transition-colors {selectedStarterId === starter.id ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                      onclick={() => { selectedStarterId = starter.id; }}
                    >
                      <div class="flex items-center gap-2">
                        <FolderTree class="size-3.5 text-muted-foreground shrink-0" />
                        <div class="min-w-0">
                          <span class="text-sm truncate block">{starter.name}</span>
                          <p class="text-xs text-muted-foreground truncate">
                            {starter.summary}
                            <span class="text-muted-foreground/60">
                              &middot; {starter.file_count} file{starter.file_count === 1 ? '' : 's'}
                            </span>
                          </p>
                        </div>
                      </div>
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>

    <!-- Footer -->
    <div class="flex justify-end pt-4 border-t">
      {#if !isInitializing}
        <Button onclick={handleInitialize} disabled={isSubmitDisabled()}>
          {getSubmitButtonText()}
          {#if selectedProviderId}
            <Cloud class="size-4 ml-1" />
          {/if}
        </Button>
      {:else}
        <div></div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
