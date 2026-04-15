<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, isTauri, isNativePluginBackend, replaceBackend, resetBackend, type TreeNode } from "./lib/backend";
  import { FsaGestureRequiredError } from "./lib/backend/fsaErrors";
  import { BackendError } from "./lib/backend/interface";
  import { pickAuthorizedWorkspaceFolder } from "./lib/backend/workspaceAccess";
  import { maybeStartWindowDrag } from "$lib/windowDrag";
  import * as browserPlugins from "$lib/plugins/browserPluginManager.svelte";
  import { switchWorkspace } from "$lib/workspace/switchWorkspace";
  import { getWorkspaceDirectoryPath } from "$lib/workspace/rootPath";
  import { installLocalPlugin } from "$lib/plugins/pluginInstallService";
  import { addFilesToZip } from "./lib/settings/zipUtils";
  import { useMobileGestures } from "$lib/hooks/useMobileGestures.svelte";
  import { createApi, type Api } from "./lib/backend/api";
  import type { JsonValue } from "./lib/backend/generated/serde_json/JsonValue";
  import { isIOS } from "$lib/hooks/useMobile.svelte";
  import { openOauthWindow } from "$lib/plugins/oauthWindow";
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import MarketplaceDialog from "./lib/MarketplaceDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import DeviceReplacementDialog from "./lib/components/DeviceReplacementDialog.svelte";
  import ImagePreviewDialog from "./lib/ImagePreviewDialog.svelte";
  import MoveEntryDialog from "./lib/MoveEntryDialog.svelte";
  import PermissionBanner from "./lib/components/PermissionBanner.svelte";
  import AudienceEditor from "./lib/components/AudienceEditor.svelte";
  import AudiencePanel from "./views/audience/AudiencePanel.svelte";
  import { getAudiencePanelStore } from "$lib/stores/audiencePanelStore.svelte";
  import MarkdownPreviewDialog from "./lib/MarkdownPreviewDialog.svelte";
  import EditorFooter from "./views/editor/EditorFooter.svelte";
  import EditorEmptyState from "./views/editor/EditorEmptyState.svelte";
  import WelcomeScreen from "./views/WelcomeScreen.svelte";
  import type { BundleSelectInfo } from "./views/BundleCarousel.svelte";
  import EditorContent from "./views/editor/EditorContent.svelte";
  import FindBar from "$lib/components/FindBar.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { PanelRight, Menu, Loader2, Settings, Store, Cloud, CloudOff, Eye, CircleUser } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import {
    handleStandardPluginHostUiAction,
    isStandardPluginHostUiAction,
  } from "$lib/plugins/pluginHostUiActions";
  // Note: Button, icons, and LoadingSpinner are now only used in extracted view components

  // Import stores
  import {
    entryStore,
    uiStore,
    collaborationStore,
    workspaceStore,
    permissionStore,
    getThemeStore,
  } from "./models/stores";
  import { getPluginStore } from "./models/stores/pluginStore.svelte";
  import { getAudienceColorStore } from "./lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "./lib/utils/audienceDotColor";
  import type {
    PluginConfig,
    PluginPermissions,
  } from "./models/stores/permissionStore.svelte";
  import { getTemplateContextStore } from "./lib/stores/templateContextStore.svelte";
  import { buildCommandRegistry } from "$lib/commandRegistry";
  import { getAppearanceStore } from "./lib/stores/appearance.svelte";
  import { mirrorCurrentWorkspaceMutationToLinkedProviders } from "$lib/sync/browserWorkspaceMutationMirror";
  import { startSyncScheduler, stopSyncScheduler, runManualSyncNow, getSyncState } from "$lib/sync/syncScheduler.svelte";
  import { createLatestOnlyRunner } from "$lib/latestOnlyRunner";
  import {
    registerE2EBridge,
    unregisterE2EBridge,
    toCollaborationPath,
  } from "$lib/e2e/bridge";

  // Import auth
  import { initAuth, getCurrentWorkspace, verifyMagicLink, setServerUrl, refreshUserInfo, getAuthState, getWorkspaces, isSyncEnabled } from "./lib/auth";
  import {
    getLocalWorkspace,
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    getWorkspaceStorageType,
    discoverOpfsWorkspaces,
    createLocalWorkspace,
    setCurrentWorkspaceId,
    removeLocalWorkspace,
    getPrimaryWorkspaceProviderLink,
    getWorkspaceProviderLink,
    isWorkspaceSyncEnabled as isWorkspaceProviderSyncEnabled,
    setPluginMetadata,
    renameLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { getProviderDisplayLabel } from "$lib/sync/builtinProviders";
  import { linkWorkspace, unlinkWorkspace } from "$lib/sync/workspaceProviderService";

  type StartupPhaseMeasurement = {
    label: string;
    elapsedMs: number;
  };

  function getTimingNow(): number {
    return typeof performance !== "undefined" && typeof performance.now === "function"
      ? performance.now()
      : Date.now();
  }

  function getElapsedMs(startedAt: number): number {
    return Math.round(getTimingNow() - startedAt);
  }

  function createStartupTracer(prefix: string) {
    const startedAt = getTimingNow();
    const phases: StartupPhaseMeasurement[] = [];

    async function measure<T>(label: string, work: () => Promise<T>): Promise<T> {
      const phaseStartedAt = getTimingNow();
      try {
        return await work();
      } finally {
        const elapsedMs = getElapsedMs(phaseStartedAt);
        phases.push({ label, elapsedMs });
        console.info(`[${prefix}] ${label}`, { elapsedMs });
      }
    }

    function logSummary(status: string, extra: Record<string, unknown> = {}): void {
      console.info(`[${prefix}] ${status}`, {
        totalElapsedMs: getElapsedMs(startedAt),
        phases: [...phases].sort((a, b) => b.elapsedMs - a.elapsedMs),
        ...extra,
      });
    }

    return { measure, logSummary };
  }

  /** Dispatch a sync sub-command (SyncPull, SyncPush, SyncStatus) to the primary linked provider. */
  async function dispatchPluginSyncCommand(
    command: string,
  ): Promise<{ success: boolean; data?: unknown; error?: string }> {
    const wsId = getCurrentWorkspaceId();
    if (!wsId) return { success: false, error: "No workspace" };
    const link = getPrimaryWorkspaceProviderLink(wsId);
    if (!link) return { success: false, error: "No sync provider linked" };
    if (isTauri()) {
      try {
        const backend = workspaceStore.backend ?? await getBackend();
        const a = createApi(backend);
        const data = await a.executePluginCommand(link.pluginId, command, {
          provider_id: link.pluginId,
        });
        return { success: true, data };
      } catch (e) {
        return { success: false, error: e instanceof Error ? e.message : String(e) };
      }
    }
    return await browserPlugins.dispatchCommand(link.pluginId, command, {
      provider_id: link.pluginId,
    });
  }

  // Initialize theme store immediately
  const themeStore = getThemeStore();

  // Initialize template context store (feeds live values to editor template variables)
  const templateContextStore = getTemplateContextStore();

  // Initialize audience panel store
  const audiencePanelStore = getAudiencePanelStore();

  // Initialize appearance store (theme presets, typography, layout)
  const appearanceStore = getAppearanceStore();

  // Import marketplace / onboarding
  import type { BundleRegistryEntry, SpotlightStep } from "$lib/marketplace/types";
  import SpotlightOverlay from "$lib/components/SpotlightOverlay.svelte";
  import {
    shouldBypassWelcomeScreenForE2E,
    maybeBootstrapIosStarterWorkspace,
    autoCreateDefaultWorkspace as autoCreateDefaultWorkspaceController,
    handleGetStarted as handleGetStartedController,
    handleSignInCreateNew as handleSignInCreateNewController,
    handleCreateWithProvider as handleCreateWithProviderController,
    handleWelcomeComplete as handleWelcomeCompleteController,
    type AutoCreateWorkspaceDeps,
  } from "./controllers/onboardingController";

  // Import services
  import {
    revokeBlobUrls,
    checkForAppUpdatesInBackground,
  } from "./models/services";
  import {
    getMimeType,
    getAttachmentMediaKind,
    isHeicFile,
    convertHeicToJpeg,
    type AttachmentMediaKind,
  } from "./models/services/attachmentService";

  // Sync/CRDT orchestration is entirely plugin-owned (diaryx_sync plugin).
  function initEventSubscription(backendInstance: any): () => void {
    if (!backendInstance?.onFileSystemEvent) {
      return () => {};
    }

    const subscriptionId = backendInstance.onFileSystemEvent(async (event: any) => {
      // Bridge sync status events to the collaboration store
      if (event?.type === "SyncStatusChanged") {
        if (event.status === "error" && event.error) {
          collaborationStore.setSyncError(event.error);
        } else {
          collaborationStore.setSyncStatus(event.status);
        }
        return;
      }
      if (event?.type === "SyncProgress") {
        collaborationStore.setSyncProgress({
          total: event.total ?? 0,
          completed: event.completed ?? 0,
        });
        return;
      }

      const current = entryStore.currentEntry;
      const activeApi = workspaceStore.backend ? createApi(workspaceStore.backend) : null;

      if (event?.type === "FileRenamed" && current && current.path === event.old_path && event.new_path) {
        entryStore.setCurrentEntry({
          ...current,
          path: event.new_path,
          title: current.title ?? null,
          content: current.content ?? "",
          frontmatter: current.frontmatter ?? {},
        });
      }

      const touchesCurrent =
        !!current &&
        (event?.path === current.path ||
          event?.old_path === current.path ||
          event?.new_path === current.path);

      if (touchesCurrent && activeApi) {
        const refreshPath = event?.new_path ?? current.path;
        try {
          const refreshed = await activeApi.getEntry(refreshPath);
          refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
          entryStore.setCurrentEntry(refreshed);
          entryStore.setDisplayContent(refreshed.content);
        } catch {
          // Ignore transient read failures.
        }
      }

      debouncedRefreshTree();
    });

    return () => {
      backendInstance.offFileSystemEvent?.(subscriptionId);
    };
  }

  async function withLiveSyncSetupProgress<T>(
    onProgress: ((progress: { percent: number; message: string; detail?: string }) => void) | undefined,
    task: () => Promise<T>,
  ): Promise<T> {
    if (!onProgress) {
      return await task();
    }

    const backend = workspaceStore.backend ?? await getBackend();
    if (!backend?.onFileSystemEvent || !backend?.offFileSystemEvent) {
      return await task();
    }

    const subscriptionId = backend.onFileSystemEvent((event: any) => {
      if (event?.type === "SyncProgress") {
        const total = Number(event.total ?? 0);
        const completed = Number(event.completed ?? 0);
        const percent = typeof event.percent === "number"
          ? event.percent
          : total > 0
            ? Math.round((completed / total) * 100)
            : 0;
        onProgress({
          percent,
          message: typeof event.message === "string" && event.message.length > 0
            ? event.message
            : "Syncing workspace...",
          detail: typeof event.path === "string" && event.path.length > 0 ? event.path : undefined,
        });
      } else if (event?.type === "SyncStatusChanged" && event.status === "error" && event.error) {
        onProgress({
          percent: 100,
          message: "Sync failed",
          detail: String(event.error),
        });
      }
    });

    try {
      return await task();
    } finally {
      backend.offFileSystemEvent(subscriptionId);
    }
  }

  // Import controllers
  import {
    refreshTree as refreshTreeController,
    loadNodeChildren as loadNodeChildrenController,
    runValidation as runValidationController,
    validatePath,
    openEntry as openEntryController,
    saveEntryWithSync,
    createChildEntryWithSync,
    createEntryWithSync,
    deleteEntryWithSync,
    duplicateEntry as duplicateEntryController,
    handleImportFromClipboard as importFromClipboardHandler,
    handleImportMarkdownFile as importMarkdownFileHandler,
    handleAddAttachment as addAttachmentHandler,
    handleAttachmentFileSelect as attachmentFileSelectHandler,
    handleEditorFileDrop as editorFileDropHandler,
    handleDeleteAttachment as deleteAttachmentHandler,
    handleAttachmentInsert as attachmentInsertHandler,
    handleMoveAttachment as moveAttachmentHandler,
    handleLinkClick as linkClickHandler,
    handleValidateWorkspace,
    handleWordCount,
    handleCopyAsMarkdown,
    handleViewMarkdown,
    handleReorderFootnotes,
  } from "./controllers";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);
  const mobileState = getMobileState();
  const mobileGestures = useMobileGestures();

  // Entry navigation intent tracking (keeps sidebar selection responsive while
  // the backend is still opening the next file).
  let pendingEntryPath = $state<string | null>(null);

  // ========================================================================
  // Store-backed state (using getters for now, will migrate fully later)
  // This allows gradual migration without breaking the component
  // ========================================================================

  // Entry state - proxied from entryStore
  let currentEntry = $derived(entryStore.currentEntry);
  let isDirty = $derived(entryStore.isDirty);
  let isSaving = $derived(entryStore.isSaving);
  // Editor read-only state (may be set by plugins such as sync guest mode)
  let editorReadonly = $state(false);
  let isLoading = $derived(entryStore.isLoading);
  let titleError = $derived(entryStore.titleError);
  let displayContent = $derived(entryStore.displayContent);
  let activeEntryPath = $derived(pendingEntryPath ?? currentEntry?.path ?? null);
  let loadingTargetPath = $derived(
    pendingEntryPath && pendingEntryPath !== currentEntry?.path
      ? pendingEntryPath
      : null
  );

  // UI state - proxied from uiStore
  let leftSidebarCollapsed = $derived(uiStore.leftSidebarCollapsed);
  let rightSidebarCollapsed = $derived(uiStore.rightSidebarCollapsed);
  let leftSidebarWidth = $derived(uiStore.leftSidebarWidth);
  let rightSidebarWidth = $derived(uiStore.rightSidebarWidth);
  let showSettingsDialog = $derived(uiStore.showSettingsDialog);
  let showExportDialog = $derived(uiStore.showExportDialog);
  let showNewEntryModal = $derived(uiStore.showNewEntryModal);
  let exportPath = $derived(uiStore.exportPath);
  let editorRef = $derived(uiStore.editorRef);

  // Workspace missing state — set when the workspace directory was moved/deleted externally
  let workspaceMissing = $state<{ id: string; name: string } | null>(null);

  /** Relocate a missing workspace by picking a new folder (Tauri only). */
  async function handleRelocateWorkspace() {
    if (!workspaceMissing) return;
    const { id, name } = workspaceMissing;
    try {
      const folder = await pickAuthorizedWorkspaceFolder(`Locate "${name}"`);
      if (!folder) return;
      const { addLocalWorkspace } = await import("$lib/storage/localWorkspaceRegistry.svelte");
      addLocalWorkspace({ id, name, path: folder });
      workspaceMissing = null;
      // Re-initialize with the corrected path
      entryStore.setLoading(true);
      await switchWorkspace(id, name);
      await handleWorkspaceSwitchComplete();
    } catch (e: any) {
      if (e?.name !== "AbortError") {
        console.error("[App] Relocate workspace failed:", e);
      }
    }
  }

  /** Remove a missing workspace from the registry and show the welcome screen. */
  function handleRemoveWorkspace() {
    if (!workspaceMissing) return;
    removeLocalWorkspace(workspaceMissing.id);
    workspaceMissing = null;
    showWelcomeScreen = true;
  }

  // Find in file state
  let showFindBar = $state(false);

  // Move entry dialog
  let moveEntryDialogPath = $state<string | null>(null);

  // In-app prompt dialogs (replaces window.prompt which is blocked in Tauri WKWebView)
  let promptDialog = $state<{
    title: string;
    label: string;
    value: string;
    onSubmit: (value: string) => void;
  } | null>(null);

  // Sidebar resize state
  let resizingSidebar = $state<'left' | 'right' | null>(null);
  let resizeStartX = $state(0);
  let resizeStartWidth = $state(0);

  function onResizePointerDown(side: 'left' | 'right', e: PointerEvent) {
    e.preventDefault();
    resizingSidebar = side;
    resizeStartX = e.clientX;
    resizeStartWidth = side === 'left' ? leftSidebarWidth : rightSidebarWidth;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizePointerMove(e: PointerEvent) {
    if (!resizingSidebar) return;
    const delta = resizingSidebar === 'left'
      ? e.clientX - resizeStartX
      : resizeStartX - e.clientX;
    const newWidth = resizeStartWidth + delta;
    if (resizingSidebar === 'left') {
      uiStore.setLeftSidebarWidth(newWidth);
    } else {
      uiStore.setRightSidebarWidth(newWidth);
    }
  }

  function onResizeDblClick(side: 'left' | 'right') {
    if (side === 'left') {
      uiStore.setLeftSidebarWidth(288);
    } else {
      uiStore.setRightSidebarWidth(288);
    }
  }

  function onResizePointerUp(e: PointerEvent) {
    if (resizingSidebar) {
      (e.target as HTMLElement).releasePointerCapture(e.pointerId);
    }
    resizingSidebar = null;
  }

  // Right sidebar tab control (built-in tabs or plugin tab IDs)
  let requestedSidebarTab: string | null = $state(null);

  // Left sidebar tab control (plugin-owned tabs)
  let requestedLeftTab: string | null = $state(null);


  // Delete confirmation dialog state
  let showDeleteConfirm = $state(false);
  let pendingDeletePaths = $state<string[]>([]);
  let pendingDeleteIncludesDescendants = $state(false);
  let isDeletingEntries = $state(false);

  // Generic plugin-hosted confirmation dialog state
  let showPluginConfirmDialog = $state(false);
  let pluginConfirmTitle = $state("Confirm");
  let pluginConfirmDescription = $state("");
  let pluginConfirmConfirmLabel = $state("Confirm");
  let pluginConfirmCancelLabel = $state("Cancel");
  let pluginConfirmVariant = $state<"default" | "destructive">("default");
  let pluginConfirmResolve: ((result: boolean) => void) | null = null;

  // Generic plugin-hosted prompt dialog state
  let showPluginPromptDialog = $state(false);
  let pluginPromptTitle = $state("Prompt");
  let pluginPromptDescription = $state("");
  let pluginPromptConfirmLabel = $state("OK");
  let pluginPromptCancelLabel = $state("Cancel");
  let pluginPromptVariant = $state<"default" | "destructive">("default");
  let pluginPromptValue = $state("");
  let pluginPromptPlaceholder = $state("");
  let pluginPromptResolve: ((result: string | null) => void) | null = null;
  let pluginPromptInput: HTMLInputElement | null = $state(null);

  // Audience dialog state
  let showAudienceDialog = $state(false);
  let audienceDialogPath = $state<string | null>(null);
  let audienceDialogAudience = $state<string[] | null>(null);
  let pendingDeleteName = $derived.by(() => {
    if (pendingDeletePaths.length === 1) {
      return pendingDeletePaths[0]?.split('/').pop()?.replace('.md', '') ?? '';
    }
    if (pendingDeletePaths.length > 1) {
      return `${pendingDeletePaths.length} selected entries`;
    }
    return '';
  });
  let pendingDeleteDescription = $derived.by(() => {
    if (pendingDeletePaths.length === 0) return "";
    if (pendingDeletePaths.length === 1) {
      return pendingDeleteIncludesDescendants
        ? `Are you sure you want to delete "${pendingDeleteName}" and its descendants? This action cannot be undone.`
        : `Are you sure you want to delete "${pendingDeleteName}"? This action cannot be undone.`;
    }
    return pendingDeleteIncludesDescendants
      ? `Are you sure you want to delete ${pendingDeletePaths.length} selected entries and their descendants? This action cannot be undone.`
      : `Are you sure you want to delete ${pendingDeletePaths.length} selected entries? This action cannot be undone.`;
  });

  // Welcome screen (shown when no workspaces exist, or when user clicks logo)
  let showWelcomeScreen = $state(false);
  let welcomeScreenRef: ReturnType<typeof WelcomeScreen> | undefined = $state();
  /** Non-null when the user navigated to the welcome screen from an active workspace */
  let welcomeReturnWorkspaceName = $state<string | null>(null);
  /** When set, opens the welcome screen to a specific view (e.g. 'bundles', 'workspace-picker'). */
  let welcomeInitialView = $state<'main' | 'sign-in' | 'workspace-picker' | 'bundles' | 'provider-choice' | null>(null);
  // Clear initialView when the welcome screen closes so it doesn't persist on re-open
  $effect(() => { if (!showWelcomeScreen) welcomeInitialView = null; });
  let spotlightSteps = $state<SpotlightStep[] | null>(null);

  // Launch zoom overlay (persists after WelcomeScreen unmounts)
  let launchOverlay = $state<BundleSelectInfo | null>(null);
  let launchOverlayDone = $state(false);

  async function dismissLaunchOverlay() {
    launchOverlayDone = true;
    await new Promise((r) => setTimeout(r, 500));
    launchOverlay = null;
    launchOverlayDone = false;
  }

  // Mobile spotlight actions: open/close sidebars for steps targeting elements inside them.
  // The spotlight SVG cutout reveals the target through the overlay — no z-index hacks needed.
  const mobileSpotlightActions: Record<string, { prepare: () => Promise<(() => void) | null> }> = {
    "workspace-tree": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        return () => {
          uiStore.setLeftSidebarCollapsed(true);
        };
      }
    },
    "marketplace-button": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        return () => {
          uiStore.setLeftSidebarCollapsed(true);
        };
      }
    },
    "properties-panel": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(true);
        uiStore.setRightSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        return () => {
          uiStore.setRightSidebarCollapsed(true);
        };
      }
    },
  };

  // Marketplace dialog
  let showMarketplaceDialog = $state(false);

  // Edge hover state for sidebar open buttons (focus mode reveal)
  let leftEdgeHovered = $state(false);
  // Mobile focus-mode chrome tap-to-reveal state
  let mobileFocusChromeRevealed = $state(false);
  let mobileFocusChromeRevealTimer: ReturnType<typeof setTimeout> | undefined;
  // Effective audience tags for the current entry (includes inherited/default)
  let effectiveAudienceTags = $state<string[]>([]);
  // FSA reconnect state (shown when local folder needs user gesture to re-grant access)
  let fsaNeedsReconnect = $state(false);
  let fsaReconnectWsId = $state<string | undefined>(undefined);
  let fsaReconnectWsName = $state<string | undefined>(undefined);

  // Settings dialog initial tab (for opening to a specific tab)
  let settingsInitialTab = $state<string | undefined>(undefined);

  // Workspace state - proxied from workspaceStore
  let tree = $derived(workspaceStore.tree);
  let expandedNodes = $derived(workspaceStore.expandedNodes);
  let validationResult = $derived(workspaceStore.validationResult);
  let backend = $derived(workspaceStore.backend);
  let showUnlinkedFiles = $derived(workspaceStore.showUnlinkedFiles);
  let showHiddenFiles = $derived(workspaceStore.showHiddenFiles);
  let focusMode = $derived(workspaceStore.focusMode);
  let mobileFocusModeActive = $derived(
    focusMode && leftSidebarCollapsed && rightSidebarCollapsed,
  );

  // API wrapper - uses execute() internally for all operations
  let api: Api | null = $derived(backend ? createApi(backend) : null);

  // Root frontmatter plugin permissions cache (used by runtime permission checks).
  let pluginPermissionsConfig = $state<Record<string, PluginConfig> | undefined>(
    undefined,
  );
  let pluginPermissionsRootPath = $state<string | null>(null);

  // Reserved for plugin-provided history panels that may need host context.
  let rustApi: any | null = $state(null);
  let lastAutoDispatchedFileOpenKey = $state<string | null>(null);


  // Collaboration state - proxied from collaborationStore
  let collaborationEnabled = $derived(collaborationStore.collaborationEnabled);
  let authState = $derived(getAuthState());
  const syncState = getSyncState();
  let collapsedBarSyncEnabled = $derived.by(() => {
    const wsId = getCurrentWorkspaceId();
    return !!wsId && isWorkspaceProviderSyncEnabled(wsId);
  });
  let collapsedBarSyncColor = $derived.by(() => {
    const s = collaborationStore.effectiveSyncStatus;
    if (s === "synced") return "text-green-600 dark:text-green-400";
    if (s === "syncing" || s === "connecting") return "text-amber-500";
    if (s === "error") return "text-red-500";
    return "text-muted-foreground";
  });
  let pluginManifestCount = $derived(getPluginStore().allManifests.length);
  let activeLocalWorkspaceId = $derived(
    authState.activeWorkspaceId ?? getCurrentWorkspaceId(),
  );
  const PREVIEW_AUDIENCE_UNSET = Symbol("preview-audience-unset");
  let lastPreviewAudience = $state<string[] | null | typeof PREVIEW_AUDIENCE_UNSET>(
    PREVIEW_AUDIENCE_UNSET,
  );

  // ========================================================================
  // Non-store state (component-specific, not shared)
  // ========================================================================

  // Auto-save timer (component-local, not needed in global store)
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;
  const AUTO_SAVE_DELAY_MS = 300; // 300ms – near-instant local save; remote sync has its own 3s debounce

  // Tree refresh debounce timer (prevents rapid refreshes during sync)
  let refreshTreeTimeout: ReturnType<typeof setTimeout> | null = null;
  const REFRESH_TREE_DEBOUNCE_MS = 100;

  // Event subscription cleanup (for filesystem events from Rust backend)
  let cleanupEventSubscription: (() => void) | null = null;
  let guestWorkspaceState:
    | {
        previousBackend: typeof backend;
        previousWorkspaceId: string | null;
        previousWorkspaceName: string | null;
        previousStorageType: ReturnType<typeof getWorkspaceStorageType> | undefined;
      }
    | null = null;

  function clearMobileFocusChromeRevealTimer(): void {
    if (mobileFocusChromeRevealTimer) {
      clearTimeout(mobileFocusChromeRevealTimer);
      mobileFocusChromeRevealTimer = undefined;
    }
  }

  function revealMobileFocusChromeTemporarily(): void {
    if (!mobileFocusModeActive) {
      return;
    }

    mobileFocusChromeRevealed = true;
    clearMobileFocusChromeRevealTimer();
    mobileFocusChromeRevealTimer = setTimeout(() => {
      mobileFocusChromeRevealed = false;
      mobileFocusChromeRevealTimer = undefined;
    }, 3000);
  }

  $effect(() => {
    if (!mobileFocusModeActive) {
      clearMobileFocusChromeRevealTimer();
      mobileFocusChromeRevealed = false;
    }
  });

  // Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri)
  function normalizeFrontmatter(frontmatter: any): Record<string, any> {
    if (!frontmatter) return {};
    if (frontmatter instanceof Map) {
      return Object.fromEntries(frontmatter.entries());
    }
    return frontmatter;
  }

  async function resolveWorkspaceRootIndexPath(): Promise<string | null> {
    if (!api) {
      return null;
    }

    return await api.resolveWorkspaceRootIndexPath(tree?.path ?? null);
  }

  async function reloadPluginPermissionsConfig(): Promise<void> {
    if (!api) {
      pluginPermissionsConfig = undefined;
      pluginPermissionsRootPath = null;
      return;
    }

    const rootIndexPath = await resolveWorkspaceRootIndexPath();
    if (!rootIndexPath) {
      pluginPermissionsConfig = {};
      pluginPermissionsRootPath = null;
      return;
    }

    if (pluginPermissionsRootPath !== rootIndexPath) {
      permissionStore.clearSessionCache();
      pluginPermissionsRootPath = rootIndexPath;
    }

    try {
      const fm = await api.getFrontmatter(rootIndexPath);
      const raw = fm.plugins as unknown;
      if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
        pluginPermissionsConfig = {};
        return;
      }
      pluginPermissionsConfig = raw as Record<string, PluginConfig>;
    } catch {
      pluginPermissionsConfig = {};
    }
  }

  async function persistPluginPermissionsConfig(
    nextConfig: Record<string, PluginConfig>,
  ): Promise<void> {
    if (!api) {
      throw new Error("Workspace root is not available");
    }

    const rootIndexPath = await resolveWorkspaceRootIndexPath();
    if (!rootIndexPath) {
      throw new Error("Workspace root is not available");
    }
    await api.setFrontmatterProperty(
      rootIndexPath,
      "plugins",
      nextConfig as unknown as JsonValue,
      rootIndexPath,
    );
    pluginPermissionsRootPath = rootIndexPath;
    pluginPermissionsConfig = nextConfig;
  }

  function hasRequestedPermissionDefaults(defaults: PluginPermissions): boolean {
    return Object.values(defaults).some((rule) => rule != null);
  }

  function mergeRequestedPermissionDefaults(
    currentConfig: Record<string, PluginConfig> | undefined,
    pluginId: string,
    defaults: PluginPermissions,
  ): Record<string, PluginConfig> | null {
    if (!hasRequestedPermissionDefaults(defaults)) {
      return null;
    }

    const existingConfig = currentConfig ?? {};
    const existingPluginConfig = existingConfig[pluginId] ?? { permissions: {} };
    const nextPermissions: PluginPermissions = {
      ...(existingPluginConfig.permissions ?? {}),
    };
    let changed = false;

    for (const [permissionType, requestedRule] of Object.entries(defaults)) {
      if (!requestedRule) continue;
      const typedPermission = permissionType as keyof PluginPermissions;
      const existingRule = nextPermissions[typedPermission];
      const isEmptyRule =
        !existingRule ||
        ((existingRule.include?.length ?? 0) === 0 &&
          (existingRule.exclude?.length ?? 0) === 0);
      if (!isEmptyRule) continue;

      nextPermissions[typedPermission] = {
        include: [...(requestedRule.include ?? [])],
        exclude: [...(requestedRule.exclude ?? [])],
      } as never;
      changed = true;
    }

    if (!changed) {
      return null;
    }

    return {
      ...existingConfig,
      [pluginId]: {
        ...existingPluginConfig,
        permissions: nextPermissions,
      },
    };
  }

  async function persistRequestedPluginPermissionDefaults(
    pluginId: string,
    defaults: PluginPermissions,
  ): Promise<void> {
    const nextConfig = mergeRequestedPermissionDefaults(
      pluginPermissionsConfig,
      pluginId,
      defaults,
    );
    if (!nextConfig) return;
    await persistPluginPermissionsConfig(nextConfig);
  }

  async function reloadWorkspaceScopedBrowserState(): Promise<void> {
    const activeBackend = await getBackend();

    await Promise.all([
      themeStore.reloadFromWorkspace?.(),
      appearanceStore.reloadFromWorkspace?.(),
    ]);

    if (isTauri() || isNativePluginBackend()) {
      return;
    }

    const pluginSupport = browserPlugins.getBrowserPluginSupport();
    if (!pluginSupport.supported) {
      console.info('[App] Browser plugins disabled:', pluginSupport.reason);
      return;
    }

    browserPlugins.setPluginPermissionConfigProvider(() => pluginPermissionsConfig);
    browserPlugins.setPluginPermissionConfigPersistor(
      persistRequestedPluginPermissionDefaults,
    );
    await browserPlugins.loadAllPlugins().catch((e: unknown) =>
      console.warn('[App] Failed to load browser plugins:', e),
    );

    // Stop any previous sync scheduler before restarting with new workspace context.
    stopSyncScheduler();

    await mirrorCurrentWorkspaceMutationToLinkedProviders({
      backend: {
        getWorkspacePath: () => activeBackend.getWorkspacePath(),
        resolveRootIndex: async (workspacePath) => {
          const finder = (activeBackend as { findRootIndex?: (path: string) => Promise<string> }).findRootIndex;
          return typeof finder === "function" ? await finder(workspacePath) : workspacePath;
        },
      },
      runPluginCommand: async (pluginId, command, params = null) => {
        const api = createApi(activeBackend);
        return await api.executePluginCommand(pluginId, command, params);
      },
    }).catch((e: unknown) =>
      console.warn('[App] Failed to initialize linked workspace sync state:', e),
    );

    getPluginStore().preloadInsertCommandIcons();

    // Start debounced sync scheduler — runs full provider sync on startup,
    // after a quiet period following file mutations, and when the tab resumes.
    startSyncScheduler();
  }



  // Attachment state
  let attachmentError: string | null = $state(null);
  let attachmentFileInput: HTMLInputElement | null = $state(null);

  // Image preview state
  let imagePreviewOpen = $state(false);
  let previewImageUrl: string | null = $state(null);
  let previewImageName = $state("");
  let previewImageKind: AttachmentMediaKind = $state("image");

  // Markdown preview state
  let markdownPreviewOpen = $state(false);
  let markdownPreviewBody = $state("");
  let markdownPreviewFrontmatter: Record<string, unknown> = $state({});

  // Note: Blob URL management is now in attachmentService.ts

  // Display settings are now persisted to workspace root index frontmatter
  // via workspaceStore.hydrateDisplaySettings() — no localStorage needed.

  $effect(() => {
    void pluginManifestCount;
    void activeLocalWorkspaceId;
    const autoFileOpenDisabledForE2E = import.meta.env.DEV
      && typeof globalThis !== "undefined"
      && (globalThis as typeof globalThis & {
        __diaryx_e2e_disable_auto_file_open?: boolean;
      }).__diaryx_e2e_disable_auto_file_open === true;
    if (autoFileOpenDisabledForE2E) {
      lastAutoDispatchedFileOpenKey = null;
      return;
    }
    if (!collaborationEnabled || !currentEntry?.path) {
      lastAutoDispatchedFileOpenKey = null;
      return;
    }
    const syncPath = toCollaborationPath(currentEntry.path, tree?.path ?? "");
    const dispatchKey = `${activeLocalWorkspaceId ?? "local"}:${pluginManifestCount}:${syncPath}`;
    if (lastAutoDispatchedFileOpenKey === dispatchKey) {
      return;
    }
    lastAutoDispatchedFileOpenKey = dispatchKey;
    void browserPlugins.dispatchFileOpenedEvent(syncPath);
  });

  $effect(() => {
    if (!getPluginStore().isPluginEnabled("diaryx.templating")) return;
    if (currentEntry?.frontmatter) {
      templateContextStore.setContext(normalizeFrontmatter(currentEntry.frontmatter));
    } else {
      templateContextStore.clear();
    }
  });

  // Refresh the file tree when the audience preview filter changes
  $effect(() => {
    const previewAudience = templateContextStore.previewAudience;
    if (!api || !backend) return;
    if (lastPreviewAudience === PREVIEW_AUDIENCE_UNSET) {
      lastPreviewAudience = previewAudience;
      return;
    }
    if (JSON.stringify(lastPreviewAudience) === JSON.stringify(previewAudience)) {
      return;
    }
    lastPreviewAudience = previewAudience;
    refreshTree();
  });

  // Resolve effective audience (including inherited) for the mobile header dots
  $effect(() => {
    const entry = currentEntry;
    const currentApi = api;
    if (!entry || !currentApi) {
      effectiveAudienceTags = [];
      return;
    }
    // If explicit tags exist, use them directly
    if (Array.isArray(entry.frontmatter?.audience) && entry.frontmatter.audience.length > 0) {
      effectiveAudienceTags = entry.frontmatter.audience.map(String);
      return;
    }
    // Otherwise resolve inherited/default audience
    currentApi.getEffectiveAudience(entry.path).then((result) => {
      effectiveAudienceTags = result.tags ?? [];
    }).catch(() => {
      effectiveAudienceTags = [];
    });
  });

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    registerE2EBridge({
      getTreeRootPath: () => tree?.path ?? "",
      getCurrentEntryPath: () => currentEntry?.path ?? null,
      openEntry,
      normalizeFrontmatter,
      getCollaborationEnabled: () => collaborationEnabled,
      getTree: () => tree,
    });

    // Refresh tree when zip import completes (from ImportSettings)
    window.addEventListener("import:complete", handleImportComplete);

    if (typeof window !== "undefined") {
      const params = new URLSearchParams(window.location.search);
      const isOauthCallback = window.location.pathname === "/oauth/callback";
      const code = params.get("code");
      const error = params.get("error");
      if (isOauthCallback && window.opener && (code || error)) {
        window.opener.postMessage(
          {
            type: "oauth-callback",
            code,
            error,
          },
          window.location.origin,
        );
        window.close();
        return;
      }
    }

    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      uiStore.setLeftSidebarCollapsed(false);
      uiStore.setRightSidebarCollapsed(false);
    }

    // Add swipe gestures for mobile:
    // - Swipe up from bottom: open command palette
    // - Swipe right from the left edge: open left sidebar (or close right sidebar if open)
    // - Swipe left from the right edge: open right sidebar (or close left sidebar if open)
    // Gestures that begin inside modal surfaces or become text selections are ignored.
    // Attached early so gestures work even if workspace initialisation fails.
    mobileGestures.attach();

    // On macOS Tauri with overlay titlebar, set titlebar area height for traffic light clearance
    if (isTauri() && (navigator.platform === "MacIntel" || navigator.platform.startsWith("Mac"))) {
      document.documentElement.style.setProperty("--titlebar-area-height", "28px");
      document.documentElement.classList.add("tauri-macos-overlay");
    }

    // Load saved collaboration settings (server URL is read by the sync plugin)
    if (typeof window !== "undefined") {
      const savedServerUrl = localStorage.getItem("diaryx_sync_server_url")
        ?? localStorage.getItem("diaryx-sync-server");
      if (savedServerUrl) {
        collaborationStore.setServerUrl(savedServerUrl);
      }
    }

    const startupTracer = createStartupTracer("WorkspaceStartup");
    let startupStatus = "failed";
    let startupWorkspaceId: string | null = null;

    try {
      // Kick off independent init work in parallel:
      // - Auth validation (HTTP call to server)
      // - Editor component dynamic import
      // - OPFS workspace discovery (filesystem scan)
      const [, editorModule] = await Promise.all([
        startupTracer.measure("auth bootstrap", async () => {
          // Initialize auth state - if user was previously logged in,
          // this will validate their token and enable collaboration automatically
          await initAuth();

          // Check for magic link token in URL (auto-verify without wizard)
          // This must happen AFTER initAuth() so the auth service is initialized
          if (typeof window !== "undefined") {
            const params = new URLSearchParams(window.location.search);
            const token = params.get("token");
            if (token) {
              // Clear the token from URL immediately to prevent double verification
              const url = new URL(window.location.href);
              url.searchParams.delete("token");
              window.history.replaceState({}, "", url.toString());

              // If no server URL is configured, set the default before verifying
              // This handles the case where user clicks magic link in a new browser/tab
              const serverUrl = localStorage.getItem("diaryx_sync_server_url");
              if (!serverUrl) {
                setServerUrl("https://app.diaryx.org/api");
              }
              // Verify automatically and wait for completion before continuing
              await handleMagicLinkToken(token);
            }
          }

          // Check for Stripe checkout result in URL
          if (typeof window !== "undefined") {
            const params = new URLSearchParams(window.location.search);
            const checkoutResult = params.get("checkout");
            if (checkoutResult) {
              const url = new URL(window.location.href);
              url.searchParams.delete("checkout");
              window.history.replaceState({}, "", url.toString());

              if (checkoutResult === "success") {
                // Poll for tier update — the webhook often arrives after the redirect
                let upgraded = false;
                for (let i = 0; i < 10; i++) {
                  await refreshUserInfo();
                  if (getAuthState().tier === "plus") {
                    upgraded = true;
                    break;
                  }
                  await new Promise((r) => setTimeout(r, 1500));
                }
                if (upgraded) {
                  toast.success("Welcome to Diaryx Plus!", {
                    description: "Your subscription is now active.",
                  });
                } else {
                  toast.info("Payment received!", {
                    description: "Your subscription is being activated. Please refresh in a moment.",
                  });
                }
              }
            }
          }
        }),
        startupTracer.measure("editor import", () => import("./lib/Editor.svelte")),
        startupTracer.measure("discover OPFS workspaces", () => discoverOpfsWorkspaces()),
      ]);

      Editor = editorModule.default;

      // HTTP backend mode (`diaryx edit`): skip workspace registry — the CLI
      // server already knows which workspace to serve.
      const httpParams = typeof window !== "undefined" ? new URLSearchParams(window.location.search) : null;
      const isHttpBackend = httpParams?.get("backend") === "http" && !!httpParams?.get("api_url");

      // On Tauri mobile, the workspace lives at a fixed path (document_dir/Diaryx).
      // Skip the registry entirely — just initialize the backend with the default
      // path. The registry (localStorage) can be cleared by the OS independently
      // of the actual workspace files, so we never rely on it for mobile startup.
      const isTauriMobile = isTauri() && isIOS();

      // Check if any workspaces exist before proceeding
      let defaultWorkspace = getCurrentWorkspace();
      let localWsList = getLocalWorkspaces();
      let currentWsId = getCurrentWorkspaceId();

      if (!isTauriMobile && !isHttpBackend && !defaultWorkspace && (localWsList.length === 0 || !currentWsId)) {
        const dataJustCleared = sessionStorage.getItem('diaryx_data_cleared');
        if (dataJustCleared) {
          sessionStorage.removeItem('diaryx_data_cleared');
        }

        if ((globalThis as any).__diaryx_preview || shouldBypassWelcomeScreenForE2E()) {
          try {
            await autoCreateDefaultWorkspace(null);
            defaultWorkspace = getCurrentWorkspace();
            localWsList = getLocalWorkspaces();
            currentWsId = getCurrentWorkspaceId();
          } catch (e) {
            console.error("[App] E2E onboarding bypass failed:", e);
            showWelcomeScreen = true;
            startupStatus = "welcome_screen";
            return;
          }
        } else {
          // No workspaces exist — show welcome/onboarding screen
          showWelcomeScreen = true;
          startupStatus = "welcome_screen";
          return;
        }
      }

      // Initialize the backend (auto-detects Tauri vs WASM)
      // Pass workspace ID and name so the backend uses the correct OPFS directory.
      // On Tauri mobile, pass undefined — initialize_app resolves the fixed default path.
      let wsId: string | undefined;
      let wsName: string | undefined;
      if (isTauriMobile) {
        // Mobile: backend uses document_dir/Diaryx directly, no registry needed
        wsId = currentWsId ?? undefined;
        wsName = undefined;
      } else if (isHttpBackend) {
        // HTTP backend doesn't use the workspace registry
        wsId = undefined;
        wsName = undefined;
      } else if (defaultWorkspace) {
        wsId = defaultWorkspace.id;
        wsName = defaultWorkspace.name;
      } else {
        const localWs = getLocalWorkspace(currentWsId ?? '');
        if (!localWs) {
          showWelcomeScreen = true;
          startupStatus = "welcome_screen";
          return;
        }
        wsId = localWs.id;
        wsName = localWs.name;
      }
      // Save for FSA reconnect in case getBackend throws FsaGestureRequiredError
      fsaReconnectWsId = wsId;
      fsaReconnectWsName = wsName;
      startupWorkspaceId = wsId ?? null;
      const backendInstance = await startupTracer.measure(
        "backend init",
        () => getBackend(wsId, wsName, wsId ? getWorkspaceStorageType(wsId) : undefined),
      );
      workspaceStore.setBackend(backendInstance);
      void checkForAppUpdatesInBackground(backendInstance);

      // On Tauri mobile, ensure the workspace is in the registry now that the
      // backend is initialized. This back-fills the registry after we skipped
      // the registry check above, so settings/sync/etc. still work correctly.
      if (isTauriMobile && getLocalWorkspaces().length === 0) {
        const ws = createLocalWorkspace(
          localStorage.getItem('diaryx-workspace-name') || 'My Workspace',
        );
        setCurrentWorkspaceId(ws.id);
        startupWorkspaceId = ws.id;
      }

      // HTTP backend: bootstrap auth from server-provided user info.
      // This runs AFTER initAuth() (which found nothing) so we directly
      // populate the auth store from the CLI's cached /auth/me response.
      if (isHttpBackend && 'authInfo' in backendInstance && backendInstance.authInfo) {
        const { bootstrapAuthFromHttp } = await import("./lib/auth/authStore.svelte");
        const apiUrl = httpParams?.get("api_url") ?? "";
        bootstrapAuthFromHttp(
          backendInstance.authInfo as import("./lib/auth/authService").MeResponse,
          `${apiUrl.replace(/\/+$/, "")}/api/sync`,
        );
      }

      const apiInstance = createApi(backendInstance);

      // Initialize filesystem event subscription for automatic UI updates
      cleanupEventSubscription = initEventSubscription(backendInstance);

      // When background plugin loading completes, refresh the plugin store
      // and re-open the current entry so plugin-dependent rendering
      // (highlighting, spoilers) and UI contributions take effect.
      if (backendInstance.onPluginsReady) {
        const unsubPlugins = backendInstance.onPluginsReady(async () => {
          console.log('[App] Plugins ready, refreshing plugin store and re-opening entry');
          try {
            await getPluginStore().init(apiInstance);
          } catch (e) {
            console.warn('[App] Failed to refresh plugin store after plugins-ready:', e);
          }
          if (currentEntry) {
            // Force reload from disk: the plugin manifest change above will
            // trigger an editor rebuild, and we need displayContent to refresh
            // from disk so the editor's content sync stays consistent with the
            // backend rather than the (potentially stale) in-memory prop.
            await openEntry(currentEntry.path, { force: true });
          }
        });
        // Clean up on workspace switch
        const prevCleanup = cleanupEventSubscription;
        cleanupEventSubscription = () => { unsubPlugins(); prevCleanup(); };
      }

      rustApi = null;

      // Set workspace ID for plugin system (sync plugin reads this)
      const sharedWorkspaceId = getCurrentWorkspace()?.id ?? null;
      workspaceStore.setWorkspaceId(sharedWorkspaceId);

      // Run plugin manifest fetch and tree refresh in parallel — both are
      // independent backend calls that don't depend on each other.
      await Promise.all([
        startupTracer.measure("plugin manifest init", () => getPluginStore().init(apiInstance)),
        startupTracer.measure("initial refreshTree", () => refreshTree()),
      ]);

      // Open the root entry immediately — the editor doesn't need config
      // hydration to render. Run config hydration in parallel.
      let openEntryPromise: Promise<void> | undefined;
      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        openEntryPromise = openEntry(tree.path);
      }

      // Hydrate view preferences from workspace config (stored in root index
      // frontmatter) so they travel with the workspace instead of localStorage.
      const workspaceRootIndexPath = await startupTracer.measure(
        "resolve workspace root index",
        () => apiInstance.resolveWorkspaceRootIndexPath(tree?.path ?? null),
      );
      if (workspaceRootIndexPath) {
        try {
          await startupTracer.measure("hydrate workspace config", async () => {
            const wsConfig = await apiInstance.getWorkspaceConfig(workspaceRootIndexPath);
            workspaceStore.hydrateDisplaySettings(wsConfig, async (field, value) => {
              try {
                const nextRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
                  tree?.path ?? null,
                );
                if (!nextRootIndexPath) return;
                await apiInstance.setWorkspaceConfig(nextRootIndexPath, field, value);
              } catch (e) {
                console.warn('[App] Failed to persist display setting:', field, e);
              }
            });

            // Hydrate theme mode
            themeStore.hydrateThemeMode(wsConfig.theme_mode, async (mode) => {
              try {
                const nextRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
                  tree?.path ?? null,
                );
                if (!nextRootIndexPath) return;
                await apiInstance.setWorkspaceConfig(nextRootIndexPath, 'theme_mode', mode);
              } catch (e) {
                console.warn('[App] Failed to persist theme_mode:', e);
              }
            });

            appearanceStore.hydrateWorkspaceTheme(
              {
                presetId: wsConfig.theme_preset,
                accentHue: wsConfig.theme_accent_hue,
              },
              async ({ presetId, accentHue }) => {
                try {
                  const nextRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
                    tree?.path ?? null,
                  );
                  if (!nextRootIndexPath) return;
                  await apiInstance.setWorkspaceConfig(nextRootIndexPath, 'theme_preset', presetId);
                  await apiInstance.setWorkspaceConfig(
                    nextRootIndexPath,
                    'theme_accent_hue',
                    accentHue === null ? 'null' : JSON.stringify(accentHue),
                  );
                } catch (e) {
                  console.warn('[App] Failed to persist workspace theme selection:', e);
                }
              },
            );

            // Hydrate audience colors
            getAudienceColorStore().hydrate(wsConfig.audience_colors as Record<string, string> | undefined, async (colors) => {
              try {
                const nextRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
                  tree?.path ?? null,
                );
                if (!nextRootIndexPath) return;
                await apiInstance.setWorkspaceConfig(nextRootIndexPath, 'audience_colors', JSON.stringify(colors));
              } catch (e) {
                console.warn('[App] Failed to persist audience_colors:', e);
              }
            });

            // Hydrate disabled plugins
            getPluginStore().hydrateDisabledPlugins(wsConfig.disabled_plugins, async (disabledIds) => {
              try {
                const nextRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
                  tree?.path ?? null,
                );
                if (!nextRootIndexPath) return;
                await apiInstance.setWorkspaceConfig(nextRootIndexPath, 'disabled_plugins', JSON.stringify(disabledIds));
              } catch (e) {
                console.warn('[App] Failed to persist disabled_plugins:', e);
              }
            });

            // Re-fetch tree if view prefs changed from defaults
            if (wsConfig.show_unlinked_files || wsConfig.show_hidden_files) {
              await startupTracer.measure(
                "refreshTree after workspace config hydration",
                () => refreshTree(),
              );
            }
          });
        } catch (e) {
          console.warn('[App] Failed to load workspace config:', e);
        }
      }

      const bootstrappedIosStarter = await startupTracer.measure(
        "maybe bootstrap iOS starter workspace",
        () => maybeBootstrapIosStarterWorkspace(
          apiInstance,
          backendInstance,
          wsName ?? "My Workspace",
        ),
      );
      if (bootstrappedIosStarter) {
        await startupTracer.measure(
          "refreshTree after iOS starter bootstrap",
          () => refreshTree(),
        );
      }

      // Configure permission persistence + provider now that we have a root tree path.
      permissionStore.setPersistenceHandlers({
        getPluginsConfig: () => pluginPermissionsConfig,
        savePluginsConfig: persistPluginPermissionsConfig,
      });

      // Reload workspace-scoped browser assets (appearance + Extism plugins)
      Promise.resolve().then(async () => {
        await reloadWorkspaceScopedBrowserState();
      });

      // Wait for the root entry to finish loading (started above, in parallel
      // with config hydration).
      if (openEntryPromise) {
        await startupTracer.measure("open root entry", () => openEntryPromise!);
      }

      // Run initial validation in the background — not needed for first render
      runValidation();
      startupStatus = "completed";

    } catch (e) {
      if (e instanceof FsaGestureRequiredError) {
        console.warn("[App] FSA needs user gesture to reconnect:", e);
        fsaNeedsReconnect = true;
        startupStatus = "fsa_reconnect_required";
        return;
      }
      if (e instanceof BackendError && e.kind === "WorkspaceDirectoryMissing") {
        console.warn("[App] Workspace directory missing on startup:", e.message);
        workspaceMissing = {
          id: fsaReconnectWsId ?? "",
          name: fsaReconnectWsName ?? "Unknown",
        };
        startupStatus = "workspace_missing";
        return;
      }
      console.error("[App] Initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      startupTracer.logSummary(startupStatus, {
        workspaceId: startupWorkspaceId,
        treePath: tree?.path ?? null,
        currentEntryPath: currentEntry?.path ?? null,
      });
      entryStore.setLoading(false);
    }
  });

  onDestroy(() => {
    unregisterE2EBridge();
    // Cleanup blob URLs
    revokeBlobUrls();
    stopSyncScheduler();
    // Cleanup filesystem event subscription
    cleanupEventSubscription?.();
    cleanupEventSubscription = null;
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
    if (refreshTreeTimeout) {
      clearTimeout(refreshTreeTimeout);
      refreshTreeTimeout = null;
    }
    mobileGestures.cleanup();
    clearMobileFocusChromeRevealTimer();
    // Cleanup import:complete listener
    window.removeEventListener("import:complete", handleImportComplete);
    resetBackend();
  });

  // FSA reconnect: user clicks button (providing the gesture), then we retry init
  async function handleFsaReconnect() {
    fsaNeedsReconnect = false;
    entryStore.setLoading(true);
    try {
      resetBackend();
      const storageType = fsaReconnectWsId ? getWorkspaceStorageType(fsaReconnectWsId) : undefined;
      const backendInstance = await getBackend(fsaReconnectWsId, fsaReconnectWsName, storageType);
      workspaceStore.setBackend(backendInstance);

      const apiInstance = createApi(backendInstance);
      await getPluginStore().init(apiInstance);
      cleanupEventSubscription = initEventSubscription(backendInstance);
      rustApi = null;

      const sharedWorkspaceId = getCurrentWorkspace()?.id ?? null;
      workspaceStore.setWorkspaceId(sharedWorkspaceId);

      await refreshTree();
      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }
      await runValidation();
    } catch (e) {
      console.error("[App] FSA reconnect failed:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  }

  // Workspace switching handlers
  function handleWorkspaceSwitchStart() {
    // Auto-save before switching
    if (isDirty && api && currentEntry && editorRef) {
      cancelAutoSave();
      save();
    }
    // Clear UI state
    entryStore.setCurrentEntry(null);
    workspaceStore.setTree(null);
    workspaceStore.setValidationResult(null);
    entryStore.setLoading(true);
  }

  async function handleWorkspaceSwitchComplete() {
    // Re-initialize references: get the new backend from the singleton
    const newBackend = await getBackend();
    workspaceStore.setBackend(newBackend);
    rustApi = null;
    // Refresh tree and validation from new workspace
    await refreshTree();

    // Hydrate view preferences from the new workspace's config
    const workspaceRootIndexPath = await api?.resolveWorkspaceRootIndexPath(
      tree?.path ?? null,
    );
    if (workspaceRootIndexPath && api) {
      try {
        const wsConfig = await api.getWorkspaceConfig(workspaceRootIndexPath);
        workspaceStore.hydrateDisplaySettings(wsConfig, async (field, value) => {
          try {
            const nextRootIndexPath = await api!.resolveWorkspaceRootIndexPath(
              tree?.path ?? null,
            );
            if (!nextRootIndexPath) return;
            await api!.setWorkspaceConfig(nextRootIndexPath, field, value);
          } catch (e) {
            console.warn('[App] Failed to persist display setting:', field, e);
          }
        });

        // Hydrate theme mode
        themeStore.hydrateThemeMode(wsConfig.theme_mode, async (mode) => {
          try {
            const nextRootIndexPath = await api!.resolveWorkspaceRootIndexPath(
              tree?.path ?? null,
            );
            if (!nextRootIndexPath) return;
            await api!.setWorkspaceConfig(nextRootIndexPath, 'theme_mode', mode);
          } catch (e) {
            console.warn('[App] Failed to persist theme_mode:', e);
          }
        });

        appearanceStore.hydrateWorkspaceTheme(
          {
            presetId: wsConfig.theme_preset,
            accentHue: wsConfig.theme_accent_hue,
          },
          async ({ presetId, accentHue }) => {
            try {
              const nextRootIndexPath = await api!.resolveWorkspaceRootIndexPath(
                tree?.path ?? null,
              );
              if (!nextRootIndexPath) return;
              await api!.setWorkspaceConfig(nextRootIndexPath, 'theme_preset', presetId);
              await api!.setWorkspaceConfig(
                nextRootIndexPath,
                'theme_accent_hue',
                accentHue === null ? 'null' : JSON.stringify(accentHue),
              );
            } catch (e) {
              console.warn('[App] Failed to persist workspace theme selection:', e);
            }
          },
        );

        // Hydrate audience colors
        getAudienceColorStore().hydrate(wsConfig.audience_colors as Record<string, string> | undefined, async (colors) => {
          try {
            const nextRootIndexPath = await api!.resolveWorkspaceRootIndexPath(
              tree?.path ?? null,
            );
            if (!nextRootIndexPath) return;
            await api!.setWorkspaceConfig(nextRootIndexPath, 'audience_colors', JSON.stringify(colors));
          } catch (e) {
            console.warn('[App] Failed to persist audience_colors:', e);
          }
        });

        // Hydrate disabled plugins
        getPluginStore().hydrateDisabledPlugins(wsConfig.disabled_plugins, async (disabledIds) => {
          try {
            const nextRootIndexPath = await api!.resolveWorkspaceRootIndexPath(
              tree?.path ?? null,
            );
            if (!nextRootIndexPath) return;
            await api!.setWorkspaceConfig(nextRootIndexPath, 'disabled_plugins', JSON.stringify(disabledIds));
          } catch (e) {
            console.warn('[App] Failed to persist disabled_plugins:', e);
          }
        });

        if (wsConfig.show_unlinked_files || wsConfig.show_hidden_files) {
          await refreshTree();
        }
      } catch (e) {
        console.warn('[App] Failed to load workspace config:', e);
      }
    }

    await reloadWorkspaceScopedBrowserState();
    // Navigate to the root entry of the new workspace
    if (tree && !currentEntry) {
      workspaceStore.expandNode(tree.path);
      await openEntry(tree.path);
    }
    await runValidation();
    entryStore.setLoading(false);
  }

  // Open an entry - thin wrapper that handles auto-save and delegates to controller
  const runLatestOpenEntry = createLatestOnlyRunner<{ path: string; force: boolean }>(async ({ path, force }) => {
    if (!api || !backend) return;

    try {
      // Auto-save before switching documents. The latest-only runner ensures
      // rapid navigation collapses to the final target instead of flooding the backend.
      if (isDirty) {
        cancelAutoSave();
        await save();
      }

      // Skip the reload when we're already on this entry and the caller didn't
      // ask to force-refresh. `force` exists for paths like onPluginsReady where
      // the editor was rebuilt and we need disk truth to flow back into
      // displayContent even though the path hasn't changed.
      if (!force && currentEntry?.path === path && !isDirty) {
        return;
      }

      await openEntryController(api, path, tree, collaborationEnabled);
    } finally {
      if (pendingEntryPath === path) {
        pendingEntryPath = null;
      }
    }
  });

  async function openEntry(path: string, options: { force?: boolean } = {}) {
    pendingEntryPath = path;
    await runLatestOpenEntry({ path, force: options.force ?? false });
  }

  // Save current entry - delegates to controller with sync support
  // detectH1Title: when true, backend detects first-line H1 and syncs to title/filename
  async function save(detectH1Title = false) {
    if (!api || !currentEntry || !editorRef) return;
    const result = await saveEntryWithSync(api, currentEntry, editorRef, tree?.path, detectH1Title);
    if (result?.newPath) {
      // H1→title sync caused a path change — update UI state
      const entry = entryStore.currentEntry;
      if (entry) {
        if (expandedNodes.has(entry.path)) {
          workspaceStore.collapseNode(entry.path);
          workspaceStore.expandNode(result.newPath);
        }
        // Re-fetch the entry to get updated frontmatter (title may have changed)
        try {
          const updatedEntry = await api.getEntry(result.newPath);
          entryStore.setCurrentEntry(updatedEntry);
          if (collaborationEnabled) {
            collaborationStore.setCollaborationPath(toCollaborationPath(result.newPath, tree?.path ?? ""));
          }
        } catch {
          // Fallback: just update the path
          entryStore.setCurrentEntry({ ...entry, path: result.newPath });
        }
        await refreshTree();
      }
    }
  }

  // Cancel pending auto-save
  function cancelAutoSave() {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
  }

  // Schedule auto-save with debounce (no H1 detection)
  function scheduleAutoSave() {
    cancelAutoSave();
    autoSaveTimer = setTimeout(() => {
      autoSaveTimer = null;
      if (isDirty) {
        save(false);
      }
    }, AUTO_SAVE_DELAY_MS);
  }

  // Handle content changes - triggers debounced auto-save
  // Sync propagation is handled by plugin-owned filesystem event processing.
  // We skip markdown serialization here to avoid O(doc) work on every keystroke;
  // the actual serialization happens once when the debounced save fires.
  function handleContentChange() {
    if (!isDirty) {
      entryStore.markDirty();
    }
    scheduleAutoSave();
  }

  // Handle editor blur - save immediately with H1 detection if dirty
  function handleEditorBlur() {
    cancelAutoSave();
    if (isDirty) {
      save(true);
    }
  }

  // Toggle node expansion
  function toggleNode(path: string) {
    // Use store method to ensure expanded state persists across tree refreshes
    workspaceStore.toggleNode(path);
  }

  // Sidebar toggles
  function toggleLeftSidebar() {
    uiStore.toggleLeftSidebar();
    leftEdgeHovered = false;

  }

  function toggleRightSidebar() {
    uiStore.toggleRightSidebar();

  }

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save(true);
    }
    // Command palette with Cmd/Ctrl + K
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      uiStore.openCommandPalette();
    }
    // Find in file with Cmd/Ctrl + F
    if ((event.metaKey || event.ctrlKey) && event.key === "f") {
      event.preventDefault();
      showFindBar = true;
    }
    // Toggle left sidebar with Cmd/Ctrl + [ (bracket)
    if ((event.metaKey || event.ctrlKey) && event.key === "[") {
      event.preventDefault();
      toggleLeftSidebar();
    }
    // Toggle right sidebar with Cmd/Ctrl + ]
    if ((event.metaKey || event.ctrlKey) && event.key === "]") {
      event.preventDefault();
      toggleRightSidebar();
    }
    // Open settings:
    // - Tauri: Cmd/Ctrl + , (standard desktop convention)
    // - Web: Cmd/Ctrl + Shift + , (to avoid browser settings conflict)
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      if (isTauri() || event.shiftKey) {
        event.preventDefault();
        showSettingsDialog = true;
      }
    }
  }

  /**
   * Handle generic host actions requested by plugin iframes.
   */
  async function enterGuestWorkspace(sessionCode: string): Promise<{ entered: true }> {
    if (!sessionCode.trim()) {
      throw new Error("enter-guest-workspace requires a session code");
    }

    workspaceStore.saveTreeState();
    entryStore.setCurrentEntry(null);
    workspaceStore.setTree(null);

    if (isTauri()) {
      if (!backend?.startGuestMode) {
        throw new Error("Guest mode is unavailable for this backend");
      }
      await backend.startGuestMode(sessionCode);
      cleanupEventSubscription?.();
      cleanupEventSubscription = initEventSubscription(backend);
      await refreshTree();
      return { entered: true };
    }

    if (!backend) {
      throw new Error("Workspace backend is unavailable");
    }

    const currentWorkspaceId = getCurrentWorkspaceId();
    const currentWorkspace = currentWorkspaceId
      ? getLocalWorkspace(currentWorkspaceId)
      : null;
    guestWorkspaceState = {
      previousBackend: backend,
      previousWorkspaceId: currentWorkspaceId,
      previousWorkspaceName: currentWorkspace?.name ?? null,
      previousStorageType: currentWorkspaceId
        ? getWorkspaceStorageType(currentWorkspaceId)
        : undefined,
    };

    const { createGuestBackend } = await import("$lib/backend/workerBackendNew");
    const guestBackend = await createGuestBackend();

    cleanupEventSubscription?.();
    replaceBackend(guestBackend);
    workspaceStore.setBackend(guestBackend);
    cleanupEventSubscription = initEventSubscription(guestBackend);
    await refreshTree();
    return { entered: true };
  }

  async function leaveGuestWorkspace(): Promise<{ left: true }> {
    if (isTauri()) {
      if (backend?.endGuestMode) {
        await backend.endGuestMode();
        cleanupEventSubscription?.();
        cleanupEventSubscription = initEventSubscription(backend);
        await refreshTree();
      }
      workspaceStore.restoreTreeState();
      return { left: true };
    }

    if (!guestWorkspaceState?.previousBackend) {
      workspaceStore.restoreTreeState();
      return { left: true };
    }

    cleanupEventSubscription?.();
    replaceBackend(
      guestWorkspaceState.previousBackend,
      guestWorkspaceState.previousWorkspaceId ?? undefined,
      guestWorkspaceState.previousWorkspaceName ?? undefined,
      guestWorkspaceState.previousStorageType,
    );
    workspaceStore.setBackend(guestWorkspaceState.previousBackend);
    cleanupEventSubscription = initEventSubscription(guestWorkspaceState.previousBackend);
    guestWorkspaceState = null;
    await refreshTree();
    workspaceStore.restoreTreeState();
    return { left: true };
  }

  async function handlePluginHostAction(action: { type: string; payload?: unknown }) {
    const actionType = action?.type;
    const payload = (action?.payload ?? {}) as Record<string, unknown>;

    if (isStandardPluginHostUiAction(actionType)) {
      return handleStandardPluginHostUiAction(action, {
        showToast: ({ message, description, variant }) => {
          switch (variant) {
            case "success":
              toast.success(message, { description });
              break;
            case "warning":
              toast.warning(message, { description });
              break;
            case "error":
              toast.error(message, { description });
              break;
            default:
              toast.info(message, { description });
              break;
          }
        },
        confirm: async ({ title, description, confirmLabel, cancelLabel, variant }) => {
          if (pluginConfirmResolve || pluginPromptResolve) {
            throw new Error("Another plugin dialog is already open");
          }

          pluginConfirmTitle = title;
          pluginConfirmDescription = description;
          pluginConfirmConfirmLabel = confirmLabel;
          pluginConfirmCancelLabel = cancelLabel;
          pluginConfirmVariant = variant;
          showPluginConfirmDialog = true;

          return await new Promise<boolean>((resolve) => {
            pluginConfirmResolve = resolve;
          });
        },
        prompt: async ({
          title,
          description,
          confirmLabel,
          cancelLabel,
          variant,
          value,
          placeholder,
        }) => {
          if (pluginConfirmResolve || pluginPromptResolve) {
            throw new Error("Another plugin dialog is already open");
          }

          pluginPromptTitle = title;
          pluginPromptDescription = description;
          pluginPromptConfirmLabel = confirmLabel;
          pluginPromptCancelLabel = cancelLabel;
          pluginPromptVariant = variant;
          pluginPromptValue = value;
          pluginPromptPlaceholder = placeholder;
          showPluginPromptDialog = true;

          await tick();
          pluginPromptInput?.focus();
          pluginPromptInput?.select();

          return await new Promise<string | null>((resolve) => {
            pluginPromptResolve = resolve;
          });
        },
      });
    }

    switch (actionType) {
      case "get-workspace-tree": {
        if (!api) throw new Error("Workspace API is unavailable");
        if (tree) return JSON.parse(JSON.stringify(tree));
        return api.getWorkspaceTree();
      }
      case "create-ai-conversation-entry": {
        const title = typeof payload.title === "string" ? payload.title.trim() : "";
        const parentPath =
          typeof payload.parentPath === "string" ? payload.parentPath : null;
        if (!title) {
          throw new Error(
            "host-action create-ai-conversation-entry requires payload.title",
          );
        }
        return createAiConversationEntry(title, parentPath);
      }
      case "delete-entry": {
        const path = typeof payload.path === "string" ? payload.path : null;
        if (!path) throw new Error("host-action delete-entry requires payload.path");
        if (!api) throw new Error("Workspace API is unavailable");
        const parentPath = workspaceStore.getParentNodePath(path);
        const deleted = await deleteEntryWithSync(
          api,
          path,
          currentEntry?.path ?? null,
          async () => {
            await refreshTree();
            if (parentPath) {
              await loadNodeChildren(parentPath);
            }
            await runValidation();
          },
        );
        if (!deleted) throw new Error(`Failed to delete entry: ${path}`);
        return { deleted: path };
      }
      case "open-entry": {
        const path = typeof payload.path === "string" ? payload.path : null;
        if (!path) throw new Error("host-action open-entry requires payload.path");
        await openEntry(path);
        return { opened: path };
      }
      case "refresh-tree":
        await refreshTree();
        return { refreshed: true };
      case "open-settings": {
        const tab = typeof payload.tab === "string" ? payload.tab : undefined;
        settingsInitialTab = tab;
        showSettingsDialog = true;
        return { opened: "settings", tab: tab ?? null };
      }
      case "open-audience-manager":
        audiencePanelStore.openPanel();
        return { opened: "audience-manager" };
      case "open-marketplace":
        showSettingsDialog = false;
        showMarketplaceDialog = true;
        return { opened: "marketplace" };
      case "open-export-dialog":
        exportPath = tree?.path ?? ".";
        showExportDialog = true;
        return { opened: "export-dialog", path: exportPath };
      case "toggle-left-sidebar":
        toggleLeftSidebar();
        return { toggled: "left" };
      case "toggle-right-sidebar":
        toggleRightSidebar();
        return { toggled: "right" };
      case "open-oauth": {
        return openOauthWindow({
          url: typeof payload.url === "string" ? payload.url : "",
          redirect_uri_prefix:
            typeof payload.redirect_uri_prefix === "string"
              ? payload.redirect_uri_prefix
              : undefined,
        });
      }
      case "enter-guest-workspace": {
        const sessionCode =
          typeof payload.session_code === "string"
            ? payload.session_code
            : typeof payload.join_code === "string"
              ? payload.join_code
              : "";
        return enterGuestWorkspace(sessionCode);
      }
      case "leave-guest-workspace":
        return leaveGuestWorkspace();
      default:
        throw new Error(`Unknown host action: ${actionType ?? "undefined"}`);
    }
  }

  function resolvePluginConfirm(result: boolean) {
    showPluginConfirmDialog = false;
    const resolve = pluginConfirmResolve;
    pluginConfirmResolve = null;
    resolve?.(result);
  }

  function resolvePluginPrompt(result: string | null) {
    showPluginPromptDialog = false;
    const resolve = pluginPromptResolve;
    pluginPromptResolve = null;
    resolve?.(result);
  }

  /**
   * Handle magic link token verification from URL.
   * Verifies the token automatically and updates plugin-owned sync status surfaces.
   */
  async function handleMagicLinkToken(token: string) {
    // Show connecting status while verifying
    collaborationStore.setSyncStatus('connecting');

    try {
      // Verify the magic link token
      // Note: URL token is cleared before this function is called to prevent double verification
      await verifyMagicLink(token);

      // Set status to idle; sync plugin status updates to 'synced' when connected.
      collaborationStore.setSyncStatus('idle');

      // Show success toast. If sync isn't set up yet (new device), the wizard will
      // open automatically after initialization — avoid the misleading "now syncing" message.
      if (!isSyncEnabled() && getWorkspaces().length > 0) {
        toast.success("Signed in successfully", {
          description: "Downloading your workspace from server...",
        });
      } else {
        toast.success("Signed in successfully", {
          description: "Your workspace is now syncing.",
        });
      }

    } catch (error) {
      console.error("[App] Magic link verification failed:", error);
      collaborationStore.setSyncStatus('error');
      collaborationStore.setSyncError(
        error instanceof Error ? error.message : "Verification failed"
      );
      toast.error("Sign in failed", {
        description: error instanceof Error ? error.message : "Could not verify magic link",
      });
    }
  }

  // Create a child entry - delegates to controller with sync support
  async function handleCreateChildEntry(parentPath: string) {
    if (!api) return;
    await createChildEntryWithSync(api, parentPath, async (result) => {
      await refreshTree();
      // Use result.parent_path (may differ from original if parent was converted to index)
      await loadNodeChildren(result.parent_path);
      await openEntry(result.child_path);
      await runValidation();
    });
  }

  // Create a new entry - delegates to controller with sync support
  async function createNewEntry(title: string, parentPath: string | null) {
    if (!api) return;

    // Generate filename from title using Rust backend (reads workspace config's filename_style)
    const filename = await api.generateFilename(title, tree?.path ?? undefined);

    // Create the entry at workspace root
    const newPath = await createEntryWithSync(api, filename, { title, rootIndexPath: tree?.path }, async () => {
      await refreshTree();
    });

    if (newPath && parentPath) {
      // Attach to selected parent
      try {
        const movedPath = await api.attachEntryToParent(newPath, parentPath);
        await refreshTree();
        await openEntry(movedPath);
        await runValidation();
        return;
      } catch (e) {
        toast.error("Entry created but failed to attach to parent");
        console.error("[App] attachEntryToParent failed:", e);
      }
    }

    if (newPath) {
      await openEntry(newPath);
      await runValidation();
    }
  }

  async function createAiConversationEntry(title: string, parentPath: string | null) {
    if (!api) throw new Error("Workspace API is unavailable");
    if (!tree?.path) throw new Error("A workspace must be open to create AI conversations");

    const filename = await api.generateFilename(title, tree.path);
    const newPath = await createEntryWithSync(
      api,
      filename,
      { title, rootIndexPath: tree.path },
      async () => {
        await refreshTree();
      },
    );

    if (!newPath) {
      throw new Error("Failed to create the conversation file");
    }

    let finalPath = newPath;
    if (parentPath) {
      try {
        finalPath = await api.attachEntryToParent(newPath, parentPath);
        await refreshTree();
      } catch (e) {
        toast.error("Conversation file created but failed to attach to parent");
        console.error("[App] attachEntryToParent failed for AI conversation:", e);
      }
    }

    await runValidation();
    return { path: finalPath, title };
  }

  // Initialize an empty workspace with a root index
  async function handleInitializeWorkspace() {
    if (!api || !backend) return;
    try {
      // Get workspace name from local registry
      const wsId = getCurrentWorkspaceId();
      const localWs = wsId ? getLocalWorkspace(wsId) : null;
      const wsName = localWs?.name ?? "My Journal";
      // Use the actual workspace directory, not "." (which is CWD on Tauri)
      const workspaceDir = getWorkspaceDirectoryPath(backend.getWorkspacePath());
      await api.createWorkspace(workspaceDir, wsName);
      await refreshTree();
      // Open the newly created root index
      if (tree) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }


  // Handle welcome screen completion — delegates to onboardingController
  async function handleWelcomeComplete(_id: string, _name: string) {
    showWelcomeScreen = false;
    entryStore.setLoading(true);

    try {
      await handleWelcomeCompleteController(
        {
          getBackend: () => getBackend(),
          setBackend: (b) => workspaceStore.setBackend(b),
          clearRustApi: () => { rustApi = null; },
          refreshTree,
          getTree: () => tree,
          getCurrentEntry: () => currentEntry,
          expandNode: (path) => workspaceStore.expandNode(path),
          openEntry,
          runValidation,
        },
        _id,
        _name,
      );
    } catch (e) {
      console.error("[App] Post-welcome initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  }

  // getWorkspaceDirectoryPath, isWorkspaceAlreadyExistsError, seedStarterWorkspaceContent,
  // maybeBootstrapIosStarterWorkspace, autoCreateDefaultWorkspace, and applyOnboardingBundle
  // are now imported from onboardingController.

  /** Build the AutoCreateWorkspaceDeps used by onboarding controller functions. */
  function buildAutoCreateDeps(): AutoCreateWorkspaceDeps {
    return {
      createLocalWorkspace,
      setCurrentWorkspaceId,
      getBackend: (id, name, storageType) => getBackend(id, name, storageType),
      createApi,
      setBackend: (b) => workspaceStore.setBackend(b),
      clearRustApi: () => { rustApi = null; },
      initEventSubscription,
      setCleanupEventSubscription: (cleanup) => { cleanupEventSubscription = cleanup; },
      refreshTree,
      setupPermissions: () => {
        permissionStore.setPersistenceHandlers({
          getPluginsConfig: () => pluginPermissionsConfig,
          savePluginsConfig: persistPluginPermissionsConfig,
        });
        browserPlugins.setPluginPermissionConfigProvider(() => pluginPermissionsConfig);
        browserPlugins.setPluginPermissionConfigPersistor(
          persistRequestedPluginPermissionDefaults,
        );
      },
      persistPermissionDefaults: persistRequestedPluginPermissionDefaults,
    };
  }

  /** Thin wrapper that delegates to the onboarding controller. */
  async function autoCreateDefaultWorkspace(
    bundle?: BundleRegistryEntry | null,
  ): Promise<{ id: string; name: string }> {
    return autoCreateDefaultWorkspaceController(buildAutoCreateDeps(), bundle);
  }

  // Rename an entry by title - uses SetFrontmatterProperty which handles title→filename→H1 sync
  async function handleRenameEntry(path: string, newTitle: string): Promise<string> {
    if (!api) throw new Error("API not initialized");
    if (currentEntry?.path === path && isDirty && editorRef) {
      await saveEntryWithSync(api, currentEntry, editorRef, tree?.path);
    }

    // Use SetFrontmatterProperty to set title, which atomically handles filename rename + H1 sync
    const newPath = await api.setFrontmatterProperty(path, "title", newTitle, tree?.path);
    const effectivePath = newPath ?? path;

    await refreshTree();
    const parentPath = workspaceStore.getParentNodePath(path);
    if (parentPath) {
      await loadNodeChildren(parentPath);
    }
    await runValidation();

    if (currentEntry?.path === path) {
      // Re-fetch entry to get updated frontmatter and body (H1 synced by backend)
      try {
        const refreshed = await api.getEntry(effectivePath);
        refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
        entryStore.setCurrentEntry(refreshed);
        entryStore.setDisplayContent(refreshed.content);
      } catch {
        entryStore.setCurrentEntry({
          ...currentEntry,
          path: effectivePath,
          frontmatter: { ...currentEntry.frontmatter, title: newTitle },
        });
      }
      if (collaborationEnabled) {
        collaborationStore.setCollaborationPath(toCollaborationPath(effectivePath, tree?.path ?? ""));
      }
    }

    return effectivePath;
  }

  // Duplicate an entry - delegates to controller with sync support
  async function handleDuplicateEntry(path: string): Promise<string> {
    if (!api) throw new Error("API not initialized");
    const parentPath = workspaceStore.getParentNodePath(path);
    const newPath = await duplicateEntryController(api, path, async () => {
      await refreshTree();
      if (parentPath) {
        await loadNodeChildren(parentPath);
      }
      await runValidation();
    });
    return newPath;
  }

  async function openDeleteConfirm(paths: string[]) {
    const uniquePaths = Array.from(new Set(paths));
    pendingDeletePaths = uniquePaths;
    pendingDeleteIncludesDescendants = api ? await api.checkDeleteIncludesDescendants(uniquePaths, tree?.path ?? undefined) : false;
    showDeleteConfirm = true;
  }

  async function buildDeletePlan(paths: string[]): Promise<string[]> {
    if (!api) return [];
    return api.prepareMultiDelete(paths, tree?.path ?? undefined);
  }

  // Delete an entry - shows confirmation dialog, then delegates to controller
  async function handleDeleteEntry(path: string) {
    if (!api) return;
    await openDeleteConfirm([path]);
  }

  async function handleDeleteEntries(paths: string[]) {
    if (!api || paths.length === 0) return;
    await openDeleteConfirm(paths);
  }

  // Called when user confirms deletion in the dialog
  async function confirmDeleteEntry() {
    if (!api || pendingDeletePaths.length === 0 || isDeletingEntries) return;

    isDeletingEntries = true;
    const requestedPaths = [...pendingDeletePaths];
    const parentPaths = new Set(
      requestedPaths
        .map((path) => workspaceStore.getParentNodePath(path))
        .filter((path): path is string => Boolean(path)),
    );

    try {
      const deletePlan = await buildDeletePlan(requestedPaths);
      let deletedCount = 0;

      for (const path of deletePlan) {
        const deleted = await deleteEntryWithSync(
          api,
          path,
          currentEntry?.path ?? null,
        );
        if (!deleted) break;
        deletedCount++;
      }

      if (deletedCount > 0) {
        await refreshTree();
        for (const parentPath of parentPaths) {
          await loadNodeChildren(parentPath);
        }
        await runValidation();
      }

      if (deletedCount === deletePlan.length && deletePlan.length > 0) {
        toast.success(
          deletePlan.length === 1
            ? "Entry deleted"
            : `Deleted ${deletePlan.length} entries`,
        );
      } else if (deletedCount > 0) {
        toast.warning(
          `Deleted ${deletedCount} of ${deletePlan.length} requested entries`,
        );
      }
    } finally {
      isDeletingEntries = false;
      showDeleteConfirm = false;
      pendingDeletePaths = [];
      pendingDeleteIncludesDescendants = false;
    }
  }

  // Called when user cancels deletion
  function cancelDeleteEntry() {
    if (isDeletingEntries) return;
    showDeleteConfirm = false;
    pendingDeletePaths = [];
    pendingDeleteIncludesDescendants = false;
  }

  // Open audience dialog for a tree entry (or full-screen manager if no audiences exist)
  async function handleSetAudience(path: string) {
    if (!api) return;
    try {
      const available = await api.getAvailableAudiences(tree?.path ?? "");
      if (available.length === 0) {
        // No audiences exist — open the audience panel
        audiencePanelStore.openPanel();
        return;
      }
      const entry = await api.getEntry(path);
      const fm = normalizeFrontmatter(entry.frontmatter);
      audienceDialogPath = path;
      audienceDialogAudience = Array.isArray(fm.audience) ? fm.audience : null;
      showAudienceDialog = true;
    } catch (e) {
      console.error("[App] Failed to load entry for audience dialog:", e);
      toast.error("Failed to load entry");
    }
  }

  /** Paint-mode: toggle each active brush audience on an entry's frontmatter.
   *  Clear brush empties the list; otherwise each picked brush XORs against
   *  the entry's current audience set in pick order. */
  async function handlePaintEntry(entryPath: string) {
    if (!api) return;
    const brushes = audiencePanelStore.paintBrushes;
    if (brushes.length === 0) return;

    const rootPath = tree?.path ?? "";
    const isClear = brushes.length === 1 && brushes[0] === "__clear__";

    try {
      const fm = await api.getFrontmatter(entryPath);
      const current: string[] = Array.isArray(fm.audience) ? (fm.audience as string[]) : [];

      let updated: string[];
      if (isClear) {
        updated = [];
      } else {
        updated = [...current];
        for (const brush of brushes) {
          const idx = updated.indexOf(brush);
          if (idx !== -1) {
            updated.splice(idx, 1);
          } else {
            updated.push(brush);
          }
        }
      }

      // Optimistic UI: update the tree node immediately
      workspaceStore.updateNodeAudience(entryPath, updated);

      if (updated.length === 0) {
        await api.removeFrontmatterProperty(entryPath, "audience");
      } else {
        await api.setFrontmatterProperty(entryPath, "audience", updated, rootPath);
      }

      // If the active brush is a transient (newly-typed) audience, this paint
      // is what makes it real. Mark it persisted and refresh the panel's list.
      audiencePanelStore.notePainted();
      templateContextStore.bumpAudiencesVersion();
    } catch (e) {
      console.error("[App] Paint entry failed:", e);
      toast.error("Failed to update audience");
    }
  }

  async function handleAudienceChange(value: string[] | null) {
    if (!api || !audienceDialogPath) return;
    try {
      if (value === null) {
        // Remove audience property to revert to inheritance
        await api.setFrontmatterProperty(audienceDialogPath, "audience", null as any, tree?.path);
      } else {
        await api.setFrontmatterProperty(audienceDialogPath, "audience", value, tree?.path);
      }
      audienceDialogAudience = value;
      // Refresh tree in case audience filter is active
      await refreshTree();
      // If this is the currently open entry, refresh it
      if (currentEntry?.path === audienceDialogPath) {
        const refreshed = await api.getEntry(audienceDialogPath);
        refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
        entryStore.setCurrentEntry(refreshed);
      }
    } catch (e) {
      console.error("[App] Failed to update audience:", e);
      toast.error("Failed to update audience");
    }
  }

  // Run workspace validation (delegates to controller)
  async function runValidation() {
    if (!api || !backend) return;
    await runValidationController(api, backend, tree);
  }

  // Validate a specific path (delegates to controller)
  async function handleValidate(path: string) {
    if (!api) return;
    await validatePath(api, path);
  }

  // Quick fix: Remove broken part_of reference from a file
  async function handleRemoveBrokenPartOf(filePath: string) {
    if (!api) return;
    try {
      await api.removeFrontmatterProperty(filePath, "part_of");
      await runValidation();
      // Refresh current entry if it's the fixed file
      if (currentEntry?.path === filePath) {
        const refreshed = await api.getEntry(filePath);
        refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
        entryStore.setCurrentEntry(refreshed);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Quick fix: Remove broken entry from an index's contents
  async function handleRemoveBrokenContentsRef(indexPath: string, target: string) {
    if (!api) return;
    try {
      // Get current contents
      const entry = await api.getEntry(indexPath);
      const contents = entry.frontmatter?.contents;
      if (Array.isArray(contents)) {
        // Filter out the broken target
        const newContents = contents.filter((item) => item !== target);
         await api.setFrontmatterProperty(indexPath, "contents", newContents, tree?.path);
        await refreshTree();
        await runValidation();
        // Refresh current entry if it's the fixed file
        if (currentEntry?.path === indexPath) {
          const refreshed = await api.getEntry(indexPath);
          refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
          entryStore.setCurrentEntry(refreshed);
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Quick fix: Attach an unlinked entry to the workspace root
  async function handleAttachUnlinkedEntry(entryPath: string) {
    if (!api || !tree) return;
    try {
       // Attach to the workspace root (tree.path is the root index)
      await api.attachEntryToParent(entryPath, tree.path);
      await refreshTree();
      await runValidation();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Wrapper functions that delegate to controllers
  async function refreshTree() {
    if (!api || !backend) return;
    const audiences = templateContextStore.previewAudience ?? undefined;
    const startedAt = getTimingNow();
    const treeRefreshStartedAt = getTimingNow();
    await refreshTreeController(api, backend, showUnlinkedFiles, showHiddenFiles, audiences);
    const treeRefreshElapsedMs = getElapsedMs(treeRefreshStartedAt);

    const permissionsReloadStartedAt = getTimingNow();
    await reloadPluginPermissionsConfig();
    const permissionsReloadElapsedMs = getElapsedMs(permissionsReloadStartedAt);

    console.info("[WorkspaceRefresh] completed", {
      audiences: audiences ?? null,
      showUnlinkedFiles,
      showHiddenFiles,
      treePath: workspaceStore.tree?.path ?? null,
      treeRefreshElapsedMs,
      permissionsReloadElapsedMs,
      totalElapsedMs: getElapsedMs(startedAt),
    });
  }

  // Handle import:complete event from ImportSettings
  function handleImportComplete() {
    // Force a fresh tree replacement after import so removed nodes aren't
    // preserved by merge heuristics from the pre-import tree.
    workspaceStore.setTree(null);
    void (async () => {
      await refreshTree();
      debouncedRefreshTree();
    })();
  }

  // Debounced version of refreshTree to prevent rapid refreshes during sync
  function debouncedRefreshTree() {
    if (refreshTreeTimeout) clearTimeout(refreshTreeTimeout);
    refreshTreeTimeout = setTimeout(async () => {
      refreshTreeTimeout = null;
      await refreshTree();
    }, REFRESH_TREE_DEBOUNCE_MS);
  }

  async function loadNodeChildren(nodePath: string) {
    if (!api) return;
    const audiences = templateContextStore.previewAudience ?? undefined;
    await loadNodeChildrenController(api, nodePath, showUnlinkedFiles, showHiddenFiles, audiences);
  }

  // ========================================================================
  // Import handlers used by fallback command UIs
  // ========================================================================

  async function handleImportFromClipboard() {
    if (!api) return;
    await importFromClipboardHandler(api, tree, refreshTree, openEntry);
  }

  async function handleImportMarkdownFile() {
    if (!api) return;
    await importMarkdownFileHandler(api, tree, currentEntry?.path ?? null, refreshTree, openEntry);
  }

  async function handleQuickBackupExport() {
    if (!tree?.path || !backend) {
      toast.error("No workspace loaded");
      return;
    }

    try {
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        const workspaceDir = tree.path.substring(0, tree.path.lastIndexOf("/"));
        const result = await invoke("export_to_zip", { workspacePath: workspaceDir });

        if (result?.cancelled) return;
        if (!result?.success) {
          throw new Error(result?.error || "Export failed");
        }

        toast.success(`Backup exported (${result.files_exported ?? 0} files)`);
        return;
      }

      const JSZip = (await import("jszip")).default;

      const workspaceDir = tree.path.substring(0, tree.path.lastIndexOf("/"));
      const filesystemTree = await api!.getFilesystemTree(workspaceDir, false);

      const zip = new JSZip();
      const reader = {
        readText: (path: string) => api!.readFile(path),
        readBinary: (path: string) => (backend as any).readBinary(path) as Promise<Uint8Array>,
      };

      const fileCount = await addFilesToZip(zip, filesystemTree, workspaceDir, reader);
      const blob = await zip.generateAsync({ type: "blob" });
      const url = URL.createObjectURL(blob);

      const downloadLink = document.createElement("a");
      downloadLink.href = url;
      const baseName = tree.path.split("/").pop()?.replace(".md", "") || "workspace";
      const timestamp = new Date().toISOString().slice(0, 10);
      downloadLink.download = `${baseName}-${timestamp}.zip`;
      document.body.appendChild(downloadLink);
      downloadLink.click();
      document.body.removeChild(downloadLink);
      URL.revokeObjectURL(url);

      toast.success(`Backup exported (${fileCount} files)`);
    } catch (error) {
      console.error("[App] Quick backup export failed:", error);
      toast.error(error instanceof Error ? error.message : String(error));
    }
  }

  // ========================================================================
  // Command Palette Handlers - Parameterless wrappers for current entry
  // ========================================================================

  async function cmdDuplicateEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    const newPath = await handleDuplicateEntry(currentEntry.path);
    await openEntry(newPath);
    toast.success("Entry duplicated");
  }

  function cmdRenameEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    const currentTitle = (typeof currentEntry.frontmatter?.title === "string" ? currentEntry.frontmatter.title : null)
      || currentEntry.path.split("/").pop()?.replace(".md", "") || "";
    const entryPath = currentEntry.path;
    promptDialog = {
      title: "Rename Entry",
      label: "New title",
      value: currentTitle,
      onSubmit: async (newTitle: string) => {
        if (!newTitle || newTitle === currentTitle) return;
        await handleRenameEntry(entryPath, newTitle);
      },
    };
  }

  function cmdDeleteEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    handleDeleteEntry(currentEntry.path);
  }

  function cmdMoveEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    moveEntryDialogPath = currentEntry.path;
  }

  async function cmdCreateChildEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    await handleCreateChildEntry(currentEntry.path);
  }

  async function cmdValidateWorkspace() {
    if (!api) return;
    await handleValidateWorkspace(api, tree, backend);
  }

  function cmdWordCount() {
    handleWordCount(editorRef, currentEntry);
  }

  async function cmdCopyAsMarkdown() {
    await handleCopyAsMarkdown(editorRef, currentEntry);
  }

  function cmdViewMarkdown() {
    const result = handleViewMarkdown(editorRef, currentEntry);
    if (result) {
      markdownPreviewBody = result.body;
      markdownPreviewFrontmatter = result.frontmatter;
      markdownPreviewOpen = true;
    }
  }

  function cmdReorderFootnotes() {
    handleReorderFootnotes(editorRef);
  }

  // ========================================================================
  // Command Registry — central source of truth for command palette + footer
  // ========================================================================

  /** Opens a native file picker filtered to images/videos, uploads, and inserts into the editor. */
  function cmdUploadMedia() {
    if (!api || !currentEntry || !editorRef) return;
    // Snapshot whether the editor has a meaningful cursor position before the
    // command palette closes and the file picker steals focus.
    const editor = editorRef.getEditor?.();
    const hadUserCursor =
      editor != null && editor.state.selection.anchor > 1;

    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*,video/*";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      // If the user never placed a cursor, move to the end of the document
      // so the media doesn't land at the very top.
      if (!hadUserCursor) {
        editorRef.getEditor?.()?.commands.focus("end");
      }
      const result = await handleEditorFileDrop(file);
      if (result) {
        handleAttachmentInsert({
          path: result.attachmentPath,
          kind: result.kind,
          blobUrl: result.blobUrl || undefined,
          sourceEntryPath: currentEntry!.path,
        });
      }
    };
    input.click();
  }

  const pluginBlockCommands = $derived(getPluginStore().editorInsertCommands.block);
  const pluginBlockPickerItems = $derived(getPluginStore().blockPickerItems);

  const commandRegistry = $derived.by(() =>
    buildCommandRegistry({
      getEditor: () => editorRef?.getEditor?.() ?? null,
      hasEntry: !!currentEntry,
      hasEditor: !!editorRef,
      readonly: editorReadonly,
      onUploadMedia: cmdUploadMedia,
      onDuplicateEntry: cmdDuplicateEntry,
      onRenameEntry: cmdRenameEntry,
      onDeleteEntry: cmdDeleteEntry,
      onMoveEntry: cmdMoveEntry,
      onCreateChildEntry: cmdCreateChildEntry,
      onFindInFile: () => { showFindBar = true; },
      onWordCount: cmdWordCount,
      onCopyAsMarkdown: cmdCopyAsMarkdown,
      onViewMarkdown: cmdViewMarkdown,
      onReorderFootnotes: cmdReorderFootnotes,
      onOpenWorkspaceSettings: () => {
        settingsInitialTab = "workspace";
        showSettingsDialog = true;
      },
      onRefreshTree: refreshTree,
      onValidateWorkspace: cmdValidateWorkspace,
      onOpenBackupImport: handleQuickBackupExport,
      onImportFromClipboard: handleImportFromClipboard,
      onImportMarkdownFile: handleImportMarkdownFile,
      onSyncNow: runManualSyncNow,
      isSyncAvailable: () => {
        const wsId = getCurrentWorkspaceId();
        return !!wsId && isWorkspaceProviderSyncEnabled(wsId);
      },
      pluginCommandPaletteItems: getPluginStore().commandPaletteItems as Array<{
        pluginId: string;
        contribution: { id: string; label: string; group: string | null; plugin_command: string };
      }>,
      dispatchPluginCommand: async (pluginId: string, command: string, params?: unknown) => {
        if (isTauri()) {
          try {
            const backend = workspaceStore.backend ?? await getBackend();
            const tauriApi = createApi(backend);
            const data = await tauriApi.executePluginCommand(pluginId, command, params ?? {});
            return { success: true, data };
          } catch (e) {
            return { success: false, error: e instanceof Error ? e.message : String(e) };
          }
        }
        return await browserPlugins.dispatchCommand(pluginId, command, params ?? {});
      },
      pluginBlockCommands,
      pluginBlockPickerItems,
    }),
  );

  // ========================================================================
  // Attachment Handlers - Thin wrappers that delegate to controllers
  // ========================================================================

  function handleAddAttachment(entryPath: string) {
    addAttachmentHandler(entryPath, attachmentFileInput);
  }

  async function handleAttachmentFileSelect(event: Event) {
    if (!api) return;
    await attachmentFileSelectHandler(event, api, currentEntry, editorRef);
  }

  async function handleEditorFileDrop(
    file: File,
  ): Promise<{ blobUrl: string; attachmentPath: string; kind: AttachmentMediaKind } | null> {
    if (!api) return null;
    return editorFileDropHandler(file, api, currentEntry);
  }

  async function handleDeleteAttachment(attachmentPath: string) {
    if (!api) return;
    await deleteAttachmentHandler(attachmentPath, api, currentEntry);
  }

  async function handlePreviewAttachment(attachmentValue: string) {
    if (!api || !currentEntry) return;
    // Parse markdown link if present: [name](/path) -> extract path
    const linkMatch = /^\[([^\]]*)\]\(([^)]+)\)$/.exec(attachmentValue);
    const attachmentPath = linkMatch ? linkMatch[2] : attachmentValue;
    const assetPath = attachmentPath.endsWith(".md") ? attachmentPath.slice(0, -3) : attachmentPath;
    const displayName = linkMatch ? (linkMatch[1] || assetPath.split("/").pop() || assetPath) : assetPath.split("/").pop() || attachmentValue;

    try {
      const data = await api.getAttachmentData(currentEntry.path, attachmentPath);
      const mimeType = getMimeType(assetPath);
      let blob = new Blob([new Uint8Array(data)], { type: mimeType });
      let mediaKind = getAttachmentMediaKind(assetPath, mimeType);
      if (isHeicFile(assetPath)) {
        blob = await convertHeicToJpeg(blob);
        mediaKind = "image";
      }

      // On iOS Tauri, use native image viewer for images (pinch-to-zoom, etc.)
      if (isTauri() && isIOS() && mediaKind === "image") {
        const arrayBuffer = await blob.arrayBuffer();
        const bytes = new Uint8Array(arrayBuffer);
        let binary = "";
        for (let i = 0; i < bytes.length; i++) {
          binary += String.fromCharCode(bytes[i]);
        }
        const base64 = btoa(binary);
        (window as any).webkit?.messageHandlers?.editorToolbar?.postMessage({
          type: "imagePreview",
          base64,
          name: displayName,
        });
        return;
      }

      // Revoke previous preview URL if any
      if (previewImageUrl) URL.revokeObjectURL(previewImageUrl);
      previewImageUrl = URL.createObjectURL(blob);
      previewImageName = displayName;
      previewImageKind = mediaKind;
      imagePreviewOpen = true;
    } catch (e) {
      console.error("[App] Failed to load image preview:", e);
    }
  }

  function handleImagePreviewClose(open: boolean) {
    imagePreviewOpen = open;
    if (!open && previewImageUrl) {
      URL.revokeObjectURL(previewImageUrl);
      previewImageUrl = null;
      previewImageKind = "image";
    }
  }

  function handleAttachmentInsert(selection: {
    path: string;
    kind: AttachmentMediaKind;
    blobUrl?: string;
    filename?: string;
    sourceEntryPath: string;
  }) {
    attachmentInsertHandler(selection, editorRef, currentEntry);
  }

  // Handle drag-drop: attach entry to new parent (with optional position hint for reorder after reparent)
  async function handleMoveEntry(
    entryPath: string,
    newParentPath: string,
    position?: { beforePath?: string; afterPath?: string },
  ) {
    if (!api) return;
    if (entryPath === newParentPath) return;

    try {
      // Save state for undo: get old parent's contents before the move
      const oldParentNode = tree ? findTreeParent(tree, entryPath) : null;
      const oldParentPath = oldParentNode?.path ?? null;
      let oldContents: unknown[] | null = null;
      if (oldParentPath) {
        const oldParentEntry = await api.getEntry(oldParentPath);
        if (oldParentEntry && Array.isArray(oldParentEntry.frontmatter.contents)) {
          oldContents = [...oldParentEntry.frontmatter.contents];
        }
      }

      // attachEntryToParent handles leaf-to-index conversion internally
      // (creates directory structure, moves the file, updates hierarchy metadata).
      // Returns the new path of the entry, which may differ from entryPath when
      // the file moves to a different directory.
      const nextPath = await api.attachEntryToParent(entryPath, newParentPath);

      // If a position hint was given, reorder within the new parent
      if (position && (position.beforePath || position.afterPath)) {
        const parentEntry = await api.getEntry(newParentPath);
        if (parentEntry && Array.isArray(parentEntry.frontmatter.contents)) {
          const contents = parentEntry.frontmatter.contents.map(String);
          // Find the just-added entry in contents using its new path after the move
          const entryCanonical = await api.canonicalizeLink(nextPath, newParentPath).catch(() => null);
          let entryLinkIdx = -1;
          for (let i = 0; i < contents.length; i++) {
            const canonical = await api.canonicalizeLink(contents[i], newParentPath).catch(() => null);
            if (canonical === entryCanonical || canonical === nextPath) {
              entryLinkIdx = i;
              break;
            }
          }
          if (entryLinkIdx !== -1) {
            const [entryLink] = contents.splice(entryLinkIdx, 1);
            const anchorPath = position.beforePath || position.afterPath!;
            let anchorIdx = -1;
            for (let i = 0; i < contents.length; i++) {
              const canonical = await api.canonicalizeLink(contents[i], newParentPath).catch(() => null);
              if (canonical === anchorPath) {
                anchorIdx = i;
                break;
              }
            }
            if (anchorIdx !== -1) {
              const insertIdx = position.afterPath ? anchorIdx + 1 : anchorIdx;
              contents.splice(insertIdx, 0, entryLink);
              await api.setFrontmatterProperty(newParentPath, "contents", contents, tree?.path);
            }
          }
        }
      }

      await refreshTree();
      await runValidation();

      // Show toast with undo
      const entryName = entryPath.split("/").pop()?.replace(".md", "") ?? entryPath;
      const parentName = newParentPath.split("/").pop()?.replace(".md", "") ?? newParentPath;
      toast.success(`Moved "${entryName}" into "${parentName}"`, {
        action: oldContents && oldParentPath ? {
          label: "Undo",
          onClick: async () => {
            try {
              if (!api) return;
              await api.attachEntryToParent(entryPath, oldParentPath);
              await api.setFrontmatterProperty(oldParentPath, "contents", oldContents as JsonValue, tree?.path);
              await refreshTree();
              if (currentEntry?.path === oldParentPath || currentEntry?.path === newParentPath) {
                const refreshed = await api.getEntry(currentEntry!.path);
                if (refreshed) entryStore.setCurrentEntry(refreshed);
              }
            } catch {
              toast.error("Failed to undo move");
            }
          },
        } : undefined,
      });

      // Refresh current entry if it's the parent
      if (currentEntry?.path === newParentPath || currentEntry?.path === oldParentPath) {
        const refreshed = await api.getEntry(currentEntry!.path);
        if (refreshed) entryStore.setCurrentEntry(refreshed);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Tree traversal helper for undo support
  function findTreeParent(root: TreeNode, targetPath: string): TreeNode | null {
    for (const child of root.children) {
      if (child.path === targetPath) return root;
      const found = findTreeParent(child, targetPath);
      if (found) return found;
    }
    return null;
  }

  // Handle reorder of children within a parent (drag between siblings)
  async function handleReorderChildren(parentPath: string, childPaths: string[]) {
    if (!api) return;
    try {
      // Get the parent entry to access its current contents links
      const parentEntry = await api.getEntry(parentPath);
      if (!parentEntry) return;
      const currentContents = parentEntry.frontmatter.contents;
      if (!Array.isArray(currentContents)) return;

      // Build a map from canonical path -> original link string
      const linkByCanonical = new Map<string, string>();
      for (const link of currentContents) {
        const linkStr = String(link);
        try {
          const canonical = await api.canonicalizeLink(linkStr, parentPath);
          linkByCanonical.set(canonical, linkStr);
        } catch {
          // If canonicalization fails, skip
        }
      }

      // Reorder: build new contents array matching childPaths order
      const newContents: string[] = [];
      for (const childPath of childPaths) {
        const originalLink = linkByCanonical.get(childPath);
        if (originalLink) {
          newContents.push(originalLink);
        }
      }
      // Append any contents entries that weren't matched (safety)
      for (const link of currentContents) {
        if (!newContents.includes(String(link))) {
          newContents.push(String(link));
        }
      }

      // Save old contents for undo
      const oldContents = currentContents.map(String);

      await api.setFrontmatterProperty(parentPath, "contents", newContents, tree?.path);
      await refreshTree();
      // Refresh the current entry if it's the parent whose contents changed
      if (currentEntry?.path === parentPath) {
        const refreshed = await api.getEntry(parentPath);
        if (refreshed) {
          entryStore.setCurrentEntry(refreshed);
        }
      }

      // Toast with undo
      toast.success("Reordered entries", {
        action: {
          label: "Undo",
          onClick: async () => {
            try {
              if (!api) return;
              await api.setFrontmatterProperty(parentPath, "contents", oldContents, tree?.path);
              await refreshTree();
              if (currentEntry?.path === parentPath) {
                const refreshed = await api.getEntry(parentPath);
                if (refreshed) entryStore.setCurrentEntry(refreshed);
              }
            } catch {
              toast.error("Failed to undo reorder");
            }
          },
        },
      });
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleMoveAttachmentWrapper(
    sourceEntryPath: string,
    targetEntryPath: string,
    attachmentPath: string
  ) {
    if (!api) return;
    await moveAttachmentHandler(sourceEntryPath, targetEntryPath, attachmentPath, api, currentEntry);
  }

  // Handle frontmatter property changes
  // Title changes with auto-rename are handled atomically by the backend
  async function handlePropertyChange(key: string, value: unknown) {
    if (!api || !currentEntry) return;
    try {
      const path = currentEntry.path;
      const normalizedFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);

      if (key === "title" && typeof value === "string" && value.trim()) {
        const nextTitle = value.trim();
        const currentTitle =
          typeof normalizedFrontmatter.title === "string"
            ? normalizedFrontmatter.title
            : "";

        if (nextTitle === currentTitle) {
          entryStore.setTitleError(null);
          return;
        }

        // Flush pending editor saves before title change (backend may rename the file)
        if (isDirty && editorRef) {
          await saveEntryWithSync(api, currentEntry, editorRef, tree?.path);
        }

        try {
          // Backend handles: workspace config read, filename style, rename, title set, H1 sync
          // Returns new path string if rename occurred, null otherwise
          const newPath = await api.setFrontmatterProperty(path, key, value, tree?.path);

          const effectivePath = newPath ?? path;

          if (newPath) {
            // Rename happened — update expanded node tracking
            if (expandedNodes.has(path)) {
              workspaceStore.collapseNode(path);
              workspaceStore.expandNode(newPath);
            }
            if (collaborationEnabled) {
              collaborationStore.setCollaborationPath(toCollaborationPath(newPath, tree?.path ?? ""));
            }
          }

          // Re-read the entry from disk so the editor picks up the
          // backend's H1 sync (and any other on-disk changes).
          try {
            const refreshed = await api.getEntry(effectivePath);
            refreshed.frontmatter = normalizeFrontmatter(refreshed.frontmatter);
            entryStore.setCurrentEntry(refreshed);
            entryStore.setDisplayContent(refreshed.content);
          } catch {
            // Fallback: just update frontmatter in store
            const updatedEntry = {
              ...currentEntry,
              path: effectivePath,
              frontmatter: { ...normalizedFrontmatter, [key]: value },
            };
            entryStore.setCurrentEntry(updatedEntry);
          }

          // If this entry is the root index, sync workspace name in registry
          if (path === tree?.path || effectivePath === tree?.path) {
            const wsId = getCurrentWorkspaceId();
            if (wsId) {
              renameLocalWorkspace(wsId, nextTitle);
            }
          }

          entryStore.setTitleError(null);
          await refreshTree();
        } catch (renameError) {
          // Rename failed (e.g., target exists), show user-friendly error
          const errorMsg =
            renameError instanceof Error
              ? renameError.message
              : String(renameError);
          if (
            errorMsg.includes("already exists") ||
            errorMsg.includes("Destination")
          ) {
            entryStore.setTitleError(
              "A file with that name already exists. Choose a different title."
            );
          } else {
            entryStore.setTitleError(`Could not rename: ${errorMsg}`);
          }
        }
      } else {
        // Non-title properties: update normally
        await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue, tree?.path);
        const updatedEntry = {
          ...currentEntry,
          frontmatter: { ...normalizedFrontmatter, [key]: value },
        };
        entryStore.setCurrentEntry(updatedEntry);

        await refreshTree();
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handlePropertyRemove(key: string) {
    if (!api || !currentEntry) return;
    try {
      await api.removeFrontmatterProperty(currentEntry.path, key);
      // Update local state
      const newFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);
      delete newFrontmatter[key];
      entryStore.setCurrentEntry({ ...currentEntry, frontmatter: newFrontmatter });
      await refreshTree();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handlePropertyAdd(key: string, value: unknown) {
    if (!api || !currentEntry) return;
    try {
      await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue, tree?.path);
      const normalizedFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);
      // Update local state
      const updatedEntry = {
        ...currentEntry,
        frontmatter: { ...normalizedFrontmatter, [key]: value },
      };
      entryStore.setCurrentEntry(updatedEntry);
      await refreshTree();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handlePropertyReorder(keys: string[]) {
    if (!api || !currentEntry) return;
    try {
      await api.reorderFrontmatterKeys(currentEntry.path, keys);
      // Refresh entry to get new order
      const refreshed = await api.getEntry(currentEntry.path);
      if (refreshed) {
        entryStore.setCurrentEntry(refreshed);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleEntryRefreshRequest() {
    if (!api || !currentEntry) return;
    try {
      const refreshed = await api.getEntry(currentEntry.path);
      if (refreshed) {
        entryStore.setCurrentEntry(refreshed);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Handle link clicks in the editor - delegates to controller
  async function handleLinkClick(href: string) {
    if (!api) return;
    await linkClickHandler(href, api, currentEntry, tree, openEntry, refreshTree);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<PermissionBanner />

{#if showNewEntryModal}
  <NewEntryModal
    {tree}
    {api}
    rootIndexPath={tree?.path ?? null}
    onSave={createNewEntry}
    onCancel={() => uiStore.closeNewEntryModal()}
  />
{/if}

<!-- Command Palette -->
  <CommandPalette
    bind:open={uiStore.showCommandPalette}
    swipeProgress={mobileGestures.commandPaletteSwipeProgress}
    {api}
    {commandRegistry}
  />

<!-- In-app Prompt Dialog (replaces window.prompt) -->
<Dialog.Root
  open={!!promptDialog}
  onOpenChange={(open) => { if (!open) promptDialog = null; }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{promptDialog?.title ?? ""}</Dialog.Title>
    </Dialog.Header>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        const value = promptDialog?.value ?? "";
        const onSubmit = promptDialog?.onSubmit;
        promptDialog = null;
        onSubmit?.(value);
      }}
    >
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm text-muted-foreground whitespace-pre-line">{promptDialog?.label ?? ""}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="text"
        class="mt-2 flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-base shadow-xs outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] md:text-sm"
        value={promptDialog?.value ?? ""}
        oninput={(e) => {
          if (promptDialog) promptDialog = { ...promptDialog, value: (e.target as HTMLInputElement).value };
        }}
        autofocus
      />
      <div class="flex justify-end gap-2 mt-4">
        <Button variant="outline" type="button" onclick={() => { promptDialog = null; }}>Cancel</Button>
        <Button type="submit">OK</Button>
      </div>
    </form>
  </Dialog.Content>
</Dialog.Root>

<SettingsDialog
  bind:open={showSettingsDialog}
  bind:focusMode
  workspacePath={tree?.path}
  initialTab={settingsInitialTab}
  {api}
  onHostAction={handlePluginHostAction}
/>

<MarketplaceDialog bind:open={showMarketplaceDialog} />

<!-- Export Dialog -->
<ExportDialog
  bind:open={showExportDialog}
  rootPath={exportPath}
  {api}
  onOpenChange={(open) => (showExportDialog = open)}
/>


<!-- Device Replacement Dialog (shown when sign-in hits device limit) -->
<DeviceReplacementDialog onAuthenticated={() => {
  if (showWelcomeScreen && welcomeScreenRef) {
    welcomeScreenRef.handleSignInComplete();
  } else {
    handleWelcomeComplete("", "");
  }
}} />

<!-- Delete Confirmation Dialog -->
<Dialog.Root bind:open={showDeleteConfirm} onOpenChange={(open) => { if (!open) cancelDeleteEntry(); }}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{pendingDeletePaths.length > 1 ? "Delete entries" : "Delete entry"}</Dialog.Title>
      <Dialog.Description>
        {pendingDeleteDescription}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={cancelDeleteEntry} disabled={isDeletingEntries}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDeleteEntry} disabled={isDeletingEntries}>
        {isDeletingEntries ? "Deleting..." : "Delete"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Plugin Confirmation Dialog -->
<Dialog.Root
  bind:open={showPluginConfirmDialog}
  onOpenChange={(open) => {
    if (!open && pluginConfirmResolve) resolvePluginConfirm(false);
  }}
>
  <Dialog.Content showCloseButton={false} class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{pluginConfirmTitle}</Dialog.Title>
      <Dialog.Description>{pluginConfirmDescription}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => resolvePluginConfirm(false)}>
        {pluginConfirmCancelLabel}
      </Button>
      <Button
        variant={pluginConfirmVariant === "destructive" ? "destructive" : "default"}
        onclick={() => resolvePluginConfirm(true)}
      >
        {pluginConfirmConfirmLabel}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Plugin Prompt Dialog -->
<Dialog.Root
  bind:open={showPluginPromptDialog}
  onOpenChange={(open) => {
    if (!open && pluginPromptResolve) resolvePluginPrompt(null);
  }}
>
  <Dialog.Content showCloseButton={false} class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{pluginPromptTitle}</Dialog.Title>
      <Dialog.Description>{pluginPromptDescription}</Dialog.Description>
    </Dialog.Header>
    <div class="py-2">
      <input
        bind:this={pluginPromptInput}
        bind:value={pluginPromptValue}
        placeholder={pluginPromptPlaceholder}
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            resolvePluginPrompt(pluginPromptValue);
          }
        }}
      />
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => resolvePluginPrompt(null)}>
        {pluginPromptCancelLabel}
      </Button>
      <Button
        variant={pluginPromptVariant === "destructive" ? "destructive" : "default"}
        onclick={() => resolvePluginPrompt(pluginPromptValue)}
      >
        {pluginPromptConfirmLabel}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Audience Dialog -->
<Dialog.Root bind:open={showAudienceDialog} onOpenChange={(open) => { if (!open) { showAudienceDialog = false; audienceDialogPath = null; } }}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Set Audience</Dialog.Title>
      <Dialog.Description>
        {audienceDialogPath?.split('/').pop()?.replace('.md', '') ?? ''}
      </Dialog.Description>
    </Dialog.Header>
    <div class="py-2">
      {#if audienceDialogPath}
        <AudienceEditor
          audience={audienceDialogAudience}
          entryPath={audienceDialogPath}
          rootPath={tree?.path ?? ""}
          {api}
          onChange={handleAudienceChange}
          onOpenManager={() => { showAudienceDialog = false; audienceDialogPath = null; audiencePanelStore.openPanel(); }}
        />
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<AudiencePanel {api} rootPath={tree?.path ?? ""} />

<!-- Toast Notifications -->
<Toaster position={mobileState.isMobile ? "top-center" : "bottom-right"} />

<!-- Tooltip Provider for keyboard shortcut hints -->
<Tooltip.Provider>

<!-- Shared Tauri overlay titlebar drag region (macOS desktop). Keep it outside
     the app-state branches so welcome/onboarding and narrow windows stay draggable. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed top-0 left-0 right-0 z-[5]"
  style="height: var(--titlebar-area-height);"
  onmousedown={maybeStartWindowDrag}
></div>

{#if fsaNeedsReconnect}
  <div class="flex h-full items-center justify-center bg-background">
    <div class="flex flex-col items-center gap-4 text-center max-w-sm px-4">
      <div class="text-4xl">&#128194;</div>
      <h2 class="text-lg font-semibold">Reconnect local folder</h2>
      <p class="text-sm text-muted-foreground">
        Your workspace uses a local folder that needs permission to access. Click below to reconnect.
      </p>
      <Button onclick={handleFsaReconnect}>Reconnect folder</Button>
    </div>
  </div>
{:else if showWelcomeScreen}
  <WelcomeScreen
    bind:this={welcomeScreenRef}
    initialView={welcomeInitialView}
    onLaunch={(info) => { launchOverlay = info; }}
    onGetStarted={async (selectedBundle, pluginOverrides) => {
      entryStore.setLoading(true);
      try {
        const result = await handleGetStartedController(
          {
            autoCreateDeps: buildAutoCreateDeps(),
            installLocalPlugin: (bytes, name) => installLocalPlugin(bytes, name),
            refreshTree,
            getTree: () => tree,
            expandNode: (path) => workspaceStore.expandNode(path),
            openEntry,
            runValidation,
            dismissLaunchOverlay,
          },
          selectedBundle,
          pluginOverrides,
        );
        showWelcomeScreen = false;

        // Trigger spotlight onboarding tour if the bundle defines one
        if (result.spotlightSteps) {
          await tick();
          requestAnimationFrame(() => {
            spotlightSteps = result.spotlightSteps;
          });
        }
      } catch (e) {
        console.error("[App] Auto-create from welcome screen failed:", e);
        throw e;
      } finally {
        entryStore.setLoading(false);
        // Clear the launch zoom overlay so it doesn't block the welcome screen on error
        launchOverlay = null;
        launchOverlayDone = false;
      }
    }}
    onSignInCreateNew={async () => {
      // User signed in but has no existing workspaces — create first synced workspace
      entryStore.setLoading(true);
      try {
        await handleSignInCreateNewController({
          autoCreateDeps: buildAutoCreateDeps(),
          refreshTree,
          getTree: () => tree,
          expandNode: (path) => workspaceStore.expandNode(path),
          openEntry,
          runValidation,
        });
        showWelcomeScreen = false;
      } catch (e) {
        console.error("[App] Auto-create after sign-in failed:", e);
        throw e;
      } finally {
        entryStore.setLoading(false);
      }
    }}
    onCreateWithProvider={async (bundle, providerPluginId, pluginOverrides, restoreNamespace, onProgress) => {
      entryStore.setLoading(true);
      try {
        const result = await withLiveSyncSetupProgress(onProgress, async () => await handleCreateWithProviderController(
          {
            autoCreateDeps: buildAutoCreateDeps(),
            installLocalPlugin: (bytes, name) => installLocalPlugin(bytes, name),
            refreshTree,
            getTree: () => tree,
            expandNode: (path) => workspaceStore.expandNode(path),
            openEntry,
            runValidation,
            dismissLaunchOverlay,
            persistPermissionDefaults: persistRequestedPluginPermissionDefaults,
            switchWorkspace,
          },
          bundle,
          providerPluginId,
          pluginOverrides,
          restoreNamespace,
          onProgress,
        ),
        );
        showWelcomeScreen = false;

        if (result.spotlightSteps) {
          await tick();
          requestAnimationFrame(() => {
            spotlightSteps = result.spotlightSteps;
          });
        }
      } catch (e) {
        console.error("[App] Create with provider failed:", e);
        throw e;
      } finally {
        entryStore.setLoading(false);
        // Clear the launch zoom overlay so it doesn't block the welcome screen on error
        launchOverlay = null;
        launchOverlayDone = false;
      }
    }}
    onMoveCurrentWorkspace={async (providerPluginId, onProgress) => {
      const workspaceId = getCurrentWorkspaceId();
      const workspace = workspaceId ? getLocalWorkspace(workspaceId) : null;
      if (!workspaceId || !workspace) {
        throw new Error("No current workspace is available.");
      }

      const currentLink = getPrimaryWorkspaceProviderLink(workspaceId);
      const currentProviderId = currentLink?.pluginId ?? null;
      const currentProviderLabel = currentProviderId
        ? getProviderDisplayLabel(currentProviderId) ?? currentProviderId
        : null;

      entryStore.setLoading(true);
      try {
        await withLiveSyncSetupProgress(onProgress, async () => {
        if (!providerPluginId) {
          if (!currentProviderId) {
            toast.message("This workspace is already stored only on this device.");
            showWelcomeScreen = false;
            welcomeReturnWorkspaceName = null;
            return;
          }

          if (isWorkspaceProviderSyncEnabled(workspaceId)) {
            onProgress?.({ percent: 20, message: "Disconnecting cloud sync..." });
            await unlinkWorkspace(currentProviderId, workspaceId);
          } else {
            setPluginMetadata(workspaceId, currentProviderId, null);
          }

          toast.success(`"${workspace.name}" now lives only on this device.`);
        } else {
          const providerLabel = getProviderDisplayLabel(providerPluginId) ?? providerPluginId;
          const existingTargetLink = getWorkspaceProviderLink(workspaceId, providerPluginId);

          if (currentProviderId && currentProviderId !== providerPluginId) {
            if (isWorkspaceProviderSyncEnabled(workspaceId)) {
              onProgress?.({ percent: 18, message: "Disconnecting previous provider..." });
              await unlinkWorkspace(currentProviderId, workspaceId);
            } else {
              setPluginMetadata(workspaceId, currentProviderId, null);
            }
          }

          if (currentProviderId === providerPluginId && isWorkspaceProviderSyncEnabled(workspaceId)) {
            toast.message(`"${workspace.name}" is already stored with ${providerLabel}.`);
          } else {
            await linkWorkspace(providerPluginId, {
              localId: workspaceId,
              name: workspace.name,
              remoteId: existingTargetLink?.remoteWorkspaceId,
            }, onProgress);
            toast.success(
              currentProviderLabel && currentProviderId !== providerPluginId
                ? `Moved "${workspace.name}" from ${currentProviderLabel} to ${providerLabel}.`
                : `"${workspace.name}" is now stored with ${providerLabel}.`,
            );
          }
        }
        });

        showWelcomeScreen = false;
        welcomeReturnWorkspaceName = null;
      } finally {
        entryStore.setLoading(false);
        launchOverlay = null;
        launchOverlayDone = false;
      }
    }}
    returnWorkspaceName={welcomeReturnWorkspaceName}
    onReturn={() => {
      showWelcomeScreen = false;
      welcomeReturnWorkspaceName = null;
    }}
  />
{:else}
<div class="relative flex h-full bg-background overflow-hidden {resizingSidebar ? 'select-none cursor-col-resize' : ''}">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {activeEntryPath}
    {isLoading}
    {workspaceMissing}
    {expandedNodes}
    {validationResult}
    {showUnlinkedFiles}
    {api}
    collapsed={leftSidebarCollapsed}
    sidebarWidth={leftSidebarWidth}
    resizing={resizingSidebar === 'left'}
    swipeProgress={mobileGestures.leftSidebarSwipeProgress}
    onOpenEntry={openEntry}
    onToggleNode={toggleNode}
    onToggleCollapse={toggleLeftSidebar}
    onOpenSettings={() => { settingsInitialTab = undefined; showSettingsDialog = true; }}
    onOpenMarketplace={() => { showMarketplaceDialog = true; }}
    settingsDialogOpen={showSettingsDialog}
    marketplaceDialogOpen={showMarketplaceDialog}
    onOpenAccountSettings={() => { settingsInitialTab = "account"; showSettingsDialog = true; }}
    onAddWorkspace={() => {
      const wsId = getCurrentWorkspaceId();
      const localWs = wsId ? getLocalWorkspace(wsId) : null;
      welcomeReturnWorkspaceName = localWs?.name ?? null;
      welcomeInitialView = 'bundles';
      showWelcomeScreen = true;
    }}
    onBrowseRemoteWorkspaces={() => {
      const wsId = getCurrentWorkspaceId();
      const localWs = wsId ? getLocalWorkspace(wsId) : null;
      welcomeReturnWorkspaceName = localWs?.name ?? null;
      welcomeInitialView = 'workspace-picker';
      showWelcomeScreen = true;
    }}
    onMoveEntry={handleMoveEntry}
    onReorderChildren={handleReorderChildren}
    onOpenMoveDialog={(path) => { moveEntryDialogPath = path; }}
    onCreateChildEntry={handleCreateChildEntry}
    onDeleteEntry={handleDeleteEntry}
    onDeleteEntries={handleDeleteEntries}
    onExport={(path) => {
      exportPath = path;
      showExportDialog = true;
    }}
    onOpenBackupImport={handleQuickBackupExport}
    onImportMarkdownFile={handleImportMarkdownFile}
    onAddAttachment={handleAddAttachment}
    onMoveAttachment={handleMoveAttachmentWrapper}
    onRemoveBrokenPartOf={handleRemoveBrokenPartOf}
    onRemoveBrokenContentsRef={handleRemoveBrokenContentsRef}
    onAttachUnlinkedEntry={handleAttachUnlinkedEntry}
    onValidationFix={async () => {
      await refreshTree();
      await runValidation();
      // Refresh current entry to reflect frontmatter changes
      if (currentEntry && api) {
        const entry = await api.getEntry(currentEntry.path);
        entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
        entryStore.setCurrentEntry(entry);
      }
    }}
    onLoadChildren={loadNodeChildren}
    onValidate={handleValidate}
    onRenameEntry={handleRenameEntry}
    onDuplicateEntry={handleDuplicateEntry}
    onWorkspaceSwitchStart={handleWorkspaceSwitchStart}
    onWorkspaceSwitchComplete={handleWorkspaceSwitchComplete}
    onWorkspaceMissing={(ws) => { workspaceMissing = ws; entryStore.setLoading(false); }}
    onInitializeWorkspace={handleInitializeWorkspace}
    onShowWelcome={() => {
      const wsId = getCurrentWorkspaceId();
      const localWs = wsId ? getLocalWorkspace(wsId) : null;
      welcomeReturnWorkspaceName = localWs?.name ?? null;
      showWelcomeScreen = true;
    }}
    onSetAudience={handleSetAudience}
    onPaintEntry={handlePaintEntry}
    requestedTab={requestedLeftTab}
    onRequestedTabConsumed={() => (requestedLeftTab = null)}
    onPluginHostAction={handlePluginHostAction}
    syncEnabled={(() => {
      const wsId = getCurrentWorkspaceId();
      return !!wsId && isWorkspaceProviderSyncEnabled(wsId);
    })()}
    onSync={runManualSyncNow}
    onSyncPull={async () => {
      const result = await dispatchPluginSyncCommand("SyncPull");
      if (!result.success) console.warn("[App] SyncPull:", result.error);
    }}
    onSyncPush={async () => {
      const result = await dispatchPluginSyncCommand("SyncPush");
      if (!result.success) console.warn("[App] SyncPush:", result.error);
    }}
    onSyncRefreshStatus={async () => {
      const result = await dispatchPluginSyncCommand("SyncStatus");
      if (!result.success) {
        console.warn("[App] SyncStatus:", result.error);
        return;
      }
      // Update the collaboration store from the plugin's status response
      // so the UI reflects the current state without waiting for events.
      if (result.data && typeof result.data === "object") {
        const d = result.data as Record<string, unknown>;
        const state = typeof d.state === "string" ? d.state : null;
        if (state === "synced" || state === "dirty") {
          collaborationStore.setSyncStatus("synced");
          collaborationStore.setSyncProgress(null);
        } else if (state === "error") {
          collaborationStore.setSyncStatus("error");
        }
        if (typeof d.dirty_count === "number" && d.dirty_count > 0) {
          collaborationStore.setSyncStatus("syncing");
        }
      }
    }}
  />

  <!-- Left sidebar resize handle -->
  {#if !leftSidebarCollapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="hidden md:block w-1 shrink-0 cursor-col-resize select-none hover:bg-primary/20 active:bg-primary/30 transition-colors {resizingSidebar === 'left' ? 'bg-primary/30' : ''}"
      onpointerdown={(e) => onResizePointerDown('left', e)}
      onpointermove={onResizePointerMove}
      onpointerup={onResizePointerUp}
      ondblclick={() => onResizeDblClick('left')}
      role="separator"
      aria-orientation="vertical"
    ></div>
  {/if}

  <!-- Hidden file input for attachments (accepts all file types) -->
  <input
    type="file"
    bind:this={attachmentFileInput}
    onchange={handleAttachmentFileSelect}
    class="hidden"
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0 relative" data-spotlight="editor-area">
    <!-- Sidebar open buttons (visible when collapsed, fade in focus mode, reveal on hover via edge strip) -->
    {#if leftSidebarCollapsed}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="absolute bottom-[calc(env(safe-area-inset-bottom)+2.25rem)] left-0 z-20 w-10 hidden md:flex flex-col items-center group"
        onmouseenter={() => leftEdgeHovered = true}
        onmouseleave={() => leftEdgeHovered = false}
      >
        <!-- Collapsed sidebar quick-access icons -->
        <div class="flex flex-col items-center gap-0.5 pb-1 transition-opacity duration-200
          {focusMode && leftSidebarCollapsed && rightSidebarCollapsed && !leftEdgeHovered ? 'opacity-0' : 'opacity-100'}">
          <!-- Audience/Eye -->
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="p-2 rounded-md hover:bg-accent transition-colors"
                onclick={() => { audiencePanelStore.openPanel(); }}
                aria-label="Audience filter"
              >
                <Eye class="size-4 text-muted-foreground" />
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content side="right">Audience</Tooltip.Content>
          </Tooltip.Root>

          <!-- Cloud/Sync -->
          {#if collapsedBarSyncEnabled}
            <Tooltip.Root>
              <Tooltip.Trigger>
                <button
                  type="button"
                  class="p-2 rounded-md hover:bg-accent transition-colors"
                  onclick={runManualSyncNow}
                  aria-label="Sync"
                >
                  {#if syncState.syncing}
                    <Loader2 class="size-4 animate-spin text-amber-500" />
                  {:else if collaborationStore.serverOffline}
                    <CloudOff class={`size-4 ${collapsedBarSyncColor}`} />
                  {:else}
                    <Cloud class={`size-4 ${collapsedBarSyncColor}`} />
                  {/if}
                </button>
              </Tooltip.Trigger>
              <Tooltip.Content side="right">Sync</Tooltip.Content>
            </Tooltip.Root>
          {/if}

          <!-- Marketplace -->
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="p-2 rounded-md hover:bg-accent transition-colors"
                onclick={() => { showMarketplaceDialog = true; }}
                aria-label="Open marketplace"
              >
                <Store class="size-4 text-muted-foreground" />
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content side="right">Marketplace</Tooltip.Content>
          </Tooltip.Root>

          <!-- Settings -->
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="p-2 rounded-md hover:bg-accent transition-colors"
                onclick={() => { settingsInitialTab = undefined; showSettingsDialog = true; }}
                aria-label="Open settings"
              >
                <Settings class="size-4 text-muted-foreground" />
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content side="right">Settings</Tooltip.Content>
          </Tooltip.Root>

          <!-- Account -->
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="p-2 rounded-md hover:bg-accent transition-colors"
                onclick={() => { settingsInitialTab = "account"; showSettingsDialog = true; }}
                aria-label={authState.isAuthenticated ? "Account settings" : "Sign in"}
              >
                <span class="relative inline-flex">
                  <CircleUser class="size-4 text-muted-foreground" />
                  {#if authState.isAuthenticated}
                    <span
                      class="absolute -bottom-0.5 -right-0.5 size-2 rounded-full ring-1 ring-background {collaborationStore.serverOffline ? 'bg-amber-500' : 'bg-emerald-500'}"
                    ></span>
                  {/if}
                </span>
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content side="right">{authState.isAuthenticated ? 'Account' : 'Sign in'}</Tooltip.Content>
          </Tooltip.Root>

        </div>
      </div>
    {/if}
    {#if currentEntry}
      {#if mobileFocusModeActive && !mobileFocusChromeRevealed}
        <button
          type="button"
          class="absolute inset-x-0 top-0 z-20 h-[calc(env(safe-area-inset-top)+0.75rem)] md:hidden"
          aria-label="Show editor controls"
          onclick={revealMobileFocusChromeTemporarily}
          onpointerdown={revealMobileFocusChromeTemporarily}
        >
          <span class="sr-only">Show editor controls</span>
        </button>
      {/if}
      <!-- Mobile top navigation strip -->
      <header
        class="flex items-center justify-between px-2 py-1.5 border-b border-sidebar-border bg-sidebar-accent md:hidden select-none shrink-0
          pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height))]
          transition-[opacity,transform] duration-300 ease-in-out
          {mobileFocusModeActive ? 'absolute inset-x-0 top-0 z-30' : 'relative shrink-0'}
          {mobileFocusModeActive && !mobileFocusChromeRevealed ? '-translate-y-full opacity-0 pointer-events-none' : 'translate-y-0 opacity-100'}"
        onpointerdown={revealMobileFocusChromeTemporarily}
        ontouchstart={revealMobileFocusChromeTemporarily}
      >
        <button
          type="button"
          class="p-3"
          onclick={toggleLeftSidebar}
          aria-label="Toggle navigation"
        >
          <Menu class="size-5 text-muted-foreground" />
        </button>
        <span class="flex items-center gap-1.5 min-w-0 mx-2">
          <span class="text-sm font-medium text-foreground truncate">
            {typeof currentEntry.frontmatter?.title === "string"
              ? currentEntry.frontmatter.title
              : currentEntry.path.split("/").pop()?.replace(".md", "") ?? ""}
          </span>
          {#if effectiveAudienceTags.length > 0}
            <button
              type="button"
              class="flex items-center gap-1 shrink-0 p-2"
              onclick={() => { audiencePanelStore.openPanel(); }}
              aria-label="Manage audiences"
            >
              {#each effectiveAudienceTags as tag}
                <span
                  class="size-2 rounded-full {getAudienceColor(tag, getAudienceColorStore().audienceColors)}"
                  title={tag}
                ></span>
              {/each}
            </button>
          {/if}
        </span>
        {#if rightSidebarCollapsed}
          <button
            type="button"
            class="p-3"
            onclick={toggleRightSidebar}
            aria-label="Open properties panel"
          >
            <PanelRight class="size-5 text-muted-foreground" />
          </button>
        {:else}
          <div class="size-9"></div>
        {/if}
      </header>
      <EditorContent
        {Editor}
        bind:editorRef
        content={displayContent}
        editorKey={currentEntry.path}
        readonly={editorReadonly}
        onchange={handleContentChange}
        onblur={handleEditorBlur}
        entryPath={currentEntry.path}
        {api}
        onAttachmentInsert={handleAttachmentInsert}
        onFileDrop={handleEditorFileDrop}
        onLinkClick={handleLinkClick}
        onPreviewMedia={handlePreviewAttachment}
      />
      <FindBar bind:open={showFindBar} {editorRef} />
      {#if loadingTargetPath}
        <div
          data-testid="entry-switch-loading-overlay"
          class="absolute inset-0 z-10 bg-background/70 backdrop-blur-[1px] pointer-events-none flex items-center justify-center"
        >
          <div class="rounded-md border border-border bg-card/95 px-3 py-2 text-sm text-muted-foreground shadow-sm">
            Opening {loadingTargetPath.split("/").pop()?.replace(".md", "") ?? "entry"}...
          </div>
        </div>
      {/if}

      <EditorFooter
        leftSidebarOpen={!leftSidebarCollapsed}
        rightSidebarOpen={!rightSidebarCollapsed}
        {focusMode}
        mobileFocusChromeVisible={mobileFocusChromeRevealed}
        readonly={editorReadonly}
        commandPaletteOpen={uiStore.showCommandPalette}
        onOpenCommandPalette={uiStore.openCommandPalette}
        onRevealMobileFocusChrome={revealMobileFocusChromeTemporarily}
        {api}
        audienceTags={effectiveAudienceTags}
        onOpenAudienceManager={() => { audiencePanelStore.openPanel(); }}
        onFabMount={(el) => { mobileGestures.editorFabElement = el; }}
        {commandRegistry}
        hasEditor={!!editorRef}
        showAccountButton={leftSidebarCollapsed}
        isAuthenticated={authState.isAuthenticated}
        serverOffline={collaborationStore.serverOffline}
        onOpenAccount={() => { settingsInitialTab = "account"; showSettingsDialog = true; }}
        onOpenLeftSidebar={toggleLeftSidebar}
        onOpenRightSidebar={toggleRightSidebar}
      />
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
        {isLoading}
        {workspaceMissing}
        onToggleLeftSidebar={toggleLeftSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        hasWorkspaceTree={!!tree && tree.path !== '.'}
        onInitializeWorkspace={() => {
          const wsId = getCurrentWorkspaceId();
          const localWs = wsId ? getLocalWorkspace(wsId) : null;
          welcomeReturnWorkspaceName = localWs?.name ?? null;
          showWelcomeScreen = true;
        }}
        onRelocateWorkspace={isTauri() ? handleRelocateWorkspace : undefined}
        onRemoveWorkspace={handleRemoveWorkspace}
      />
    {/if}
  </main>

  <!-- Right sidebar resize handle -->
  {#if !rightSidebarCollapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="hidden md:block w-1 shrink-0 cursor-col-resize select-none hover:bg-primary/20 active:bg-primary/30 transition-colors {resizingSidebar === 'right' ? 'bg-primary/30' : ''}"
      onpointerdown={(e) => onResizePointerDown('right', e)}
      onpointermove={onResizePointerMove}
      onpointerup={onResizePointerUp}
      ondblclick={() => onResizeDblClick('right')}
      role="separator"
      aria-orientation="vertical"
    ></div>
  {/if}

  <!-- Right Sidebar (Properties & History) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
    sidebarWidth={rightSidebarWidth}
    resizing={resizingSidebar === 'right'}
    swipeProgress={mobileGestures.rightSidebarSwipeProgress}
    onToggleCollapse={toggleRightSidebar}
    onPropertyChange={handlePropertyChange}
    onPropertyRemove={handlePropertyRemove}
    onPropertyAdd={handlePropertyAdd}
    {titleError}
    onTitleErrorClear={() => entryStore.setTitleError(null)}
    onDeleteAttachment={handleDeleteAttachment}
    onPreviewAttachment={handlePreviewAttachment}
    {attachmentError}
    onAttachmentErrorClear={() => (attachmentError = null)}
    onOpenEntry={async (path) => await openEntry(path)}
    {rustApi}
    onHistoryRestore={async () => {
      // Refresh current entry after restore
      if (currentEntry) {
        await openEntry(currentEntry.path);
      }
    }}
    {api}
    requestedTab={requestedSidebarTab}
    onRequestedTabConsumed={() => (requestedSidebarTab = null)}
    onPluginHostAction={handlePluginHostAction}
    onPropertyReorder={handlePropertyReorder}
    {isDirty}
    {isSaving}
    readonly={editorReadonly}
    onSave={() => save(true)}
    onEntryRefreshRequest={handleEntryRefreshRequest}
  />

  {#if spotlightSteps}
    <SpotlightOverlay
      steps={spotlightSteps}
      onComplete={() => { spotlightSteps = null; }}
      mobileTargetActions={mobileSpotlightActions}
    />
  {/if}
</div>
{/if}

</Tooltip.Provider>

<!-- Image Preview Dialog -->
<ImagePreviewDialog
  open={imagePreviewOpen}
  mediaUrl={previewImageUrl}
  mediaName={previewImageName}
  mediaKind={previewImageKind}
  onOpenChange={handleImagePreviewClose}
/>

<MarkdownPreviewDialog
  open={markdownPreviewOpen}
  body={markdownPreviewBody}
  frontmatter={markdownPreviewFrontmatter}
  onOpenChange={(open) => (markdownPreviewOpen = open)}
/>

<MoveEntryDialog
  open={!!moveEntryDialogPath}
  entryPath={moveEntryDialogPath ?? ''}
  {tree}
  onMove={handleMoveEntry}
  onReorderChildren={handleReorderChildren}
  onClose={() => { moveEntryDialogPath = null; }}
/>

<!-- Launch zoom overlay (lives outside WelcomeScreen so it persists during transition) -->
{#if launchOverlay?.launchRect}
  <div
    class="launch-overlay"
    class:launch-done={launchOverlayDone}
    style="
      --start-x: {launchOverlay.launchRect.left}px;
      --start-y: {launchOverlay.launchRect.top}px;
      --start-w: {launchOverlay.launchRect.width}px;
      --start-h: {launchOverlay.launchRect.height}px;
    "
  >
    <div class="launch-zoom">
      <iframe
        src={launchOverlay.previewUrl}
        title="Launching"
        class="launch-iframe"
      ></iframe>
      <div class="launch-loading-overlay">
        <Loader2 class="size-6 animate-spin text-muted-foreground" />
        <span class="text-sm text-muted-foreground">Setting up your workspace…</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .launch-overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    animation: overlayFade 0.3s ease-out forwards;
  }

  .launch-overlay.launch-done {
    animation: launchFadeOut 0.5s ease-out forwards;
  }

  @keyframes overlayFade {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @keyframes launchFadeOut {
    from { opacity: 1; }
    to   { opacity: 0; }
  }

  .launch-zoom {
    position: absolute;
    border-radius: 12px;
    overflow: hidden;
    left: var(--start-x);
    top: var(--start-y);
    width: var(--start-w);
    height: var(--start-h);
    animation: zoomToFull 0.7s cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  @keyframes zoomToFull {
    0% {
      left: var(--start-x);
      top: var(--start-y);
      width: var(--start-w);
      height: var(--start-h);
      border-radius: 12px;
    }
    100% {
      left: 0;
      top: 0;
      width: 100vw;
      height: 100vh;
      border-radius: 0;
    }
  }

  .launch-iframe {
    width: 100%;
    height: 100%;
    border: 0;
    pointer-events: none;
  }

  .launch-loading-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    background: color-mix(in oklch, var(--background) 70%, transparent);
    backdrop-filter: blur(4px);
    opacity: 0;
    animation: loadingFadeIn 0.4s ease-out 0.7s forwards;
  }

  @keyframes loadingFadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }
</style>
