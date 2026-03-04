<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, isTauri, resetBackend } from "./lib/backend";
  import { FsaGestureRequiredError } from "./lib/backend/fsaErrors";
  import * as browserPlugins from "$lib/plugins/browserPluginManager.svelte";
  import { addFilesToZip } from "./lib/settings/zipUtils";
  import { createApi, type Api } from "./lib/backend/api";
  import type { JsonValue } from "./lib/backend/generated/serde_json/JsonValue";
  import { isIOS } from "$lib/hooks/useMobile.svelte";
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import AddWorkspaceDialog from "./lib/AddWorkspaceDialog.svelte";
  import ImagePreviewDialog from "./lib/ImagePreviewDialog.svelte";
  import AudienceEditor from "./lib/components/AudienceEditor.svelte";
  import DocumentAudiencePill from "./lib/components/DocumentAudiencePill.svelte";
  import MarkdownPreviewDialog from "./lib/MarkdownPreviewDialog.svelte";
  import EditorHeader from "./views/editor/EditorHeader.svelte";
  import EditorEmptyState from "./views/editor/EditorEmptyState.svelte";
  import WelcomeScreen from "./views/WelcomeScreen.svelte";
  import PluginMarketplace from "./views/marketplace/PluginMarketplace.svelte";
  import EditorContent from "./views/editor/EditorContent.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "svelte-sonner";
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
  import type { PluginConfig } from "./models/stores/permissionStore.svelte";
  import { getTemplateContextStore } from "./lib/stores/templateContextStore.svelte";
  import { getAppearanceStore } from "./lib/stores/appearance.svelte";


  // Import auth
  import { initAuth, getCurrentWorkspace, verifyMagicLink, setServerUrl, refreshUserInfo, getAuthState, isAuthenticated, getWorkspaces, isSyncEnabled } from "./lib/auth";
  import { getLocalWorkspace, getLocalWorkspaces, getCurrentWorkspaceId, getWorkspaceStorageType, discoverOpfsWorkspaces, createLocalWorkspace, setCurrentWorkspaceId } from "$lib/storage/localWorkspaceRegistry.svelte";

  // Initialize theme store immediately
  getThemeStore();

  // Initialize template context store (feeds live values to editor template variables)
  const templateContextStore = getTemplateContextStore();

  // Initialize appearance store (theme presets, typography, layout)
  getAppearanceStore();

  // Import services
  import {
    revokeBlobUrls,
  } from "./models/services";
  import { getMimeType, isHeicFile, convertHeicToJpeg } from "./models/services/attachmentService";

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
    handleFindInFile,
    handleWordCount,
    handleCopyAsMarkdown,
    handleViewMarkdown,
    handleReorderFootnotes,
  } from "./controllers";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);
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
  let showSettingsDialog = $derived(uiStore.showSettingsDialog);
  let showExportDialog = $derived(uiStore.showExportDialog);
  let showNewEntryModal = $derived(uiStore.showNewEntryModal);
  let exportPath = $derived(uiStore.exportPath);
  let editorRef = $derived(uiStore.editorRef);

  // Right sidebar tab control (built-in tabs or plugin tab IDs)
  let requestedSidebarTab: string | null = $state(null);

  // Left sidebar tab control (plugin-owned tabs)
  let requestedLeftTab: string | null = $state(null);

  // Add workspace dialog
  let showAddWorkspace = $state(false);

  // Delete confirmation dialog state
  let showDeleteConfirm = $state(false);
  let pendingDeletePath = $state<string | null>(null);

  // Audience dialog state
  let showAudienceDialog = $state(false);
  let audienceDialogPath = $state<string | null>(null);
  let audienceDialogAudience = $state<string[] | null>(null);
  let pendingDeleteName = $derived(
    pendingDeletePath?.split('/').pop()?.replace('.md', '') ?? ''
  );

  // Welcome screen (shown when no workspaces exist)
  let showWelcomeScreen = $state(false);

  // Dedicated plugin marketplace surface.
  let showMarketplace = $state(false);

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
  let showEditorTitle = $derived(workspaceStore.showEditorTitle);
  let showEditorPath = $derived(workspaceStore.showEditorPath);
  let focusMode = $derived(workspaceStore.focusMode);

  // API wrapper - uses execute() internally for all operations
  let api: Api | null = $derived(backend ? createApi(backend) : null);

  // Root frontmatter plugin permissions cache (used by runtime permission checks).
  let pluginPermissionsConfig = $state<Record<string, PluginConfig> | undefined>(
    undefined,
  );
  let pluginPermissionsRootPath = $state<string | null>(null);

  // Reserved for plugin-provided history panels that may need host context.
  let rustApi: any | null = $state(null);


  // Collaboration state - proxied from collaborationStore
  let collaborationEnabled = $derived(collaborationStore.collaborationEnabled);

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

  // Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri)
  function normalizeFrontmatter(frontmatter: any): Record<string, any> {
    if (!frontmatter) return {};
    if (frontmatter instanceof Map) {
      return Object.fromEntries(frontmatter.entries());
    }
    return frontmatter;
  }

  async function reloadPluginPermissionsConfig(): Promise<void> {
    if (!api || !tree?.path) {
      pluginPermissionsConfig = undefined;
      pluginPermissionsRootPath = null;
      return;
    }

    if (pluginPermissionsRootPath !== tree.path) {
      permissionStore.clearSessionCache();
      pluginPermissionsRootPath = tree.path;
    }

    try {
      const fm = await api.getFrontmatter(tree.path);
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
    if (!api || !tree?.path) {
      throw new Error("Workspace root is not available");
    }
    await api.setFrontmatterProperty(
      tree.path,
      "plugins",
      nextConfig as unknown as JsonValue,
      tree.path,
    );
    pluginPermissionsConfig = nextConfig;
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

  // Attachment state
  let attachmentError: string | null = $state(null);
  let attachmentFileInput: HTMLInputElement | null = $state(null);

  // Image preview state
  let imagePreviewOpen = $state(false);
  let previewImageUrl: string | null = $state(null);
  let previewImageName = $state("");

  // Markdown preview state
  let markdownPreviewOpen = $state(false);
  let markdownPreviewBody = $state("");
  let markdownPreviewFrontmatter: Record<string, unknown> = $state({});

  // Note: Blob URL management is now in attachmentService.ts

  // Persist display setting to localStorage when changed
  $effect(() => {
    if (typeof window !== "undefined") {
      localStorage.setItem(
        "diaryx-show-unlinked-files",
        String(showUnlinkedFiles),
      );
      localStorage.setItem("diaryx-show-hidden-files", String(showHiddenFiles));
    }
  });

  // Sync current entry's frontmatter to the template context store
  // so template variable NodeViews and conditional block decorations update live.
  // Only active when the templating plugin is loaded.
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

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    // Refresh tree when zip import completes (from ImportSettings)
    window.addEventListener("import:complete", handleImportComplete);

    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      uiStore.setLeftSidebarCollapsed(false);
      uiStore.setRightSidebarCollapsed(false);
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
          setServerUrl("https://sync.diaryx.org");
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

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Discover any OPFS workspace directories not yet in the local registry.
      // This catches workspaces created by other tabs or previous sessions.
      await discoverOpfsWorkspaces();

      // Check if any workspaces exist before proceeding
      const defaultWorkspace = getCurrentWorkspace();
      const localWsList = getLocalWorkspaces();
      const currentWsId = getCurrentWorkspaceId();

      if (!defaultWorkspace && (localWsList.length === 0 || !currentWsId)) {
        const dataJustCleared = sessionStorage.getItem('diaryx_data_cleared');
        if (dataJustCleared) {
          sessionStorage.removeItem('diaryx_data_cleared');
        }

        // No workspaces exist — show welcome/onboarding screen
        showWelcomeScreen = true;
        entryStore.setLoading(false);
        return;
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

      // Load browser-side Extism WASM plugins from IndexedDB
      Promise.resolve().then(async () => {
        const pluginSupport = browserPlugins.getBrowserPluginSupport();
        if (!pluginSupport.supported) {
          console.info('[App] Browser plugins disabled:', pluginSupport.reason);
          return;
        }

        browserPlugins.setPluginPermissionConfigProvider(() => pluginPermissionsConfig);
        await browserPlugins.loadAllPlugins().catch((e: unknown) =>
          console.warn('[App] Failed to load browser plugins:', e),
        );
        // Eagerly load icons for plugin insert commands so they're cached before menus open.
        getPluginStore().preloadInsertCommandIcons();
      });


      // Expand root and open it by default
      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }

      // Run initial validation
      await runValidation();

      // Auto-open the sync wizard for clients where sync hasn't been configured yet
      // but the server already has workspaces. This covers:
      //   - New devices where the user clicked a magic link (token was in the URL)
      //   - Returning authenticated sessions where sync setup was never completed
      // The wizard auto-detects Scenario C (server has workspaces) and runs without
      // user interaction, downloading the server workspace and enabling sync.
      if (isAuthenticated() && getWorkspaces().length > 0 && !isSyncEnabled()) {
        showAddWorkspace = true;
      }

      // Add swipe gestures for mobile:
      // - Swipe down from top: open command palette
      // - Swipe right anywhere: open left sidebar (or close right sidebar if open)
      // - Swipe left anywhere: open right sidebar (or close left sidebar if open)
      let touchStartY = 0;
      let touchStartX = 0;
      const SWIPE_THRESHOLD = 80; // minimum swipe distance
      const CROSS_AXIS_MAX = 50; // max movement in perpendicular direction

      const handleTouchStart = (e: TouchEvent) => {
        touchStartY = e.touches[0].clientY;
        touchStartX = e.touches[0].clientX;
      };
      const handleTouchEnd = (e: TouchEvent) => {
        const touchEndY = e.changedTouches[0].clientY;
        const touchEndX = e.changedTouches[0].clientX;
        const deltaY = touchEndY - touchStartY;
        const deltaX = touchEndX - touchStartX;
        const absDeltaY = Math.abs(deltaY);
        const absDeltaX = Math.abs(deltaX);

        // Swipe down from top 100px of screen, mostly vertical → open command palette
        if (touchStartY < 100 && deltaY > SWIPE_THRESHOLD && absDeltaX < CROSS_AXIS_MAX) {
          uiStore.openCommandPalette();
          return;
        }

        // Swipe right anywhere, mostly horizontal:
        // close right sidebar first, otherwise open left sidebar.
        if (deltaX > SWIPE_THRESHOLD && absDeltaY < CROSS_AXIS_MAX) {
          if (!rightSidebarCollapsed) {
            toggleRightSidebar();
          } else if (leftSidebarCollapsed) {
            toggleLeftSidebar();
          }
          return;
        }

        // Swipe left anywhere, mostly horizontal:
        // close left sidebar first, otherwise open right sidebar.
        if (deltaX < -SWIPE_THRESHOLD && absDeltaY < CROSS_AXIS_MAX) {
          if (!leftSidebarCollapsed) {
            toggleLeftSidebar();
          } else if (rightSidebarCollapsed) {
            toggleRightSidebar();
          }
          return;
        }
      };
      document.addEventListener("touchstart", handleTouchStart);
      document.addEventListener("touchend", handleTouchEnd);

    } catch (e) {
      if (e instanceof FsaGestureRequiredError) {
        console.warn("[App] FSA needs user gesture to reconnect:", e);
        fsaNeedsReconnect = true;
        return;
      }
      console.error("[App] Initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  });

  onDestroy(() => {
    // Cleanup blob URLs
    revokeBlobUrls();
    // Cleanup filesystem event subscription
    cleanupEventSubscription?.();
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
  function handleContentChange(markdown: string) {
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
  }

  function toggleRightSidebar() {
    uiStore.toggleRightSidebar();
  }

  function openMarketplace() {
    showSettingsDialog = false;
    showMarketplace = true;
  }

  function closeMarketplace() {
    showMarketplace = false;
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
  async function handlePluginHostAction(action: { type: string; payload?: unknown }) {
    const actionType = action?.type;
    const payload = (action?.payload ?? {}) as Record<string, unknown>;

    switch (actionType) {
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
        openMarketplace();
        return { opened: "marketplace" };
      case "toggle-left-sidebar":
        toggleLeftSidebar();
        return { toggled: "left" };
      case "toggle-right-sidebar":
        toggleRightSidebar();
        return { toggled: "right" };
      case "open-oauth": {
        const oauthUrl = typeof payload.url === "string" ? payload.url : null;
        if (!oauthUrl) throw new Error("open-oauth requires payload.url");

        if (isTauri()) {
          const { invoke } = await import("@tauri-apps/api/core");
          const redirectPrefix = typeof payload.redirect_uri_prefix === "string"
            ? payload.redirect_uri_prefix : "http://localhost/oauth/callback";
          const result = await invoke<{ code: string }>("oauth_webview", {
            url: oauthUrl, redirectPrefix,
          });
          return { code: result.code, redirect_uri: redirectPrefix };
        }

        // Web: popup + postMessage
        const popup = window.open(oauthUrl, "oauth-popup", "width=500,height=600");
        if (!popup) throw new Error("Popup blocked — please allow popups");
        return new Promise((resolve, reject) => {
          const timeout = setTimeout(() => {
            window.removeEventListener("message", handler);
            reject(new Error("OAuth timed out"));
          }, 120000);
          const handler = (event: MessageEvent) => {
            if (event.origin !== window.location.origin) return;
            if (event.data?.type !== "oauth-callback") return;
            clearTimeout(timeout);
            window.removeEventListener("message", handler);
            if (event.data.error) reject(new Error(event.data.error));
            else resolve({ code: event.data.code, redirect_uri: `${window.location.origin}/oauth/callback` });
          };
          window.addEventListener("message", handler);
        });
      }
      default:
        throw new Error(`Unknown host action: ${actionType ?? "undefined"}`);
    }
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
   * Returns { id, name } on success, or throws on failure.
   */
  async function autoCreateDefaultWorkspace(): Promise<{ id: string; name: string }> {
    const ws = createLocalWorkspace("My Workspace");
    setCurrentWorkspaceId(ws.id);

    const backendInstance = await getBackend(ws.id, ws.name, ws.storageType);
    workspaceStore.setBackend(backendInstance);

    const apiInstance = createApi(backendInstance);
    rustApi = null;

    cleanupEventSubscription = initEventSubscription(backendInstance);

    // Create workspace root structure and starter entries.
    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
    await seedStarterWorkspaceContent(apiInstance, workspaceDir, ws.name);

    return { id: ws.id, name: ws.name };
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
      // Re-fetch entry to get updated frontmatter
      try {
        const updatedEntry = await api.getEntry(effectivePath);
        entryStore.setCurrentEntry(updatedEntry);
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

      // Sync editor H1 if this is the current entry
      if (editorRef) {
        const editor = editorRef.getEditor();
        if (editor) {
          const { doc } = editor.state;
          let firstHeadingPos: number | null = null;
          let firstHeadingSize = 0;
          doc.descendants((node: any, pos: number) => {
            if (firstHeadingPos === null && node.type.name === 'heading' && node.attrs.level === 1) {
              firstHeadingPos = pos;
              firstHeadingSize = node.content.size;
              return false;
            }
          });
          if (firstHeadingPos !== null) {
            editor.chain()
              .setTextSelection({ from: firstHeadingPos + 1, to: firstHeadingPos + 1 + firstHeadingSize })
              .insertContent(newTitle)
              .run();
          } else {
            editor.chain()
              .insertContentAt(0, { type: 'heading', attrs: { level: 1 }, content: [{ type: 'text', text: newTitle }] })
              .run();
          }
        }
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

  // Delete an entry - shows confirmation dialog, then delegates to controller
  async function handleDeleteEntry(path: string) {
    if (!api) return;
    pendingDeletePath = path;
    showDeleteConfirm = true;
  }

  // Called when user confirms deletion in the dialog
  async function confirmDeleteEntry() {
    const path = pendingDeletePath;
    showDeleteConfirm = false;
    pendingDeletePath = null;
    if (!api || !path) return;
    const parentPath = workspaceStore.getParentNodePath(path);
    await deleteEntryWithSync(api, path, currentEntry?.path ?? null, async () => {
      await refreshTree();
      if (parentPath) {
        await loadNodeChildren(parentPath);
      }
      await runValidation();
    });
  }

  // Called when user cancels deletion
  function cancelDeleteEntry() {
    showDeleteConfirm = false;
    pendingDeletePath = null;
  }

  // Open audience dialog for a tree entry
  async function handleSetAudience(path: string) {
    if (!api) return;
    try {
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

  async function cmdRenameEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    const currentTitle = (typeof currentEntry.frontmatter?.title === "string" ? currentEntry.frontmatter.title : null)
      || currentEntry.path.split("/").pop()?.replace(".md", "") || "";
    const newTitle = window.prompt("Enter new title:", currentTitle);
    if (!newTitle || newTitle === currentTitle) return;
    await handleRenameEntry(currentEntry.path, newTitle);
  }

  function cmdDeleteEntry() {
    if (!currentEntry) { toast.error("No entry selected"); return; }
    handleDeleteEntry(currentEntry.path);
  }

  async function cmdMoveEntry() {
    if (!currentEntry || !tree) { toast.error("No entry selected"); return; }
    const allParents: string[] = [];
    function collectParents(node: typeof tree) {
      if (!node) return;
      if (node.children.length > 0 || node.path.endsWith("index.md") || node.path.endsWith("README.md")) {
        allParents.push(node.path);
      }
      node.children.forEach(collectParents);
    }
    collectParents(tree);
    const options = allParents.filter(p => p !== currentEntry!.path).map(p => p.split("/").pop()?.replace(".md", "") || p).join(", ");
    const dest = window.prompt(`Move "${currentEntry.path.split("/").pop()?.replace(".md", "")}" to which parent?\n\nAvailable: ${options}`);
    if (!dest) return;
    const match = allParents.find(p => p.split("/").pop()?.replace(".md", "").toLowerCase() === dest.toLowerCase());
    if (!match) { toast.error("Parent not found"); return; }
    await handleMoveEntry(currentEntry.path, match);
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
  ): Promise<{ blobUrl: string; attachmentPath: string } | null> {
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
      if (isHeicFile(attachmentPath)) {
        blob = await convertHeicToJpeg(blob);
      }
      // Revoke previous preview URL if any
      if (previewImageUrl) URL.revokeObjectURL(previewImageUrl);
      previewImageUrl = URL.createObjectURL(blob);
      previewImageName = displayName;
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
    }
  }

  function handleAttachmentInsert(selection: {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    sourceEntryPath: string;
  }) {
    attachmentInsertHandler(selection, editorRef, currentEntry);
  }

  // Handle drag-drop: attach entry to new parent
  async function handleMoveEntry(entryPath: string, newParentPath: string) {
    if (!api) return;
    if (entryPath === newParentPath) return;

    console.log(
      `[Drag-Drop] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`,
    );

    try {
      await api.attachEntryToParent(entryPath, newParentPath);
      await refreshTree();
      await runValidation();
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

          if (newPath) {
            // Rename happened — update UI state to new path
            if (expandedNodes.has(path)) {
              workspaceStore.collapseNode(path);
              workspaceStore.expandNode(newPath);
            }

            const updatedEntry = {
              ...currentEntry,
              path: newPath,
              frontmatter: { ...normalizedFrontmatter, [key]: value },
            };
            entryStore.setCurrentEntry(updatedEntry);
            if (collaborationEnabled) {
              collaborationStore.setCollaborationPath(toCollaborationPath(newPath));
            }
          } else {
            // No rename — just update frontmatter in store
            const updatedEntry = {
              ...currentEntry,
              frontmatter: { ...normalizedFrontmatter, [key]: value },
            };
            entryStore.setCurrentEntry(updatedEntry);
          }

          // Sync editor H1 to match the new title (backend already wrote to disk,
          // but we must update the in-memory editor to prevent the next save from reverting)
          if (editorRef) {
            const editor = editorRef.getEditor();
            if (editor) {
              const { doc } = editor.state;
              let firstHeadingPos: number | null = null;
              let firstHeadingSize = 0;
              doc.descendants((node: any, pos: number) => {
                if (firstHeadingPos === null && node.type.name === 'heading' && node.attrs.level === 1) {
                  firstHeadingPos = pos;
                  firstHeadingSize = node.content.size;
                  return false;
                }
              });
              if (firstHeadingPos !== null) {
                // Replace existing H1 text
                editor.chain()
                  .setTextSelection({ from: firstHeadingPos + 1, to: firstHeadingPos + 1 + firstHeadingSize })
                  .insertContent(value)
                  .run();
              } else {
                // Prepend H1 at start of document
                editor.chain()
                  .insertContentAt(0, { type: 'heading', attrs: { level: 1 }, content: [{ type: 'text', text: value }] })
                  .run();
              }
            }
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

        if (key === 'contents' || key === 'part_of') {
          await refreshTree();
        }
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
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  function getEntryTitle(entry: { path: string; title?: string | null; frontmatter?: Record<string, unknown> }): string {
    // Prioritize frontmatter.title for live updates, fall back to cached title
    const fm = normalizeFrontmatter(entry.frontmatter);
    const frontmatterTitle = fm?.title as string | undefined;
    return (
      frontmatterTitle ??
      entry.title ??
      entry.path.split("/").pop()?.replace(".md", "") ??
      "Untitled"
    );
  }

  // Handle link clicks in the editor - delegates to controller
  async function handleLinkClick(href: string) {
    if (!api) return;
    await linkClickHandler(href, api, currentEntry, tree, openEntry, refreshTree);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

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
  {api}
  hasEntry={!!currentEntry}
  hasEditor={!!editorRef}
  onImportFromClipboard={handleImportFromClipboard}
  onImportMarkdownFile={handleImportMarkdownFile}
  onOpenBackupImport={handleQuickBackupExport}
  onDuplicateEntry={cmdDuplicateEntry}
  onRenameEntry={cmdRenameEntry}
  onDeleteEntry={cmdDeleteEntry}
  onMoveEntry={cmdMoveEntry}
  onCreateChildEntry={cmdCreateChildEntry}
  onRefreshTree={refreshTree}
  onValidateWorkspace={cmdValidateWorkspace}
  onFindInFile={handleFindInFile}
  onWordCount={cmdWordCount}
  onCopyAsMarkdown={cmdCopyAsMarkdown}
  onViewMarkdown={cmdViewMarkdown}
  onReorderFootnotes={cmdReorderFootnotes}
/>

<SettingsDialog
  bind:open={showSettingsDialog}
  bind:focusMode
  workspacePath={tree?.path}
  initialTab={settingsInitialTab}
  {api}
  onOpenMarketplace={openMarketplace}
  onAddWorkspace={async () => {
    showSettingsDialog = false;
    await tick();
    showAddWorkspace = true;
  }}
  onHostAction={handlePluginHostAction}
/>

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
  onOpenChange={(open) => showAddWorkspace = open}
  onComplete={async () => {
    showAddWorkspace = false;
    if (showWelcomeScreen) {
      // Came from the welcome screen — dismiss it and re-initialize
      await handleWelcomeComplete("", "");
    } else {
      // Re-initialize backend references and refresh tree for the new workspace.
      await handleWorkspaceSwitchComplete();
    }
  }}
/>

<!-- Delete Confirmation Dialog -->
<Dialog.Root bind:open={showDeleteConfirm} onOpenChange={(open) => { if (!open) cancelDeleteEntry(); }}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Delete entry</Dialog.Title>
      <Dialog.Description>
        Are you sure you want to delete "{pendingDeleteName}"? This action cannot be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={cancelDeleteEntry}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDeleteEntry}>Delete</Button>
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
        />
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<!-- Toast Notifications -->
<Toaster />

<!-- Tooltip Provider for keyboard shortcut hints -->
<Tooltip.Provider>

{#if showMarketplace}
  <PluginMarketplace onClose={closeMarketplace} />
{:else if fsaNeedsReconnect}
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
    onGetStarted={async () => {
      entryStore.setLoading(true);
      try {
        await autoCreateDefaultWorkspace();
        showWelcomeScreen = false;
        await refreshTree();
        if (tree) {
          workspaceStore.expandNode(tree.path);
          await openEntry(tree.path);
        }
        await runValidation();
      } catch (e) {
        console.error("[App] Auto-create from welcome screen failed, opening dialog:", e);
        showAddWorkspace = true;
      } finally {
        entryStore.setLoading(false);
      }
    }}
  />
{:else}
<div class="flex h-full bg-background overflow-hidden">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {activeEntryPath}
    {isLoading}
    {expandedNodes}
    {validationResult}
    {showUnlinkedFiles}
    {api}
    collapsed={leftSidebarCollapsed}
    onOpenEntry={openEntry}
    onToggleNode={toggleNode}
    onToggleCollapse={toggleLeftSidebar}
    onOpenSettings={() => { settingsInitialTab = undefined; showSettingsDialog = true; }}
    onOpenAccountSettings={() => { settingsInitialTab = "account"; showSettingsDialog = true; }}
    onOpenMarketplace={openMarketplace}
    onAddWorkspace={() => { showAddWorkspace = true; }}
    onMoveEntry={handleMoveEntry}
    onCreateChildEntry={handleCreateChildEntry}
    onDeleteEntry={handleDeleteEntry}
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
    onInitializeWorkspace={handleInitializeWorkspace}
    onSetAudience={handleSetAudience}
    requestedTab={requestedLeftTab}
    onRequestedTabConsumed={() => (requestedLeftTab = null)}
    onPluginHostAction={handlePluginHostAction}
  />

  <!-- Hidden file input for attachments (accepts all file types) -->
  <input
    type="file"
    bind:this={attachmentFileInput}
    onchange={handleAttachmentFileSelect}
    class="hidden"
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0 relative pt-[env(safe-area-inset-top)]">
    {#if currentEntry}
      <EditorHeader
        title={loadingTargetPath
          ? loadingTargetPath.split("/").pop()?.replace(".md", "") ?? "Loading..."
          : getEntryTitle(currentEntry)}
        path={loadingTargetPath ?? currentEntry.path}
        {isDirty}
        {isSaving}
        showTitle={showEditorTitle}
        showPath={showEditorPath}
        leftSidebarOpen={!leftSidebarCollapsed}
        rightSidebarOpen={!rightSidebarCollapsed}
        {focusMode}
        readonly={editorReadonly}
        onSave={save}
        onToggleLeftSidebar={toggleLeftSidebar}
        onToggleRightSidebar={toggleRightSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        {api}
        onPluginToolbarAction={handlePluginToolbarAction}
      />

      <!-- Document-level audience pill: sits directly under the title bar -->
      <div class="shrink-0 px-4 md:px-6 py-1 border-b border-border/50">
        <div class="mx-auto" style:max-width="var(--editor-content-max-width)">
          <DocumentAudiencePill
            audience={currentEntry.frontmatter.audience as string[] | null ?? null}
            entryPath={currentEntry.path}
            rootPath={tree?.path ?? ""}
            {api}
            onChange={(value) => {
              if (value === null) {
                handlePropertyRemove("audience");
              } else {
                handlePropertyChange("audience", value);
              }
            }}
          />
        </div>
      </div>

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

      />
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
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
        {isLoading}
        onToggleLeftSidebar={toggleLeftSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        hasWorkspaceTree={!!tree && tree.path !== '.'}
        onInitializeWorkspace={handleInitializeEmptyWorkspace}
      />
    {/if}
  </main>

  <!-- Right Sidebar (Properties & History) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
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
  />
</div>
{/if}

</Tooltip.Provider>

<!-- Image Preview Dialog -->
<ImagePreviewDialog
  open={imagePreviewOpen}
  imageUrl={previewImageUrl}
  imageName={previewImageName}
  onOpenChange={handleImagePreviewClose}
/>

<MarkdownPreviewDialog
  open={markdownPreviewOpen}
  body={markdownPreviewBody}
  frontmatter={markdownPreviewFrontmatter}
  onOpenChange={(open) => (markdownPreviewOpen = open)}
/>
