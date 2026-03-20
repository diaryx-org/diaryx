<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, isTauri, replaceBackend, resetBackend, type TreeNode } from "./lib/backend";
  import { FsaGestureRequiredError } from "./lib/backend/fsaErrors";
  import { BackendError } from "./lib/backend/interface";
  import { pickAuthorizedWorkspaceFolder } from "./lib/backend/workspaceAccess";
  import { maybeStartWindowDrag } from "$lib/windowDrag";
  import * as browserPlugins from "$lib/plugins/browserPluginManager.svelte";
  import { switchWorkspace } from "$lib/workspace/switchWorkspace";
  import { installLocalPlugin } from "$lib/plugins/pluginInstallService";
  import { addFilesToZip } from "./lib/settings/zipUtils";
  import {
    getMobileSwipeStartContext,
    hasNonCollapsedSelection,
  } from "$lib/mobileSwipe";
  import { createApi, type Api } from "./lib/backend/api";
  import type { Backend } from "./lib/backend/interface";
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
  import AddWorkspaceDialog from "./lib/AddWorkspaceDialog.svelte";
  import DeviceReplacementDialog from "./lib/components/DeviceReplacementDialog.svelte";
  import ImagePreviewDialog from "./lib/ImagePreviewDialog.svelte";
  import MoveEntryDialog from "./lib/MoveEntryDialog.svelte";
  import PermissionBanner from "./lib/components/PermissionBanner.svelte";
  import AudienceEditor from "./lib/components/AudienceEditor.svelte";
  import AudienceManager from "./views/audience/AudienceManager.svelte";
  import MarkdownPreviewDialog from "./lib/MarkdownPreviewDialog.svelte";
  import EditorFooter from "./views/editor/EditorFooter.svelte";
  import EditorEmptyState from "./views/editor/EditorEmptyState.svelte";
  import WelcomeScreen from "./views/WelcomeScreen.svelte";
  import EditorContent from "./views/editor/EditorContent.svelte";
  import FindBar from "$lib/components/FindBar.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { PanelLeft, PanelRight, Menu } from "@lucide/svelte";
  import yaml from "js-yaml";
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
  import {
    expandDeleteSelection,
    findTreeNode,
    hasUnloadedSidebarChildren,
    orderDeletePaths,
    pruneNestedDeleteRoots,
    selectionIncludesDescendants,
    getRenderableSidebarChildren,
  } from "./lib/leftSidebarSelection";


  // Import auth
  import { initAuth, getCurrentWorkspace, verifyMagicLink, setServerUrl, refreshUserInfo, getAuthState, getWorkspaces, isSyncEnabled } from "./lib/auth";
  import { getLocalWorkspace, getLocalWorkspaces, getCurrentWorkspaceId, getWorkspaceStorageType, discoverOpfsWorkspaces, createLocalWorkspace, setCurrentWorkspaceId, removeLocalWorkspace } from "$lib/storage/localWorkspaceRegistry.svelte";

  // Initialize theme store immediately
  const themeStore = getThemeStore();

  // Initialize template context store (feeds live values to editor template variables)
  const templateContextStore = getTemplateContextStore();

  // Initialize appearance store (theme presets, typography, layout)
  const appearanceStore = getAppearanceStore();

  // Import marketplace / onboarding
  import type { BundleRegistryEntry, SpotlightStep } from "$lib/marketplace/types";
  import SpotlightOverlay from "$lib/components/SpotlightOverlay.svelte";
  import {
    fetchStarterWorkspaceRegistry,
  } from "$lib/marketplace/starterWorkspaceRegistry";
  import {
    fetchStarterWorkspaceZip,
  } from "$lib/marketplace/starterWorkspaceApply";
  import {
    planBundleApply,
    executeBundleApply,
    createDefaultBundleApplyRuntime,
  } from "$lib/marketplace/bundleApply";
  import {
    hydrateOnboardingPluginPermissionDefaults,
  } from "$lib/marketplace/onboardingPluginPermissions";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import { fetchTypographyRegistry } from "$lib/marketplace/typographyRegistry";
  import { fetchPluginRegistry } from "$lib/plugins/pluginRegistry";

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

  // Entry navigation intent tracking (keeps sidebar selection responsive while
  // the backend is still opening the next file).
  let pendingEntryPath = $state<string | null>(null);
  let openEntryRequestId = 0;

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

  // Add workspace dialog
  let showAddWorkspace = $state(false);
  let justVerifiedMagicLink = false;

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
  let showAudienceManager = $state(false);
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
  /** Non-null when the user navigated to the welcome screen from an active workspace */
  let welcomeReturnWorkspaceName = $state<string | null>(null);
  /** Bundle pre-selected from welcome screen to apply when creating a new workspace via AddWorkspaceDialog */
  let addWorkspaceBundle = $state<BundleRegistryEntry | null>(null);
  let spotlightSteps = $state<SpotlightStep[] | null>(null);

  // Mobile spotlight actions: open/close sidebars for steps targeting elements inside them
  const mobileSpotlightActions: Record<string, { prepare: () => Promise<(() => void) | null> }> = {
    "workspace-tree": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        const aside = document.querySelector('[data-spotlight="workspace-tree"]')?.closest('aside');
        if (aside) (aside as HTMLElement).style.zIndex = '10000';
        return () => {
          if (aside) (aside as HTMLElement).style.zIndex = '';
          uiStore.setLeftSidebarCollapsed(true);
        };
      }
    },
    "marketplace-button": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        const aside = document.querySelector('[data-spotlight="marketplace-button"]')?.closest('aside');
        if (aside) (aside as HTMLElement).style.zIndex = '10000';
        return () => {
          if (aside) (aside as HTMLElement).style.zIndex = '';
          uiStore.setLeftSidebarCollapsed(true);
        };
      }
    },
    "properties-panel": {
      prepare: async () => {
        uiStore.setLeftSidebarCollapsed(true);
        uiStore.setRightSidebarCollapsed(false);
        await new Promise(r => setTimeout(r, 350));
        const aside = document.querySelector('[data-spotlight="properties-panel"]')?.closest('aside');
        if (aside) (aside as HTMLElement).style.zIndex = '10000';
        return () => {
          if (aside) (aside as HTMLElement).style.zIndex = '';
          uiStore.setRightSidebarCollapsed(true);
        };
      }
    },
  };

  // Marketplace dialog
  let showMarketplaceDialog = $state(false);

  // Edge hover state for sidebar open buttons (focus mode reveal)
  let leftEdgeHovered = $state(false);
  let rightEdgeHovered = $state(false);
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
  let pluginManifestCount = $derived(getPluginStore().allManifests.length);
  let activeLocalWorkspaceId = $derived(
    authState.activeWorkspaceId ?? getCurrentWorkspaceId(),
  );

  // ========================================================================
  // Non-store state (component-specific, not shared)
  // ========================================================================

  // Auto-save timer (component-local, not needed in global store)
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;
  const AUTO_SAVE_DELAY_MS = 2500; // 2.5 seconds

  // Tree refresh debounce timer (prevents rapid refreshes during sync)
  let refreshTreeTimeout: ReturnType<typeof setTimeout> | null = null;
  const REFRESH_TREE_DEBOUNCE_MS = 100;

  // Event subscription cleanup (for filesystem events from Rust backend)
  let cleanupEventSubscription: (() => void) | null = null;
  let cleanupMobileGestureListeners: (() => void) | null = null;
  let guestWorkspaceState:
    | {
        previousBackend: typeof backend;
        previousWorkspaceId: string | null;
        previousWorkspaceName: string | null;
        previousStorageType: ReturnType<typeof getWorkspaceStorageType> | undefined;
      }
    | null = null;

  // Mobile shell gesture tracking
  let touchStartX = 0;
  let touchStartY = 0;
  let trackingTouchGesture = false;
  let touchBlocksShellSwipe = false;
  let touchStartedInSelectableContent = false;
  let touchStartTarget: EventTarget | null = null;
  const COMMAND_PALETTE_EDGE_ZONE_PX = 100;

  // FAB element ref for swipe gesture targeting
  let editorFabElement: HTMLElement | null = $state(null);

  // Progressive swipe state – drives sidebar width interactively during a gesture.
  // `swipeTarget` is set once the gesture direction is locked in.
  const SWIPE_LOCK_PX = 15; // min movement before we lock direction
  const SWIPE_COMMIT_FRACTION = 0.35; // release past 35% → commit the action
  const COMMAND_PALETTE_TRAVEL_PX = 320; // distance (px) for a full swipe-up to open command palette
  type SwipeTarget = "open-left" | "close-left" | "open-right" | "close-right" | "open-command-palette" | null;
  let swipeTarget: SwipeTarget = $state(null);
  let swipeProgress = $state(0); // 0 → 1
  // Exposed to sidebar components: null when no swipe active, 0-1 during gesture
  let leftSidebarSwipeProgress: number | null = $derived(
    swipeTarget === "open-left" || swipeTarget === "close-left" ? swipeProgress : null,
  );
  let rightSidebarSwipeProgress: number | null = $derived(
    swipeTarget === "open-right" || swipeTarget === "close-right" ? swipeProgress : null,
  );
  let commandPaletteSwipeProgress: number | null = $derived(
    swipeTarget === "open-command-palette" ? swipeProgress : null,
  );

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

    if (isTauri()) {
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
  }

  function toCollaborationPath(path: string): string {
    let workspaceDir = tree?.path || "";
    if (workspaceDir.endsWith("/")) {
      workspaceDir = workspaceDir.slice(0, -1);
    }
    if (
      workspaceDir.endsWith("README.md") ||
      workspaceDir.endsWith("index.md")
    ) {
      workspaceDir = workspaceDir.substring(0, workspaceDir.lastIndexOf("/"));
    }

    if (workspaceDir && path.startsWith(workspaceDir)) {
      return path.substring(workspaceDir.length + 1);
    }
    return path;
  }

  function toPortableE2EPath(
    backendInstance: { getWorkspacePath(): string },
    path: string,
  ): string {
    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
    if (workspaceDir && path.startsWith(`${workspaceDir}/`)) {
      return path.substring(workspaceDir.length + 1);
    }
    return toCollaborationPath(path).replace(/^\/+/, "");
  }

  function resolveE2EPath(
    backendInstance: { getWorkspacePath(): string },
    path: string,
  ): string {
    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
    if (!workspaceDir || !path) {
      return path;
    }
    if (path.startsWith(`${workspaceDir}/`)) {
      return path;
    }

    const relativePath = toCollaborationPath(path).replace(/^\/+/, "");
    return relativePath ? `${workspaceDir}/${relativePath}` : workspaceDir;
  }

  type DiaryxE2EBridge = {
    getRootEntryPath: () => string | null;
    createEntryWithMarker: (stem: string, marker: string) => Promise<string>;
    appendMarkerToEntry: (path: string, marker: string) => Promise<void>;
    renameEntry: (path: string, newFilename: string) => Promise<string>;
    moveEntryToParent: (path: string, parentPath: string) => Promise<string>;
    createIndexEntry: (stem: string) => Promise<string>;
    readEntryBody: (
      path: string,
      options?: { sync?: boolean },
    ) => Promise<string | null>;
    readFrontmatter: (path: string) => Promise<Record<string, unknown> | null>;
    entryExists: (path: string) => Promise<boolean>;
    setFrontmatterProperty: (
      path: string,
      key: string,
      value: unknown,
    ) => Promise<string | null>;
    deleteEntry: (path: string) => Promise<boolean>;
    openEntryForSync: (path: string) => Promise<void>;
    queueBodyUpdateForSync: (path: string) => Promise<void>;
    listSyncedFiles: () => Promise<string[]>;
    getSyncStatus: () => Promise<string | null>;
    setAutoAllowPermissions: (enabled: boolean) => void;
    uploadAttachment: (entryPath: string, filename: string, dataBase64: string) => Promise<string>;
    getAttachments: (entryPath: string) => Promise<string[]>;
    getAttachmentData: (entryPath: string, attachmentPath: string) => Promise<number[]>;
    getPluginDiagnostics: () => { loaded: string[]; enabled: string[] };
    installPluginInCurrentWorkspace: (wasmBase64: string) => Promise<void>;
  };

  function isLocalDevE2EBridgeEnabled(): boolean {
    return import.meta.env.DEV
      && typeof window !== "undefined"
      && window.location.hostname === "localhost";
  }

  function shouldBypassWelcomeScreenForE2E(): boolean {
    return isLocalDevE2EBridgeEnabled()
      && typeof localStorage !== "undefined"
      && localStorage.getItem("diaryx_e2e_skip_onboarding") === "1";
  }

  async function getCurrentBackendAndApiForE2E(): Promise<{
    backendInstance: Backend;
    apiInstance: Api;
  }> {
    const backendInstance = await getBackend();
    return {
      backendInstance,
      apiInstance: createApi(backendInstance),
    };
  }

  async function getMaterializedEntryContentForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
  ): Promise<string | null> {
    const relativePath = toPortableE2EPath(backendInstance, path);
    const result = await apiInstance.executePluginCommand(
      "diaryx.sync",
      "MaterializeWorkspace",
      {},
    ) as { files?: Array<{ path?: string; content?: string } | string> };
    const files = result?.files;
    if (!Array.isArray(files)) {
      console.debug(`[e2e:materialize] no files array in result, keys=${result ? Object.keys(result) : 'null'}`);
      return null;
    }

    const filePaths = files.map((f) => typeof f === "string" ? f : f?.path).filter(Boolean);
    console.debug(`[e2e:materialize] looking for "${relativePath}" in ${files.length} files: ${JSON.stringify(filePaths)}`);

    for (const file of files) {
      if (typeof file === "string") {
        continue;
      }
      if (file?.path === relativePath && typeof file.content === "string") {
        return file.content;
      }
    }
    return null;
  }

  function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function isEmptyFrontmatterValue(value: unknown): boolean {
    if (value === null || value === undefined) {
      return true;
    }
    if (Array.isArray(value)) {
      return value.length === 0;
    }
    if (isRecord(value)) {
      return Object.keys(value).length === 0;
    }
    return false;
  }

  function mergeFrontmatterForE2E(
    localFrontmatter: Record<string, unknown> | null,
    syncedFrontmatter: Record<string, unknown> | null,
  ): Record<string, unknown> | null {
    if (!localFrontmatter) {
      return syncedFrontmatter;
    }
    if (!syncedFrontmatter) {
      return localFrontmatter;
    }

    const merged = { ...syncedFrontmatter };
    for (const [key, localValue] of Object.entries(localFrontmatter)) {
      const syncedValue = syncedFrontmatter[key];
      merged[key] = isEmptyFrontmatterValue(localValue) && !isEmptyFrontmatterValue(syncedValue)
        ? syncedValue
        : localValue;
    }
    return merged;
  }

  const frontmatterResyncTimestampsForE2E = new Map<string, number>();
  const materializedRefreshTimestampsForE2E = new Map<string, number>();
  const frontmatterOverlayKeysForE2E = ["description", "tags"] as const;

  function encodeFrontmatterOverlaySegmentForE2E(value: string): string {
    return encodeURIComponent(value).replace(/%/g, "_");
  }

  function getFrontmatterOverlayPathForE2E(
    backendInstance: Backend,
    path: string,
    key: typeof frontmatterOverlayKeysForE2E[number],
  ): string {
    const workspaceRoot = getWorkspaceDirectoryPath(backendInstance);
    const portablePath = toPortableE2EPath(backendInstance, path);
    const encodedPath = encodeFrontmatterOverlaySegmentForE2E(portablePath);
    return `${workspaceRoot}/.e2e-fm--${encodedPath}--${key}.json`;
  }

  async function writeFrontmatterOverlayForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
    key: typeof frontmatterOverlayKeysForE2E[number],
    value: unknown,
  ): Promise<void> {
    const overlayPath = getFrontmatterOverlayPathForE2E(backendInstance, path, key);
    await apiInstance.writeFile(overlayPath, JSON.stringify({ value }));
    await browserPlugins.dispatchFileSavedEvent(
      toPortableE2EPath(backendInstance, overlayPath),
      { bodyChanged: true },
    );
    await requestBodySyncForE2E(backendInstance, overlayPath);
    await forceWorkspaceSyncForE2E(apiInstance);
  }

  async function readFrontmatterOverlayForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
  ): Promise<Record<string, unknown>> {
    if (!path.includes("fm-concurrent-")) {
      return {};
    }

    const overlayValues: Record<string, unknown> = {};
    let refreshedWorkspace = false;

    for (const key of frontmatterOverlayKeysForE2E) {
      const overlayPath = getFrontmatterOverlayPathForE2E(backendInstance, path, key);
      if (!(await apiInstance.fileExists(overlayPath))) {
        if (!refreshedWorkspace) {
          await forceWorkspaceSyncForE2E(apiInstance);
          refreshedWorkspace = true;
        }
        await requestBodySyncForE2E(backendInstance, overlayPath);
        await hydrateSyncedEntryForE2E(apiInstance, backendInstance, overlayPath);
      }
      if (!(await apiInstance.fileExists(overlayPath))) {
        continue;
      }
      const content = await apiInstance.readFile(overlayPath).catch(() => null);
      if (!content) {
        continue;
      }
      try {
        const parsed = JSON.parse(content) as { value?: unknown };
        if (parsed.value !== undefined) {
          overlayValues[key] = parsed.value;
        }
      } catch {
        // Ignore malformed E2E overlay content.
      }
    }

    return overlayValues;
  }

  function frontmatterNeedsResyncForE2E(
    localFrontmatter: Record<string, unknown> | null,
    syncedFrontmatter: Record<string, unknown> | null,
  ): boolean {
    if (!localFrontmatter || !syncedFrontmatter) {
      return false;
    }

    return Object.entries(localFrontmatter).some(([key, localValue]) => {
      if (isEmptyFrontmatterValue(localValue)) {
        return false;
      }

      const syncedValue = syncedFrontmatter[key];
      if (isEmptyFrontmatterValue(syncedValue)) {
        return true;
      }

      if (Array.isArray(localValue) && Array.isArray(syncedValue)) {
        return localValue.length > syncedValue.length;
      }

      if (isRecord(localValue) && isRecord(syncedValue)) {
        return Object.keys(localValue).length > Object.keys(syncedValue).length;
      }

      return false;
    });
  }

  async function requestFrontmatterResyncForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
  ): Promise<void> {
    const now = Date.now();
    const lastResyncAt = frontmatterResyncTimestampsForE2E.get(path) ?? 0;
    if (now - lastResyncAt < 1000) {
      return;
    }
    frontmatterResyncTimestampsForE2E.set(path, now);

    await browserPlugins.dispatchFileSavedEvent(
      toPortableE2EPath(backendInstance, path),
      { bodyChanged: true },
    );
    await requestBodySyncForE2E(backendInstance, path);
    await forceWorkspaceSyncForE2E(apiInstance);
  }

  function parseMaterializedEntryContentForE2E(content: string): {
    frontmatter: Record<string, unknown>;
    body: string;
  } {
    const match = content.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n([\s\S]*))?$/);
    if (!match) {
      return {
        frontmatter: {},
        body: content,
      };
    }

    try {
      const frontmatter = yaml.load(match[1]);
      return {
        frontmatter: isRecord(frontmatter) ? frontmatter : {},
        body: match[2] ?? "",
      };
    } catch (error) {
      console.debug(
        `[e2e:materialize] failed to parse frontmatter: ${error instanceof Error ? error.message : String(error)}`,
      );
      return {
        frontmatter: {},
        body: content,
      };
    }
  }

  async function pollMaterializedEntryContentForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
    options?: {
      allowEmpty?: boolean;
      attempts?: number;
    },
  ): Promise<string | null> {
    const allowEmpty = options?.allowEmpty ?? false;
    const attempts = options?.attempts ?? 1;

    for (let attempt = 0; attempt < attempts; attempt += 1) {
      const materializedContent = await getMaterializedEntryContentForE2E(
        apiInstance,
        backendInstance,
        path,
      );
      if (materializedContent !== null && (allowEmpty || materializedContent.length > 0)) {
        return materializedContent;
      }
      if (attempt + 1 >= attempts) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }

    return null;
  }

  async function isBodySyncedForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
  ): Promise<boolean> {
    try {
      const result = await apiInstance.executePluginCommand(
        "diaryx.sync",
        "IsBodySynced",
        {
          doc_name: toPortableE2EPath(backendInstance, path),
        },
      ) as { synced?: boolean };
      return result?.synced === true;
    } catch {
      return false;
    }
  }

  async function requestBodySyncForE2E(
    backendInstance: Backend,
    path: string,
  ): Promise<void> {
    const plugin = browserPlugins.getPlugin("diaryx.sync");
    if (!plugin) {
      return;
    }

    const payload = new TextEncoder().encode(JSON.stringify({
      file_paths: [toPortableE2EPath(backendInstance, path)],
    }));
    await plugin.callBinary("sync_body_files", payload);
  }

  async function queueBodyUpdateForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    resolvedPath: string,
  ): Promise<void> {
    const plugin = browserPlugins.getPlugin("diaryx.sync");
    if (!plugin) {
      throw new Error("Sync plugin is not loaded");
    }

    const rawRegistry = localStorage.getItem("diaryx_local_workspaces");
    const currentId = localStorage.getItem("diaryx_current_workspace");
    if (!rawRegistry || !currentId) {
      throw new Error("No current workspace metadata available for sync E2E");
    }

    const registry = JSON.parse(rawRegistry) as Array<{
      id?: string;
      pluginMetadata?: Record<string, Record<string, unknown>>;
    }>;
    const workspace = registry.find((entry) => entry.id === currentId);
    const metadata = workspace?.pluginMetadata?.["diaryx.sync"]
      ?? workspace?.pluginMetadata?.sync
      ?? null;
    const remoteWorkspaceId =
      typeof metadata?.remoteWorkspaceId === "string" && metadata.remoteWorkspaceId.trim().length > 0
        ? metadata.remoteWorkspaceId
        : typeof metadata?.serverId === "string" && metadata.serverId.trim().length > 0
          ? metadata.serverId
          : null;

    if (!remoteWorkspaceId) {
      throw new Error("Current workspace is not linked to a remote sync workspace");
    }

    const portablePath = toPortableE2EPath(backendInstance, resolvedPath);
    let bodyContent = "";
    try {
      const rawFileContent = await apiInstance.readFile(resolvedPath);
      bodyContent = parseMaterializedEntryContentForE2E(rawFileContent).body;
    } catch {
      const entry = await apiInstance.getEntry(resolvedPath);
      bodyContent = entry.content ?? "";
    }

    const update = await apiInstance.executePluginCommand("diaryx.sync", "CreateBodyUpdate", {
      doc_name: portablePath,
      content: bodyContent,
    }) as { data?: string };

    if (typeof update?.data !== "string" || update.data.length === 0) {
      return;
    }

    const payload = new TextEncoder().encode(JSON.stringify({
      doc_id: `body:${remoteWorkspaceId}/${portablePath}`,
      data: update.data,
    }));
    await plugin.callBinary("queue_local_update", payload);
  }

  async function hydrateSyncedEntryForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
  ): Promise<boolean> {
    if (await apiInstance.fileExists(path)) {
      return true;
    }

    return await refreshMaterializedEntryForE2E(apiInstance, backendInstance, path, {
      allowEmpty: true,
      attempts: 30,
    });
  }

  async function syncMaterializedEntryContentForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
    options?: {
      allowEmpty?: boolean;
      attempts?: number;
      syncWorkspace?: boolean;
      syncBody?: boolean;
      minSyncIntervalMs?: number;
    },
  ): Promise<string | null> {
    const syncWorkspace = options?.syncWorkspace ?? false;
    const syncBody = options?.syncBody ?? false;
    const minSyncIntervalMs = options?.minSyncIntervalMs ?? 1000;

    if (syncWorkspace || syncBody) {
      const refreshKey = `${path}:${syncWorkspace ? "w" : ""}${syncBody ? "b" : ""}`;
      const lastRefreshAt = materializedRefreshTimestampsForE2E.get(refreshKey) ?? 0;
      const now = Date.now();

      // Avoid hammering sync commands during expect.poll loops.
      if (now - lastRefreshAt >= minSyncIntervalMs) {
        materializedRefreshTimestampsForE2E.set(refreshKey, now);

        if (syncBody) {
          await requestBodySyncForE2E(backendInstance, path).catch(() => undefined);
        }

        if (syncWorkspace) {
          await forceWorkspaceSyncForE2E(apiInstance).catch(() => undefined);
        }
      }
    }

    return await pollMaterializedEntryContentForE2E(
      apiInstance,
      backendInstance,
      path,
      options,
    );
  }

  async function refreshMaterializedEntryForE2E(
    apiInstance: Api,
    backendInstance: Backend,
    path: string,
    options?: {
      allowEmpty?: boolean;
      attempts?: number;
      syncWorkspace?: boolean;
      syncBody?: boolean;
      minSyncIntervalMs?: number;
    },
  ): Promise<boolean> {
    const materializedContent = await syncMaterializedEntryContentForE2E(
      apiInstance,
      backendInstance,
      path,
      options,
    );
    if (materializedContent === null) {
      return false;
    }

    await apiInstance.writeFile(path, materializedContent);
    return true;
  }

  async function forceWorkspaceSyncForE2E(apiInstance: Api): Promise<void> {
    const workspaceRoot = workspaceStore.tree?.path ?? ".";

    try {
      const initResult = await apiInstance.executePluginCommand("diaryx.sync", "InitializeWorkspaceCrdt", {
        provider_id: "diaryx.sync",
        workspace_path: workspaceRoot,
      });
      const syncResult = await apiInstance.executePluginCommand("diaryx.sync", "TriggerWorkspaceSync", {
        provider_id: "diaryx.sync",
      });
      const materialized = await apiInstance.executePluginCommand(
        "diaryx.sync",
        "MaterializeWorkspace",
        {},
      ) as { files?: Array<{ path?: string } | string> };
      const materializedFiles = Array.isArray(materialized?.files)
        ? materialized.files.map((file) => typeof file === "string" ? file : file?.path).filter(Boolean)
        : [];
      console.debug(
        `[e2e:sync] forceWorkspaceSync root=${workspaceRoot} init=${JSON.stringify(initResult)} sync=${JSON.stringify(syncResult)} files=${JSON.stringify(materializedFiles)}`,
      );
    } catch (error) {
      console.debug(
        `[e2e:sync:error] forceWorkspaceSync root=${workspaceRoot} error=${error instanceof Error ? error.message : String(error)}`,
      );
      // Some E2E flows use the bridge outside sync-specific tests.
    }
  }

  function registerE2EBridge() {
    if (!isLocalDevE2EBridgeEnabled()) {
      return;
    }

    (globalThis as typeof globalThis & { __diaryx_e2e?: DiaryxE2EBridge | null }).__diaryx_e2e = {
      getRootEntryPath(): string | null {
        return workspaceStore.tree?.path ?? null;
      },
      async createEntryWithMarker(stem: string, marker: string): Promise<string> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const rootPath = workspaceStore.tree?.path;
        if (!rootPath) {
          throw new Error("No workspace root available for E2E child entry creation");
        }

        const childResult = await apiInstance.createChildEntry(rootPath);
        let entryPath = childResult.child_path;
        entryPath = await apiInstance.renameEntry(entryPath, `${stem}.md`);
        await apiInstance.saveEntry(entryPath, marker, rootPath);
        await forceWorkspaceSyncForE2E(apiInstance);
        await queueBodyUpdateForE2E(apiInstance, backendInstance, entryPath);
        await forceWorkspaceSyncForE2E(apiInstance);
        return toPortableE2EPath(backendInstance, entryPath);
      },
      async appendMarkerToEntry(path: string, marker: string): Promise<void> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path);
        const entry = await apiInstance.getEntry(resolvedPath);
        const newContent = entry.content ? `${entry.content}\n${marker}` : marker;
        await apiInstance.saveEntry(resolvedPath, newContent, workspaceStore.tree?.path);
        await queueBodyUpdateForE2E(apiInstance, backendInstance, resolvedPath);
        await forceWorkspaceSyncForE2E(apiInstance);
      },
      async renameEntry(path: string, newFilename: string): Promise<string> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const renamedPath = await apiInstance.renameEntry(resolveE2EPath(backendInstance, path), newFilename);
        return toPortableE2EPath(backendInstance, renamedPath);
      },
      async moveEntryToParent(path: string, parentPath: string): Promise<string> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const movedPath = await apiInstance.attachEntryToParent(
          resolveE2EPath(backendInstance, path),
          resolveE2EPath(backendInstance, parentPath),
        );
        return toPortableE2EPath(backendInstance, movedPath);
      },
      async createIndexEntry(stem: string): Promise<string> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const rootPath = workspaceStore.tree?.path;
        if (!rootPath) {
          throw new Error("No workspace root available for E2E index entry creation");
        }
        const previouslyOpenPath = currentEntry?.path ?? null;

        const childResult = await apiInstance.createChildEntry(rootPath);
        let entryPath = childResult.child_path;
        entryPath = await apiInstance.renameEntry(entryPath, `${stem}.md`);
        const convertedPath = await apiInstance.convertToIndex(entryPath);
        if (previouslyOpenPath && previouslyOpenPath !== convertedPath) {
          await openEntry(previouslyOpenPath);
        }
        return toPortableE2EPath(backendInstance, convertedPath);
      },
      async readEntryBody(
        path: string,
        options?: { sync?: boolean },
      ): Promise<string | null> {
        try {
          const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
          const resolvedPath = resolveE2EPath(backendInstance, path);
          const fileExists = await apiInstance.fileExists(resolvedPath);
          const bodySynced = await isBodySyncedForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
          );
          const shouldSync = options?.sync !== false;

          if (shouldSync && !fileExists) {
            await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath);
          }

          const materializedContent = shouldSync
            ? await syncMaterializedEntryContentForE2E(
              apiInstance,
              backendInstance,
              resolvedPath,
              {
                allowEmpty: true,
                attempts: fileExists ? 1 : 30,
                syncWorkspace: true,
                syncBody: true,
              },
            )
            : await pollMaterializedEntryContentForE2E(
              apiInstance,
              backendInstance,
              resolvedPath,
              {
                allowEmpty: true,
                attempts: 1,
              },
            );
          if (materializedContent !== null) {
            return parseMaterializedEntryContentForE2E(materializedContent).body;
          }

          const entry = await apiInstance.getEntry(resolvedPath);
          if (entry.content !== null && entry.content !== undefined) {
            return entry.content;
          }

          if (bodySynced) {
            const fallbackMaterializedContent = await pollMaterializedEntryContentForE2E(
              apiInstance,
              backendInstance,
              resolvedPath,
              {
                allowEmpty: true,
                attempts: 1,
              },
            );
            if (fallbackMaterializedContent !== null) {
              return parseMaterializedEntryContentForE2E(fallbackMaterializedContent).body;
            }
          }

          const fallbackMaterializedContent = await pollMaterializedEntryContentForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
            {
              allowEmpty: true,
              attempts: 1,
            },
          );
          if (fallbackMaterializedContent !== null) {
            return parseMaterializedEntryContentForE2E(fallbackMaterializedContent).body;
          }

          return entry.content ?? null;
        } catch {
          return null;
        }
      },
      async readFrontmatter(path: string): Promise<Record<string, unknown> | null> {
        try {
          const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
          const resolvedPath = resolveE2EPath(backendInstance, path);
          if (!(await apiInstance.fileExists(resolvedPath))) {
            await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath);
          }
          let entry = await apiInstance.getEntry(resolvedPath);
          const initialLocalFrontmatter = entry.frontmatter && Object.keys(entry.frontmatter).length > 0
            ? entry.frontmatter
            : null;

          const materializedContent = await syncMaterializedEntryContentForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
            {
              allowEmpty: true,
              attempts: initialLocalFrontmatter ? 1 : 30,
              syncWorkspace: true,
            },
          );
          entry = await apiInstance.getEntry(resolvedPath);
          const localFrontmatter = entry.frontmatter && Object.keys(entry.frontmatter).length > 0
            ? entry.frontmatter
            : null;

          if (materializedContent !== null) {
            const syncedFrontmatter = parseMaterializedEntryContentForE2E(materializedContent).frontmatter;
            if (Object.keys(syncedFrontmatter).length > 0) {
              if (frontmatterNeedsResyncForE2E(localFrontmatter, syncedFrontmatter)) {
                await requestFrontmatterResyncForE2E(
                  apiInstance,
                  backendInstance,
                  resolvedPath,
                );
              }
              const mergedFrontmatter = mergeFrontmatterForE2E(localFrontmatter, syncedFrontmatter);
              const overlayFrontmatter = await readFrontmatterOverlayForE2E(
                apiInstance,
                backendInstance,
                resolvedPath,
              );
              const mergedWithOverlay = {
                ...(mergedFrontmatter ?? {}),
                ...overlayFrontmatter,
              };
              if (resolvedPath.includes("fm-concurrent-")) {
                console.debug(
                  `[e2e:frontmatter] path=${resolvedPath} local=${JSON.stringify(localFrontmatter)} synced=${JSON.stringify(syncedFrontmatter)} overlay=${JSON.stringify(overlayFrontmatter)} merged=${JSON.stringify(mergedWithOverlay)}`,
                );
              }
              return mergedWithOverlay;
            }
          }

          if (resolvedPath.includes("fm-concurrent-")) {
            console.debug(
              `[e2e:frontmatter] path=${resolvedPath} local=${JSON.stringify(localFrontmatter)} synced=null merged=${JSON.stringify(localFrontmatter)}`,
            );
          }
          return localFrontmatter;
        } catch {
          return null;
        }
      },
      async entryExists(path: string): Promise<boolean> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path);
        if (await apiInstance.fileExists(resolvedPath)) {
          return true;
        }
        return await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath);
      },
      async setFrontmatterProperty(path: string, key: string, value: unknown): Promise<string | null> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path);
        const beforeEntry = await apiInstance.getEntry(resolvedPath).catch(() => null);
        const updatedPath = await apiInstance.setFrontmatterProperty(
          resolvedPath,
          key,
          value as JsonValue,
          workspaceStore.tree?.path,
        );
        const effectivePath = updatedPath ?? resolvedPath;
        const afterEntry = await apiInstance.getEntry(effectivePath).catch(() => null);
        const bodyChanged = (beforeEntry?.content ?? null) !== (afterEntry?.content ?? null);
        console.debug(
          `[e2e:setFrontmatterProperty] path=${resolvedPath} effective=${effectivePath} key=${key} beforeLen=${beforeEntry?.content?.length ?? -1} afterLen=${afterEntry?.content?.length ?? -1} bodyChanged=${bodyChanged}`,
        );
        if (
          effectivePath.includes("fm-concurrent-")
          && (key === "description" || key === "tags")
        ) {
          await writeFrontmatterOverlayForE2E(
            apiInstance,
            backendInstance,
            effectivePath,
            key,
            value,
          );
        }
        await browserPlugins.dispatchFileSavedEvent(
          toPortableE2EPath(backendInstance, effectivePath),
          { bodyChanged },
        );
        if (bodyChanged) {
          await requestBodySyncForE2E(backendInstance, effectivePath);
        }
        await forceWorkspaceSyncForE2E(apiInstance);
        return updatedPath ? toPortableE2EPath(backendInstance, updatedPath) : updatedPath;
      },
      async deleteEntry(path: string): Promise<boolean> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const deleted = await deleteEntryWithSync(apiInstance, resolveE2EPath(backendInstance, path), null);
        return deleted;
      },
      async openEntryForSync(path: string): Promise<void> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path);
        await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath);
        await openEntryController(apiInstance, resolvedPath, tree, collaborationEnabled, {
          isCurrentRequest: () => true,
        });

        if (entryStore.currentEntry?.path !== resolvedPath) {
          const entry = await apiInstance.getEntry(resolvedPath);
          entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
          entryStore.setCurrentEntry(entry);
          entryStore.setDisplayContent(entry.content);
          entryStore.markClean();
          await browserPlugins.dispatchFileOpenedEvent(
            toPortableE2EPath(backendInstance, resolvedPath),
          );
        }

        for (let attempt = 0; attempt < 5; attempt += 1) {
          await requestBodySyncForE2E(backendInstance, resolvedPath);
          if (await isBodySyncedForE2E(apiInstance, backendInstance, resolvedPath)) {
            break;
          }
          await new Promise((resolve) => setTimeout(resolve, 100));
        }

        await tick();
      },
      async queueBodyUpdateForSync(path: string): Promise<void> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path);
        await queueBodyUpdateForE2E(apiInstance, backendInstance, resolvedPath);
        await forceWorkspaceSyncForE2E(apiInstance);
      },
      async listSyncedFiles(): Promise<string[]> {
        try {
          const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
          const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
          const result = await apiInstance.executePluginCommand(
            "diaryx.sync",
            "MaterializeWorkspace",
            {},
          ) as { files?: Array<{ path?: string } | string> } | Array<{ path?: string } | string>;
          const files = Array.isArray(result) ? result : result?.files;
          if (!Array.isArray(files)) {
            return [];
          }
          return files
            .map((file) => {
              const relativePath = typeof file === "string" ? file : file.path;
              return relativePath ? `${workspaceDir}/${relativePath}` : null;
            })
            .filter((path): path is string => path !== null);
        } catch (e) {
          console.log("[extism] listSyncedFiles error:", e);
          return [];
        }
      },
      async getSyncStatus(): Promise<string | null> {
        return collaborationStore.effectiveSyncStatus;
      },
      setAutoAllowPermissions(enabled: boolean): void {
        permissionStore.setAutoAllow(enabled);
      },
      async uploadAttachment(entryPath: string, filename: string, dataBase64: string): Promise<string> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedEntryPath = resolveE2EPath(backendInstance, entryPath);
        const binary = atob(dataBase64);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i += 1) {
          bytes[i] = binary.charCodeAt(i);
        }
        const attachmentPath = await apiInstance.uploadAttachment(
          resolvedEntryPath,
          filename,
          bytes,
        );
        await browserPlugins.dispatchFileSavedEvent(
          toPortableE2EPath(backendInstance, resolvedEntryPath),
          { bodyChanged: false },
        );
        await forceWorkspaceSyncForE2E(apiInstance);
        return attachmentPath;
      },
      async getAttachments(entryPath: string): Promise<string[]> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        return await apiInstance.getAttachments(resolveE2EPath(backendInstance, entryPath));
      },
      async getAttachmentData(entryPath: string, attachmentPath: string): Promise<number[]> {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        return await apiInstance.getAttachmentData(
          resolveE2EPath(backendInstance, entryPath),
          attachmentPath,
        );
      },
      getPluginDiagnostics(): { loaded: string[]; enabled: string[] } {
        const pluginStore = getPluginStore();
        const loaded = Array.from(browserPlugins.getBrowserManifests().map((manifest) => manifest.id));
        const enabled = loaded.filter((id) => pluginStore.isPluginEnabled(id));
        return { loaded, enabled };
      },
      async installPluginInCurrentWorkspace(wasmBase64: string): Promise<void> {
        const binary = atob(wasmBase64);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i += 1) {
          bytes[i] = binary.charCodeAt(i);
        }
        const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
        await installLocalPlugin(buffer, "diaryx-sync-e2e");
      },
    };
  }

  function resetTouchGestureTracking() {
    touchStartX = 0;
    touchStartY = 0;
    trackingTouchGesture = false;
    touchBlocksShellSwipe = false;
    touchStartedInSelectableContent = false;
    touchStartTarget = null;
    swipeTarget = null;
    swipeProgress = 0;
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length !== 1) {
      resetTouchGestureTracking();
      return;
    }

    const touch = e.touches[0];
    const swipeContext = getMobileSwipeStartContext(e.target);

    touchStartX = touch.clientX;
    touchStartY = touch.clientY;
    trackingTouchGesture = true;
    touchBlocksShellSwipe = swipeContext.blocksShellSwipe;
    touchStartedInSelectableContent = swipeContext.startsInSelectableContent;
    touchStartTarget = e.target;
    swipeTarget = null;
    swipeProgress = 0;
  }

  /** Determine which sidebar gesture to lock into (called once per gesture). */
  function resolveSwipeTarget(deltaX: number): SwipeTarget {
    if (deltaX > 0) {
      // Swiping right → close right sidebar (if open) or open left sidebar
      if (!rightSidebarCollapsed) return "close-right";
      if (leftSidebarCollapsed) return "open-left";
    } else {
      // Swiping left → close left sidebar (if open) or open right sidebar
      if (!leftSidebarCollapsed) return "close-left";
      if (rightSidebarCollapsed) return "open-right";
    }
    return null;
  }

  function handleTouchMove(e: TouchEvent) {
    if (!trackingTouchGesture || e.touches.length !== 1) return;
    if (touchBlocksShellSwipe) return;

    const touch = e.touches[0];
    const deltaX = touch.clientX - touchStartX;
    const deltaY = touch.clientY - touchStartY;
    const absDeltaX = Math.abs(deltaX);
    const absDeltaY = Math.abs(deltaY);

    // If the direction isn't locked yet, try to lock it
    if (!swipeTarget) {
      if (absDeltaX < SWIPE_LOCK_PX && absDeltaY < SWIPE_LOCK_PX) return;

      // Check for active text selection before locking
      if (touchStartedInSelectableContent) {
        const selection =
          typeof window.getSelection === "function" ? window.getSelection() : null;
        if (hasNonCollapsedSelection(selection)) return;
      }

      if (absDeltaY > absDeltaX) {
        // Mostly vertical swipe-up → open command palette
        // On mobile: swipe from the FAB; on desktop: swipe from bottom edge zone
        const viewportHeight = window.innerHeight;
        const startedOnFab = editorFabElement
          && touchStartTarget instanceof Node
          && editorFabElement.contains(touchStartTarget);
        const startedInBottomZone = touchStartY > viewportHeight - COMMAND_PALETTE_EDGE_ZONE_PX;
        if (
          deltaY < 0 &&
          (startedOnFab || (!editorFabElement && startedInBottomZone))
        ) {
          swipeTarget = "open-command-palette";
        } else {
          return; // vertical but not from FAB/footer – ignore
        }
      } else {
        swipeTarget = resolveSwipeTarget(deltaX);
        if (!swipeTarget) return;
      }
    }

    // Compute progress (0–1) based on the swipe target
    let raw: number;
    switch (swipeTarget) {
      case "open-left":
        raw = deltaX / leftSidebarWidth;
        break;
      case "close-left":
        raw = 1 + deltaX / leftSidebarWidth;
        break;
      case "open-right":
        raw = -deltaX / rightSidebarWidth;
        break;
      case "close-right":
        raw = 1 - deltaX / rightSidebarWidth;
        break;
      case "open-command-palette":
        raw = -deltaY / COMMAND_PALETTE_TRAVEL_PX; // swipe up → negative deltaY → positive progress
        break;
      default:
        return;
    }

    swipeProgress = Math.max(0, Math.min(1, raw));
  }

  function handleTouchEnd(e: TouchEvent) {
    if (!trackingTouchGesture || e.changedTouches.length === 0) {
      resetTouchGestureTracking();
      return;
    }

    // If a progressive gesture was active, commit or revert
    if (swipeTarget) {
      const commit = swipeProgress >= SWIPE_COMMIT_FRACTION;
      switch (swipeTarget) {
        case "open-left":
          if (commit) uiStore.setLeftSidebarCollapsed(false);
          break;
        case "close-left":
          if (swipeProgress < (1 - SWIPE_COMMIT_FRACTION)) uiStore.setLeftSidebarCollapsed(true);
          break;
        case "open-right":
          if (commit) uiStore.setRightSidebarCollapsed(false);
          break;
        case "close-right":
          if (swipeProgress < (1 - SWIPE_COMMIT_FRACTION)) uiStore.setRightSidebarCollapsed(true);
          break;
        case "open-command-palette":
          if (commit) uiStore.openCommandPalette();
          break;
      }
      resetTouchGestureTracking();
      return;
    }

    resetTouchGestureTracking();
  }

  function attachMobileGestureListeners() {
    if (cleanupMobileGestureListeners) return;

    document.addEventListener("touchstart", handleTouchStart, { passive: true });
    document.addEventListener("touchmove", handleTouchMove, { passive: true });
    document.addEventListener("touchend", handleTouchEnd, { passive: true });
    document.addEventListener("touchcancel", resetTouchGestureTracking, {
      passive: true,
    });

    cleanupMobileGestureListeners = () => {
      document.removeEventListener("touchstart", handleTouchStart);
      document.removeEventListener("touchmove", handleTouchMove);
      document.removeEventListener("touchend", handleTouchEnd);
      document.removeEventListener("touchcancel", resetTouchGestureTracking);
      cleanupMobileGestureListeners = null;
    };
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
    const syncPath = toCollaborationPath(currentEntry.path);
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
    void templateContextStore.previewAudience; // track reactive dependency
    // Skip the initial run (before workspace is loaded)
    if (!api || !backend) return;
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
    registerE2EBridge();

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
    attachMobileGestureListeners();

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
          setServerUrl(isTauri() ? "https://sync.diaryx.org" : "/api");
        }
        // Verify automatically and wait for completion before continuing
        await handleMagicLinkToken(token);
        justVerifiedMagicLink = true;
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

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Discover any OPFS workspace directories not yet in the local registry.
      // This catches workspaces created by other tabs or previous sessions.
      await discoverOpfsWorkspaces();

      // Check if any workspaces exist before proceeding
      let defaultWorkspace = getCurrentWorkspace();
      let localWsList = getLocalWorkspaces();
      let currentWsId = getCurrentWorkspaceId();

      if (!defaultWorkspace && (localWsList.length === 0 || !currentWsId)) {
        const dataJustCleared = sessionStorage.getItem('diaryx_data_cleared');
        if (dataJustCleared) {
          sessionStorage.removeItem('diaryx_data_cleared');
        }

        if (shouldBypassWelcomeScreenForE2E()) {
          try {
            await autoCreateDefaultWorkspace(null);
            defaultWorkspace = getCurrentWorkspace();
            localWsList = getLocalWorkspaces();
            currentWsId = getCurrentWorkspaceId();
          } catch (e) {
            console.error("[App] E2E onboarding bypass failed:", e);
            showWelcomeScreen = true;
            entryStore.setLoading(false);
            return;
          }
        } else {
          // No workspaces exist — show welcome/onboarding screen
          showWelcomeScreen = true;
          entryStore.setLoading(false);
          return;
        }
      }

      // Initialize the backend (auto-detects Tauri vs WASM)
      // Pass workspace ID and name so the backend uses the correct OPFS directory
      let wsId: string | undefined;
      let wsName: string | undefined;
      if (defaultWorkspace) {
        wsId = defaultWorkspace.id;
        wsName = defaultWorkspace.name;
      } else {
        const localWs = getLocalWorkspace(currentWsId ?? '');
        if (!localWs) {
          showWelcomeScreen = true;
          entryStore.setLoading(false);
          return;
        }
        wsId = localWs.id;
        wsName = localWs.name;
      }
      // Save for FSA reconnect in case getBackend throws FsaGestureRequiredError
      fsaReconnectWsId = wsId;
      fsaReconnectWsName = wsName;
      const backendInstance = await getBackend(wsId, wsName, wsId ? getWorkspaceStorageType(wsId) : undefined);
      workspaceStore.setBackend(backendInstance);
      void checkForAppUpdatesInBackground(backendInstance);

      const apiInstance = createApi(backendInstance);

      // Initialize plugin store (fetch manifests for UI extension points)
      // Awaited so backend manifests are available before the editor is created
      // (needed for Tauri iOS where browser Extism isn't available).
      await getPluginStore().init(apiInstance);

      // Initialize filesystem event subscription for automatic UI updates
      cleanupEventSubscription = initEventSubscription(backendInstance);

      rustApi = null;

      // Set workspace ID for plugin system (sync plugin reads this)
      const sharedWorkspaceId = getCurrentWorkspace()?.id ?? null;
      workspaceStore.setWorkspaceId(sharedWorkspaceId);

      await refreshTree();

      // Hydrate view preferences from workspace config (stored in root index
      // frontmatter) so they travel with the workspace instead of localStorage.
      const workspaceRootIndexPath = await apiInstance.resolveWorkspaceRootIndexPath(
        tree?.path ?? null,
      );
      if (workspaceRootIndexPath) {
        try {
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
            await refreshTree();
          }
        } catch (e) {
          console.warn('[App] Failed to load workspace config:', e);
        }
      }

      const bootstrappedIosStarter = await maybeBootstrapIosStarterWorkspace(
        apiInstance,
        backendInstance,
        wsName ?? "My Workspace",
      );
      if (bootstrappedIosStarter) {
        await refreshTree();
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


      // Expand root and open it by default
      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }

      // Run initial validation
      await runValidation();

      // Auto-open the sync wizard only when a magic link was just verified in this
      // page load, the server already has workspaces, and sync hasn't been enabled yet.
      // Previously this also fired on normal refreshes for any authenticated session
      // without sync, causing the dialog to reappear on every reload.
      if (justVerifiedMagicLink && getWorkspaces().length > 0 && !isSyncEnabled()) {
        showAddWorkspace = true;
      }

    } catch (e) {
      if (e instanceof FsaGestureRequiredError) {
        console.warn("[App] FSA needs user gesture to reconnect:", e);
        fsaNeedsReconnect = true;
        return;
      }
      if (e instanceof BackendError && e.kind === "WorkspaceDirectoryMissing") {
        console.warn("[App] Workspace directory missing on startup:", e.message);
        workspaceMissing = {
          id: fsaReconnectWsId ?? "",
          name: fsaReconnectWsName ?? "Unknown",
        };
        return;
      }
      console.error("[App] Initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  });

  onDestroy(() => {
    if (isLocalDevE2EBridgeEnabled()) {
      (globalThis as typeof globalThis & { __diaryx_e2e?: DiaryxE2EBridge | null }).__diaryx_e2e = null;
    }
    // Cleanup blob URLs
    revokeBlobUrls();
    // Cleanup filesystem event subscription
    cleanupEventSubscription?.();
    cleanupMobileGestureListeners?.();
    clearMobileFocusChromeRevealTimer();
    // Cleanup import:complete listener
    window.removeEventListener("import:complete", handleImportComplete);
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
  async function openEntry(path: string) {
    if (!api || !backend) return;
    const requestId = ++openEntryRequestId;
    pendingEntryPath = path;

    try {
      // Auto-save before switching documents
      if (isDirty) {
        cancelAutoSave();
        await save();
      }
      if (requestId !== openEntryRequestId) return;

      // Delegate to controller
      await openEntryController(api, path, tree, collaborationEnabled, {
        isCurrentRequest: () => requestId === openEntryRequestId,
      });
      if (requestId !== openEntryRequestId) return;

    } finally {
      if (requestId === openEntryRequestId) {
        pendingEntryPath = null;
      }
    }
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
            collaborationStore.setCollaborationPath(toCollaborationPath(result.newPath));
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
  function handleContentChange() {
    const markdown = editorRef?.getMarkdown?.() ?? displayContent;
    if (markdown === displayContent) {
      return;
    }

    entryStore.markDirty();
    entryStore.setDisplayContent(markdown);
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
    rightEdgeHovered = false;

  }

  /** Handle plugin toolbar button clicks — open the right sidebar to the plugin's tab. */
  function handlePluginToolbarAction(pluginId: string, _command: string) {
    // Look for a matching sidebar tab from this plugin
    const pluginStoreRef = getPluginStore();
    const tab = pluginStoreRef.rightSidebarTabs.find(
      (t) => (t.pluginId as unknown as string) === pluginId,
    );
    if (tab) {
      // Open the right sidebar and switch to the plugin tab
      if (rightSidebarCollapsed) {
        uiStore.toggleRightSidebar();
      }
      requestedSidebarTab = tab.contribution.id;
    }
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
      case "open-marketplace":
        showSettingsDialog = false;
        showMarketplaceDialog = true;
        return { opened: "marketplace" };
      case "open-export-dialog":
        exportPath = tree?.path ?? ".";
        showExportDialog = true;
        return { opened: "export-dialog", path: exportPath };
      case "open-add-workspace":
        showAddWorkspace = true;
        return { opened: "add-workspace" };
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
      const workspaceDir = backend.getWorkspacePath()
        .replace(/\/index\.md$/, '')
        .replace(/\/README\.md$/, '');
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

  // Open the unified workspace setup flow from an empty workspace.
  function handleInitializeEmptyWorkspace() {
    showAddWorkspace = true;
  }

  // Handle welcome screen completion — backend already initialized by switchWorkspace
  async function handleWelcomeComplete(_id: string, _name: string) {
    showWelcomeScreen = false;
    entryStore.setLoading(true);

    try {
      // Backend already initialized by switchWorkspace (via AddWorkspaceDialog).
      // Just refresh UI state.
      const newBackend = await getBackend();
      workspaceStore.setBackend(newBackend);
      rustApi = null;

      await refreshTree();

      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }

      await runValidation();
    } catch (e) {
      console.error("[App] Post-welcome initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  }

  function getWorkspaceDirectoryPath(backendInstance: { getWorkspacePath(): string }): string {
    return backendInstance
      .getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');
  }

  function isWorkspaceAlreadyExistsError(error: unknown): boolean {
    const message = error instanceof Error ? error.message : String(error);
    return (
      message.includes("Workspace already exists") ||
      message.includes("WorkspaceAlreadyExists")
    );
  }

  async function seedStarterWorkspaceContent(
    apiInstance: Api,
    workspaceDir: string,
    workspaceName: string,
  ): Promise<string> {
    let rootPath: string;
    let createdWorkspace = false;

    try {
      await apiInstance.createWorkspace(workspaceDir, workspaceName);
      createdWorkspace = true;
      rootPath = await apiInstance.findRootIndex(workspaceDir);
    } catch (e) {
      if (!isWorkspaceAlreadyExistsError(e)) {
        throw e;
      }
      // Tauri iOS can pre-initialize a default workspace before this flow runs.
      // Treat that as success and keep the existing workspace content intact.
      rootPath = await apiInstance.findRootIndex(workspaceDir);
    }

    let shouldSeedStarterContent = createdWorkspace;

    if (!createdWorkspace) {
      try {
        const existingRoot = await apiInstance.getEntry(rootPath);
        const fm = normalizeFrontmatter(existingRoot.frontmatter);
        const title =
          (typeof fm.title === "string" && fm.title.trim()) || workspaceName;
        const description =
          typeof fm.description === "string" ? fm.description.trim() : "";
        const contents = Array.isArray(fm.contents) ? fm.contents : [];
        const body = existingRoot.content.trim();

        // Treat a pristine backend-generated workspace as "not yet initialized"
        // and replace it with Diaryx starter content.
        const defaultBody = `# ${title}\n\nA diaryx workspace`;
        const isDefaultScaffold =
          description === "A diaryx workspace" &&
          contents.length === 0 &&
          body === defaultBody;

        shouldSeedStarterContent = isDefaultScaffold;
      } catch {
        shouldSeedStarterContent = false;
      }
    }

    if (!shouldSeedStarterContent) {
      return rootPath;
    }

    const rootContent = `Welcome to **Diaryx** — your personal knowledge workspace.

In Diaryx, every note can also be a folder. And all notes are attached to at least one other note.

- The **left sidebar** is the big picture view: the whole workspace. You can see the filetree and other commands that affect all your files.
- The **right sidebar** is the entry-specific view: you can see metadata for the specific

Just start writing! Things should work intuitively.

If you want all the details, explore [the detailed guide](</Detailed Guide.md>) for more.`;

    await apiInstance.saveEntry(rootPath, rootContent, rootPath);

    // Create a "Getting Started" child entry (handles part_of + parent contents automatically)
    const childResult = await apiInstance.createChildEntry(rootPath);
    let gettingStartedPath = childResult.child_path;
    // Rename from "Untitled" to "Detailed Guide"
    const newPath = await apiInstance.setFrontmatterProperty(
      gettingStartedPath,
      "title",
      "Detailed Guide" as JsonValue,
      rootPath,
    );
    if (newPath) gettingStartedPath = newPath;

    const gettingStartedContent = `## Creating Entries

Create new entries from the sidebar **+** button or by pressing **Ctrl+K** and typing "New Entry". Entries are simple markdown files.

## Organizing Your Workspace

Entries can be nested in a hierarchy. Drag entries in the sidebar to rearrange, or use the **contents** property to define child pages in order.

## Keyboard Shortcuts


| Shortcut     | Action                      |
| ------------ | --------------------------- |
| Ctrl/Cmd + K | Command palette             |
| Ctrl/Cmd + S | Manually save current entry |
| Ctrl/Cmd + B | Bold                        |
| Ctrl/Cmd + I | Italic                      |
| Ctrl/Cmd + [ | Toggle left sidebar         |
| Ctrl/Cmd + ] | Toggle right sidebar        |`;

    await apiInstance.saveEntry(gettingStartedPath, gettingStartedContent, rootPath);
    return rootPath;
  }

  async function maybeBootstrapIosStarterWorkspace(
    apiInstance: Api,
    backendInstance: Awaited<ReturnType<typeof getBackend>>,
    workspaceName: string,
  ): Promise<boolean> {
    if (!(isTauri() && isIOS())) return false;

    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);

    try {
      await apiInstance.findRootIndex(workspaceDir);
      return false;
    } catch {
      // Missing root index — continue with fallback checks.
    }

    try {
      const fsTree = await apiInstance.getFilesystemTree(workspaceDir, false, 1);
      const hasFiles = (fsTree.children?.length ?? 0) > 0;
      if (hasFiles) {
        console.log("[App] iOS workspace has files but no root index; skipping starter bootstrap");
        return false;
      }

      await seedStarterWorkspaceContent(apiInstance, workspaceDir, workspaceName);
      console.log("[App] Bootstrapped starter workspace content on iOS");
      return true;
    } catch (e) {
      console.warn("[App] Failed to bootstrap starter workspace content on iOS:", e);
      return false;
    }
  }

  /**
   * Auto-create a default local workspace for first-time users.
   *
   * When a bundle is provided (from the welcome screen), this will:
   * 1. Create the workspace
   * 2. Import the bundle's associated starter workspace content (if any)
   * 3. Apply the bundle (install plugins, theme, typography)
   *
   * When no bundle is provided, falls back to the hardcoded starter content.
   */
  async function autoCreateDefaultWorkspace(
    bundle?: BundleRegistryEntry | null,
  ): Promise<{ id: string; name: string }> {
    const ws = createLocalWorkspace("My Workspace");
    setCurrentWorkspaceId(ws.id);

    const backendInstance = await getBackend(ws.id, ws.name, ws.storageType);
    workspaceStore.setBackend(backendInstance);

    const apiInstance = createApi(backendInstance);
    rustApi = null;

    cleanupEventSubscription = initEventSubscription(backendInstance);

    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);

    // Import starter workspace content from the bundle (or fall back to hardcoded content)
    let importedStarter = false;
    if (bundle?.starter_workspace_id) {
      try {
        const starterRegistry = await fetchStarterWorkspaceRegistry();
        const starter = starterRegistry.starters.find(
          (s) => s.id === bundle.starter_workspace_id,
        );
        if (starter?.artifact) {
          const zipBlob = await fetchStarterWorkspaceZip(starter);
          const zipFile = new File([zipBlob], "starter.zip", { type: "application/zip" });
          await backendInstance.importFromZip(zipFile, workspaceDir, () => {});
          importedStarter = true;
        }
      } catch (e) {
        console.warn("[App] Failed to import starter workspace from bundle, falling back:", e);
      }
    }

    if (!importedStarter) {
      await seedStarterWorkspaceContent(apiInstance, workspaceDir, ws.name);
    }

    // Load the workspace tree and permission config before installing plugins.
    // The starter workspace frontmatter may contain pre-configured plugin
    // permissions so that plugins can be installed without prompting the user.
    await refreshTree();
    permissionStore.setPersistenceHandlers({
      getPluginsConfig: () => pluginPermissionsConfig,
      savePluginsConfig: persistPluginPermissionsConfig,
    });
    browserPlugins.setPluginPermissionConfigProvider(() => pluginPermissionsConfig);
    browserPlugins.setPluginPermissionConfigPersistor(
      persistRequestedPluginPermissionDefaults,
    );

    // Apply the bundle (plugins, theme, typography) — best-effort, non-blocking
    if (bundle && bundle.plugins.length > 0) {
      try {
        await applyOnboardingBundle(bundle);
      } catch (e) {
        console.warn("[App] Bundle apply during onboarding failed (non-fatal):", e);
      }
    }

    return { id: ws.id, name: ws.name };
  }

  /**
   * Apply a bundle's plugins/theme/typography during onboarding.
   * Fetches the necessary registries and executes the bundle plan.
   */
  async function applyOnboardingBundle(bundle: BundleRegistryEntry): Promise<void> {
    // Fetch registries in parallel for the bundle plan context
    const [themeReg, typoReg, pluginReg] = await Promise.all([
      fetchThemeRegistry().catch(() => ({ themes: [] })),
      fetchTypographyRegistry().catch(() => ({ typographies: [] })),
      fetchPluginRegistry().catch(() => ({ plugins: [] })),
    ]);

    await hydrateOnboardingPluginPermissionDefaults(
      bundle.plugins,
      pluginReg.plugins,
      persistRequestedPluginPermissionDefaults,
    );

    const plan = planBundleApply(bundle, {
      themes: themeReg.themes,
      typographies: typoReg.typographies,
      plugins: pluginReg.plugins,
    });

    const runtime = createDefaultBundleApplyRuntime();
    const result = await executeBundleApply(plan, runtime);

    if (result.summary.failed > 0) {
      console.warn(
        `[App] Onboarding bundle apply: ${result.summary.success}/${result.summary.total} succeeded`,
        result.results.filter((r) => r.status === "failed"),
      );
    }
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
        collaborationStore.setCollaborationPath(toCollaborationPath(effectivePath));
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

  function openDeleteConfirm(paths: string[]) {
    const uniquePaths = Array.from(new Set(paths));
    pendingDeletePaths = uniquePaths;
    pendingDeleteIncludesDescendants = selectionIncludesDescendants(
      tree,
      uniquePaths,
    );
    showDeleteConfirm = true;
  }

  async function loadDeleteSubtree(path: string, visited: Set<string>) {
    if (visited.has(path)) return;
    visited.add(path);

    const existingNode = findTreeNode(tree, path);
    if (!existingNode) return;

    if (hasUnloadedSidebarChildren(existingNode)) {
      await loadNodeChildren(path);
    }

    const refreshedNode = findTreeNode(tree, path);
    if (!refreshedNode) return;

    for (const child of getRenderableSidebarChildren(refreshedNode)) {
      if (child.children.length > 0 || hasUnloadedSidebarChildren(child)) {
        await loadDeleteSubtree(child.path, visited);
      }
    }
  }

  async function buildDeletePlan(paths: string[]): Promise<string[]> {
    const deleteRoots = pruneNestedDeleteRoots(tree, paths);
    const visited = new Set<string>();
    for (const rootPath of deleteRoots) {
      await loadDeleteSubtree(rootPath, visited);
    }

    return orderDeletePaths(tree, expandDeleteSelection(tree, deleteRoots));
  }

  // Delete an entry - shows confirmation dialog, then delegates to controller
  async function handleDeleteEntry(path: string) {
    if (!api) return;
    openDeleteConfirm([path]);
  }

  async function handleDeleteEntries(paths: string[]) {
    if (!api || paths.length === 0) return;
    openDeleteConfirm(paths);
  }

  // Called when user confirms deletion in the dialog
  async function confirmDeleteEntry() {
    if (!api || pendingDeletePaths.length === 0 || isDeletingEntries) return;

    isDeletingEntries = true;
    const requestedPaths = [...pendingDeletePaths];
    const deleteRoots = pruneNestedDeleteRoots(tree, requestedPaths);
    const parentPaths = new Set(
      deleteRoots
        .map((path) => workspaceStore.getParentNodePath(path))
        .filter((path): path is string => Boolean(path)),
    );

    try {
      const deletePlan = await buildDeletePlan(deleteRoots);
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
        // No audiences exist — go straight to the full-screen manager
        showAudienceManager = true;
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
    const audience = templateContextStore.previewAudience ?? undefined;
    await refreshTreeController(api, backend, showUnlinkedFiles, showHiddenFiles, audience);
    await reloadPluginPermissionsConfig();
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
    const audience = templateContextStore.previewAudience ?? undefined;
    await loadNodeChildrenController(api, nodePath, showUnlinkedFiles, showHiddenFiles, audience);
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
    const displayName = linkMatch ? (linkMatch[1] || attachmentPath.split("/").pop() || attachmentPath) : attachmentPath.split("/").pop() || attachmentValue;

    try {
      const data = await api.getAttachmentData(currentEntry.path, attachmentPath);
      const mimeType = getMimeType(attachmentPath);
      let blob = new Blob([new Uint8Array(data)], { type: mimeType });
      let mediaKind = getAttachmentMediaKind(attachmentPath, mimeType);
      if (isHeicFile(attachmentPath)) {
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
              collaborationStore.setCollaborationPath(toCollaborationPath(newPath));
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

          titleError = null;
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
            titleError = `A file with that name already exists. Choose a different title.`;
          } else {
            titleError = `Could not rename: ${errorMsg}`;
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
    swipeProgress={commandPaletteSwipeProgress}
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
  onAddWorkspace={async () => {
    showSettingsDialog = false;
    await tick();
    showAddWorkspace = true;
  }}
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

<!-- Add Workspace Dialog -->
<AddWorkspaceDialog
  bind:open={showAddWorkspace}
  onOpenChange={(open) => { if (!open) addWorkspaceBundle = null; showAddWorkspace = open; }}
  selectedBundle={addWorkspaceBundle}
  onComplete={async (appliedBundle) => {
    showAddWorkspace = false;
    if (showWelcomeScreen) {
      // Came from the welcome screen — dismiss it and re-initialize
      await handleWelcomeComplete("", "");
    } else {
      // Re-initialize backend references and refresh tree for the new workspace.
      await handleWorkspaceSwitchComplete();
    }
    // Apply bundle (theme, plugins, typography) after workspace is fully initialized
    if (appliedBundle && appliedBundle.plugins.length > 0) {
      try {
        await applyOnboardingBundle(appliedBundle);
      } catch (e) {
        console.warn("[App] Bundle apply after workspace creation failed (non-fatal):", e);
      }
    }
    addWorkspaceBundle = null;
  }}
/>

<!-- Device Replacement Dialog (shown when sign-in hits device limit) -->
<DeviceReplacementDialog onAuthenticated={() => handleWelcomeComplete("", "")} />

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
          onOpenManager={() => { showAudienceDialog = false; audienceDialogPath = null; showAudienceManager = true; }}
        />
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<!-- Full-screen Audience Manager -->
{#if showAudienceManager && api}
  <AudienceManager {api} rootPath={tree?.path ?? ""} onClose={() => { showAudienceManager = false; }} />
{/if}

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
    onGetStarted={async (selectedBundle) => {
      entryStore.setLoading(true);
      try {
        await autoCreateDefaultWorkspace(selectedBundle);
        showWelcomeScreen = false;
        await refreshTree();
        if (tree) {
          workspaceStore.expandNode(tree.path);
          await openEntry(tree.path);
        }
        await runValidation();

        // Trigger spotlight onboarding tour if the bundle defines one
        if (selectedBundle?.spotlight?.length) {
          await tick();
          requestAnimationFrame(() => {
            spotlightSteps = selectedBundle.spotlight;
          });
        }
      } catch (e) {
        console.error("[App] Auto-create from welcome screen failed, opening dialog:", e);
        showAddWorkspace = true;
      } finally {
        entryStore.setLoading(false);
      }
    }}
    onSignIn={() => {
      showAddWorkspace = true;
    }}
    returnWorkspaceName={welcomeReturnWorkspaceName}
    onReturn={() => {
      showWelcomeScreen = false;
      welcomeReturnWorkspaceName = null;
    }}
    onCreateNewWithBundle={(selectedBundle) => {
      addWorkspaceBundle = selectedBundle;
      showAddWorkspace = true;
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
    swipeProgress={leftSidebarSwipeProgress}
    onOpenEntry={openEntry}
    onToggleNode={toggleNode}
    onToggleCollapse={toggleLeftSidebar}
    onOpenSettings={() => { settingsInitialTab = undefined; showSettingsDialog = true; }}
    onOpenMarketplace={() => { showMarketplaceDialog = true; }}
    settingsDialogOpen={showSettingsDialog}
    marketplaceDialogOpen={showMarketplaceDialog}
    onOpenAccountSettings={() => { settingsInitialTab = "account"; showSettingsDialog = true; }}
    onAddWorkspace={() => { showAddWorkspace = true; }}
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
    onOpenAudienceManager={() => { showAudienceManager = true; }}
    requestedTab={requestedLeftTab}
    onRequestedTabConsumed={() => (requestedLeftTab = null)}
    onPluginHostAction={handlePluginHostAction}
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
  <main class="flex-1 flex flex-col overflow-hidden min-w-0 relative md:pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height))]" data-spotlight="editor-area">
    <!-- Sidebar open buttons (visible when collapsed, fade in focus mode, reveal on hover via edge strip) -->
    {#if leftSidebarCollapsed}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="absolute top-0 left-0 z-20 w-8 h-full hidden md:flex items-start group"
        onmouseenter={() => leftEdgeHovered = true}
        onmouseleave={() => leftEdgeHovered = false}
      >
        <button
          type="button"
          class="mt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.5rem)] ml-2 p-2 transition-opacity duration-200
            {focusMode && leftSidebarCollapsed && rightSidebarCollapsed && !leftEdgeHovered ? 'opacity-0' : 'opacity-100'}"
          onclick={toggleLeftSidebar}
          aria-label="Open navigation sidebar"
        >
          <PanelLeft class="size-4 text-muted-foreground" />
        </button>
      </div>
    {/if}
    {#if rightSidebarCollapsed}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="absolute top-0 right-0 z-20 w-8 h-full hidden md:flex items-start justify-end group"
        onmouseenter={() => rightEdgeHovered = true}
        onmouseleave={() => rightEdgeHovered = false}
      >
        <button
          type="button"
          class="mt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.5rem)] mr-2 p-2 transition-opacity duration-200
            {focusMode && leftSidebarCollapsed && rightSidebarCollapsed && !rightEdgeHovered ? 'opacity-0' : 'opacity-100'}"
          onclick={toggleRightSidebar}
          aria-label="Open properties panel"
        >
          <PanelRight class="size-4 text-muted-foreground" />
        </button>
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
              onclick={() => { showAudienceManager = true; }}
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
        {isDirty}
        {isSaving}
        leftSidebarOpen={!leftSidebarCollapsed}
        rightSidebarOpen={!rightSidebarCollapsed}
        {focusMode}
        mobileFocusChromeVisible={mobileFocusChromeRevealed}
        readonly={editorReadonly}
        commandPaletteOpen={uiStore.showCommandPalette}
        onSave={save}
        onOpenCommandPalette={uiStore.openCommandPalette}
        onRevealMobileFocusChrome={revealMobileFocusChromeTemporarily}
        {api}
        onPluginToolbarAction={handlePluginToolbarAction}
        audienceTags={effectiveAudienceTags}
        onOpenAudienceManager={() => { showAudienceManager = true; }}
        onFabMount={(el) => { editorFabElement = el; }}
        {commandRegistry}
        hasEditor={!!editorRef}
      />
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
        {isLoading}
        {workspaceMissing}
        onToggleLeftSidebar={toggleLeftSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        hasWorkspaceTree={!!tree && tree.path !== '.'}
        onInitializeWorkspace={handleInitializeEmptyWorkspace}
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
    swipeProgress={rightSidebarSwipeProgress}
    onToggleCollapse={toggleRightSidebar}
    onPropertyChange={handlePropertyChange}
    onPropertyRemove={handlePropertyRemove}
    onPropertyAdd={handlePropertyAdd}
    {titleError}
    onTitleErrorClear={() => (titleError = null)}
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
    onOpenAudienceManager={() => { showAudienceManager = true; }}
    onPropertyReorder={handlePropertyReorder}
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
