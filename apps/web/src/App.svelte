<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, isTauri } from "./lib/backend";
  import { createApi, type Api } from "./lib/backend/api";
  import type { JsonValue } from "./lib/backend/generated/serde_json/JsonValue";
  // New Rust CRDT module imports
  import { RustCrdtApi } from "./lib/crdt/rustCrdtApi";
  import {
    disconnectWorkspace,
    setWorkspaceId,
    setBackendApi,
    setBackend,
    onSessionSync,
    onBodyChange,
    onFileChange,
    onFileRenamed,
    onSyncProgress,
    onSyncStatus,
    getTreeFromCrdt,
    initEventSubscription,
    waitForInitialSync,
    getCanonicalPathForSync,
  } from "./lib/crdt/workspaceCrdtBridge";
  // Note: YDoc and HocuspocusProvider types are now handled by collaborationStore
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import AddWorkspaceDialog from "./lib/AddWorkspaceDialog.svelte";
  import ImagePreviewDialog from "./lib/ImagePreviewDialog.svelte";
  import MarkdownPreviewDialog from "./lib/MarkdownPreviewDialog.svelte";
    import EditorHeader from "./views/editor/EditorHeader.svelte";
  import EditorEmptyState from "./views/editor/EditorEmptyState.svelte";
  import WelcomeScreen from "./views/WelcomeScreen.svelte";
  import EditorContent from "./views/editor/EditorContent.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { toast } from "svelte-sonner";
  // Note: Button, icons, and LoadingSpinner are now only used in extracted view components

  // Import stores
  import {
    entryStore,
    uiStore,
    collaborationStore,
    workspaceStore,
    getThemeStore,
    shareSessionStore
  } from "./models/stores";
  import { getFormattingStore } from "./lib/stores/formattingStore.svelte";


  // Import auth
  import { initAuth, getCurrentWorkspace, verifyMagicLink, setServerUrl, refreshUserInfo, getAuthState, isAuthenticated, getWorkspaces, isSyncEnabled } from "./lib/auth";
  import { getLocalWorkspace, getLocalWorkspaces, getCurrentWorkspaceId, getWorkspaceStorageType, discoverOpfsWorkspaces } from "$lib/storage/localWorkspaceRegistry.svelte";

  // Initialize theme store immediately
  getThemeStore();

  // Initialize formatting store
  const formattingStore = getFormattingStore();

  // Import services
  import {
    revokeBlobUrls,
    transformAttachmentPaths,
    initializeWorkspaceCrdt,
    updateCrdtFileMetadata,
    setShareServerUrl,
    joinShareSession,
  } from "./models/services";
  import { getMimeType, isHeicFile, convertHeicToJpeg } from "./models/services/attachmentService";

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
    renameEntry as renameEntryController,
    duplicateEntry as duplicateEntryController,
    handleValidateWorkspace as validateWorkspaceHandler,
    handleRefreshTree as refreshTreeHandler,
    handleDuplicateCurrentEntry as duplicateCurrentEntryHandler,
    handleRenameCurrentEntry as renameCurrentEntryHandler,
    handleDeleteCurrentEntry as deleteCurrentEntryHandler,
    handleMoveCurrentEntry as moveCurrentEntryHandler,
    handleCreateChildUnderCurrent as createChildUnderCurrentHandler,
    handleStartShareSession as startShareSessionHandler,
    handleJoinShareSession as joinShareSessionHandler,
    handleFindInFile,
    handleWordCount as wordCountHandler,
    handleImportFromClipboard as importFromClipboardHandler,
    handleCopyAsMarkdown as copyAsMarkdownHandler,
    handleViewMarkdown as viewMarkdownHandler,
    handleReorderFootnotes as reorderFootnotesHandler,
    handleAddAttachment as addAttachmentHandler,
    handleAttachmentFileSelect as attachmentFileSelectHandler,
    handleEditorFileDrop as editorFileDropHandler,
    handleDeleteAttachment as deleteAttachmentHandler,
    handleAttachmentInsert as attachmentInsertHandler,
    handleMoveAttachment as moveAttachmentHandler,
    populateCrdtBeforeHost,
    handleLinkClick as linkClickHandler,
  } from "./controllers";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // ========================================================================
  // Store-backed state (using getters for now, will migrate fully later)
  // This allows gradual migration without breaking the component
  // ========================================================================

  // Entry state - proxied from entryStore
  let currentEntry = $derived(entryStore.currentEntry);
  let isDirty = $derived(entryStore.isDirty);
  let isSaving = $derived(entryStore.isSaving);
  // Editor is read-only when guest is in a read-only session
  let editorReadonly = $derived(shareSessionStore.isGuest && shareSessionStore.readOnly);
  let isLoading = $derived(entryStore.isLoading);
  let titleError = $derived(entryStore.titleError);
  let displayContent = $derived(entryStore.displayContent);

  // UI state - proxied from uiStore
  let leftSidebarCollapsed = $derived(uiStore.leftSidebarCollapsed);
  let rightSidebarCollapsed = $derived(uiStore.rightSidebarCollapsed);
  let showSettingsDialog = $derived(uiStore.showSettingsDialog);
  let showExportDialog = $derived(uiStore.showExportDialog);
  let showNewEntryModal = $derived(uiStore.showNewEntryModal);
  let exportPath = $derived(uiStore.exportPath);
  let editorRef = $derived(uiStore.editorRef);

  // Right sidebar tab/session control
  let requestedSidebarTab: "properties" | "history" | "share" | null = $state(null);
  let triggerStartSession = $state(false);

  // Add workspace dialog
  let showAddWorkspace = $state(false);

  // Welcome screen (shown when no workspaces exist)
  let showWelcomeScreen = $state(false);

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
  let readableLineLength = $derived(workspaceStore.readableLineLength);
  let focusMode = $derived(workspaceStore.focusMode);

  // API wrapper - uses execute() internally for all operations
  let api: Api | null = $derived(backend ? createApi(backend) : null);

  // Rust CRDT API instance
  let rustApi: RustCrdtApi | null = $state(null);

  // Track whether initial guest sync has completed (to avoid re-opening root on every update)
  let guestInitialSyncDone = $state(false);

  // Track whether the current entry is a daily entry (for prev/next navigation)
  let isDailyEntry = $state(false);

  // Collaboration state - proxied from collaborationStore
  let collaborationEnabled = $derived(collaborationStore.collaborationEnabled);
  let collaborationServerUrl = $derived(collaborationStore.collaborationServerUrl);

  // Note: Per-document YDocProxy removed - sync now happens at workspace level

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

  // CRDT bridge callback cleanup functions
  let cleanupSessionSync: (() => void) | null = null;
  let cleanupBodyChange: (() => void) | null = null;
  let cleanupFileChange: (() => void) | null = null;
  let cleanupFileRenamed: (() => void) | null = null;
  let cleanupSyncProgress: (() => void) | null = null;
  let cleanupSyncStatus: (() => void) | null = null;
  let pendingCurrentRenameHint:
    | { oldCanonical: string; oldPartOf: string | null; expiresAt: number }
    | null = null;

  // Set VITE_DISABLE_WORKSPACE_CRDT=true to disable workspace CRDT for debugging
  // This keeps per-file collaboration working but disables the workspace-level sync
  const workspaceCrdtDisabled: boolean =
    typeof import.meta !== "undefined" &&
    (import.meta as any).env?.VITE_DISABLE_WORKSPACE_CRDT === "true";


  // Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri)
  function normalizeFrontmatter(frontmatter: any): Record<string, any> {
    if (!frontmatter) return {};
    if (frontmatter instanceof Map) {
      return Object.fromEntries(frontmatter.entries());
    }
    return frontmatter;
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

  function extractMarkdownLinkPath(value: string): string | null {
    // Mirror the key behavior from diaryx_core/link_parser.rs::parse_link:
    // parse [Title](path) and [Title](<path>) forms.
    if (!value.startsWith("[")) return null;

    const closeBracket = value.indexOf("]");
    if (closeBracket <= 0) return null;
    if (value.slice(closeBracket, closeBracket + 2) !== "](") return null;

    const rest = value.slice(closeBracket + 2);
    if (!rest) return null;

    // Angle-bracket URL form: [Title](<path with spaces>)
    if (rest.startsWith("<")) {
      const closeAngle = rest.indexOf(">");
      if (closeAngle <= 1) return null;
      if (rest.slice(closeAngle + 1, closeAngle + 2) !== ")") return null;
      return rest.slice(1, closeAngle).trim() || null;
    }

    // Standard URL form with support for balanced parentheses in the path.
    let depth = 0;
    for (let i = 0; i < rest.length; i++) {
      const ch = rest[i];
      if (ch === "(") {
        depth++;
      } else if (ch === ")") {
        if (depth === 0) {
          return rest.slice(0, i).trim() || null;
        }
        depth--;
      }
    }

    return null;
  }

  function normalizeRelativePath(path: string): string {
    const segments = path.split("/");
    const normalized: string[] = [];

    for (const segment of segments) {
      if (!segment || segment === ".") continue;
      if (segment === "..") {
        if (normalized.length > 0) normalized.pop();
        continue;
      }
      normalized.push(segment);
    }

    return normalized.join("/");
  }

  function normalizePartOfValue(
    value: unknown,
    currentCanonicalPath: string | null = null,
  ): string | null {
    if (typeof value !== "string") return null;

    const trimmed = value.trim();
    if (!trimmed) return null;

    let partOf = extractMarkdownLinkPath(trimmed) ?? trimmed;

    // Strip optional angle-bracket wrapped paths for plain-path input.
    if (partOf.startsWith("<") && partOf.endsWith(">")) {
      partOf = partOf.slice(1, -1).trim();
    }

    // Workspace-root links become canonical by removing leading slash.
    if (partOf.startsWith("/")) {
      return normalizeRelativePath(partOf.slice(1)) || null;
    }

    const isRelative =
      partOf.startsWith("./") ||
      partOf.startsWith("../") ||
      partOf === "." ||
      partOf === "..";

    if (!isRelative) {
      // Ambiguous/legacy paths are treated as canonical by default.
      return normalizeRelativePath(partOf) || null;
    }

    if (!currentCanonicalPath) {
      return normalizeRelativePath(partOf) || null;
    }

    const currentSegments = currentCanonicalPath.split("/");
    currentSegments.pop(); // remove filename
    const baseDir = currentSegments.join("/");
    const combined = baseDir ? `${baseDir}/${partOf}` : partOf;
    return normalizeRelativePath(combined) || null;
  }

  function getPartOf(
    frontmatter: Record<string, unknown> | undefined,
    currentCanonicalPath: string | null = null,
  ): string | null {
    const partOf = normalizeFrontmatter(frontmatter)?.part_of;
    return normalizePartOfValue(partOf, currentCanonicalPath);
  }

  function isTransientEntryReadError(error: unknown): boolean {
    const message = error instanceof Error ? error.message : String(error);
    return (
      message.includes("NotFoundError") ||
      message.includes("Failed to read file") ||
      message.includes("A requested file or directory could not be found") ||
      message.includes("The object can not be found here")
    );
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

  // Reset guest initial sync flag when leaving guest mode
  $effect(() => {
    if (!shareSessionStore.isGuest) {
      guestInitialSyncDone = false;
    }
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

    // Load saved collaboration settings
    // Note: We only load the URL into the store, but do NOT call setWorkspaceServer()
    // or setCollaborationServer() here. Those are called by initializeWorkspaceCrdt()
    // only when collaborationEnabled is true. This prevents the sync bridge from
    // trying to connect when there's no active sync session.
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
    // and BEFORE setupWorkspaceCrdt() so the CRDT initializes with auth
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
        // This ensures workspace CRDT is initialized with auth credentials
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

    // Check for local edit mode params (from `diaryx edit` CLI command)
    // These override the sync server URL and auto-join a guest session
    let localEditParams: { syncUrl: string; joinCode: string } | null = null;
    if (typeof window !== "undefined") {
      const params = new URLSearchParams(window.location.search);
      const syncUrl = params.get("sync_url");
      const joinCode = params.get("join_code");
      if (syncUrl && joinCode) {
        localEditParams = { syncUrl, joinCode };
        // Clear query params from URL to prevent re-joining on reload
        const url = new URL(window.location.href);
        url.searchParams.delete("sync_url");
        url.searchParams.delete("join_code");
        window.history.replaceState({}, "", url.toString());
        console.log('[App] Local edit mode detected, sync_url:', syncUrl, 'join_code:', joinCode);
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
        // No workspaces exist — show welcome screen
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
      const backendInstance = await getBackend(wsId, wsName, wsId ? getWorkspaceStorageType(wsId) : undefined);
      workspaceStore.setBackend(backendInstance);

      // Set the backend API for CRDT bridge (used for writing synced files to disk)
      const apiInstance = createApi(backendInstance);
      setBackendApi(apiInstance);
      setBackend(backendInstance);

      // Initialize filesystem event subscription for automatic UI updates
      cleanupEventSubscription = initEventSubscription(backendInstance);

      // Initialize Rust CRDT API
      rustApi = new RustCrdtApi(backendInstance);

      // Initialize workspace CRDT (unless disabled or in local edit mode)
      if (localEditParams) {
        // Local edit mode: override the share server URL and auto-join
        console.log('[App] Joining local edit session...');
        setShareServerUrl(localEditParams.syncUrl);
        try {
          workspaceStore.saveTreeState();
          await joinShareSession(localEditParams.joinCode);
          console.log('[App] Successfully joined local edit session');
        } catch (e) {
          console.error('[App] Failed to join local edit session:', e);
          workspaceStore.clearSavedTreeState();
          setShareServerUrl(null);
          // Fall back to normal workspace CRDT setup
          await setupWorkspaceCrdt();
          const syncCompleted = await waitForInitialSync(10000);
          if (syncCompleted) {
            console.log('[App] Fallback: initial sync complete');
          }
        }
      } else if (!workspaceCrdtDisabled) {
        await setupWorkspaceCrdt();

        // Wait for initial sync to complete before building tree
        // This ensures synced files are available for display
        console.log('[App] Waiting for initial sync to complete...');
        const syncCompleted = await waitForInitialSync(10000);
        if (syncCompleted) {
          console.log('[App] Initial sync complete, proceeding with tree refresh');
        } else {
          console.warn('[App] Initial sync timed out or not applicable, proceeding anyway');
        }
      } else {
        console.log(
          "[App] Workspace CRDT disabled via VITE_DISABLE_WORKSPACE_CRDT",
        );
      }

      await refreshTree();

      // Note: With multiplexed body sync, we no longer need to proactively
      // sync all files. Files are subscribed on-demand when opened, using a
      // single WebSocket connection for all body syncs.

      // Register callback to refresh tree when session data is received
      cleanupSessionSync = onSessionSync(async () => {
        if (shareSessionStore.isGuest) {
          // Guest mode: build tree from CRDT (guests don't have files on disk)
          console.log('[App] Session sync received (guest mode), building tree from CRDT');
          const crdtTree = await getTreeFromCrdt();
          if (crdtTree) {
            console.log('[App] Setting tree from CRDT:', crdtTree);
            workspaceStore.setTree(crdtTree);

            // Only open root entry on the first sync, not on every update.
            // Set the flag synchronously BEFORE awaiting to prevent concurrent
            // callback invocations from also entering this branch.
            if (!guestInitialSyncDone) {
              guestInitialSyncDone = true;
              console.log('[App] Guest session - initial sync, opening root entry:', crdtTree.path);
              workspaceStore.expandNode(crdtTree.path);

              // With multiplexed body sync, the root entry's body will be
              // synced on-demand when opened via ensureBodySync
              await openEntry(crdtTree.path);
            } else {
              console.log('[App] Guest session - incremental sync, tree updated');
            }
          } else {
            console.log('[App] No CRDT tree available, falling back to filesystem refresh');
            await refreshTree();
          }
        } else {
          // Device-to-device sync: files were written to disk, refresh tree from filesystem
          console.log('[App] Session sync received (device sync), refreshing tree from filesystem');
          await refreshTree();

          // If no entry is open yet, open the root
          if (tree && !currentEntry) {
            console.log('[App] Opening root entry after device sync:', tree.path);
            workspaceStore.expandNode(tree.path);
            await openEntry(tree.path);
          }
        }
      });

      // Register callback to reload editor when remote body changes arrive
      cleanupBodyChange = onBodyChange(async (path, body) => {
        // path is canonical (e.g., "file.md"), but currentEntry.path may be storage path
        // (e.g., "guest/abc123/file.md" for guests). Normalize for comparison.
        const currentCanonical = currentEntry ? await getCanonicalPathForSync(currentEntry.path) : null;
        console.log('[App] Body change received for:', path, 'current entry canonical:', currentCanonical);
        // Only update if this is the currently open file
        if (currentEntry && path === currentCanonical) {
          console.log('[App] Updating display content with remote body, length:', body.length);
          // Transform attachment paths to blob URLs for display
          const transformed = await transformAttachmentPaths(body, currentEntry.path, api);
          entryStore.setDisplayContent(transformed);
        }
      });

      // Register callback to reload entry when remote metadata changes arrive
      // This ensures the RightSidebar shows updated properties from sync
      cleanupFileChange = onFileChange(async (path, metadata) => {
        // path is canonical, but currentEntry.path may be storage path. Normalize for comparison.
        const currentCanonical = currentEntry ? await getCanonicalPathForSync(currentEntry.path) : null;
        const now = Date.now();
        if (pendingCurrentRenameHint && pendingCurrentRenameHint.expiresAt <= now) {
          pendingCurrentRenameHint = null;
        }

        // Fallback for remote renames that arrive as delete+create instead of FileRenamed.
        if (currentEntry && path && !metadata && path === currentCanonical) {
          pendingCurrentRenameHint = {
            oldCanonical: path,
            oldPartOf: getPartOf(currentEntry.frontmatter, currentCanonical),
            expiresAt: now + 5000,
          };
        }

        // Remap current entry when a likely rename target appears.
        if (currentEntry && api && path && metadata && path !== currentCanonical) {
          const incomingPartOf = normalizePartOfValue(metadata.part_of, path);
          const currentPartOf = getPartOf(currentEntry.frontmatter, currentCanonical);
          const partOfMatches =
            incomingPartOf !== null &&
            currentPartOf !== null &&
            incomingPartOf === currentPartOf;

          const matchedDeleteCreateRename =
            !!pendingCurrentRenameHint &&
            pendingCurrentRenameHint.oldCanonical === currentCanonical &&
            pendingCurrentRenameHint.oldPartOf === incomingPartOf;

          let currentMissingOnDisk = false;
          if (!matchedDeleteCreateRename && partOfMatches) {
            try {
              currentMissingOnDisk = !(await api.fileExists(currentEntry.path));
            } catch {
              // Ignore transient backend errors for fallback detection.
            }
          }

          if (
            matchedDeleteCreateRename ||
            (currentMissingOnDisk && partOfMatches)
          ) {
            console.log('[App] Current entry remapped via metadata fallback:', currentCanonical, '->', path);
            pendingCurrentRenameHint = null;

            entryStore.setCurrentEntry({
              ...currentEntry,
              path,
            });

            if (collaborationEnabled) {
              collaborationStore.setCollaborationPath(toCollaborationPath(path));
            }

            if (!isDirty) {
              await openEntryController(api, path, tree, collaborationEnabled);
            }
            return;
          }
        }

        // Only update if this is the currently open file and we have valid metadata
        if (currentEntry && api && metadata && path === currentCanonical) {
          console.log('[App] Metadata change received for current entry:', path);
          try {
            // Reload with bounded retry: safe-write swaps can create brief NotFound windows.
            let entry = null;
            let lastError: unknown = null;
            for (let attempt = 0; attempt < 8; attempt++) {
              try {
                entry = await api.getEntry(currentEntry.path);
                break;
              } catch (e) {
                lastError = e;
                if (!isTransientEntryReadError(e)) {
                  throw e;
                }
                if (attempt < 7) {
                  await new Promise((resolve) => setTimeout(resolve, 100 * (attempt + 1)));
                }
              }
            }
            if (!entry) throw lastError ?? new Error('Failed to reload entry after metadata change');

            entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
            // Update the current entry - this will trigger RightSidebar to re-render
            entryStore.setCurrentEntry(entry);
          } catch (e) {
            // Sync-safe writes can briefly make files unreadable; avoid noisy warnings
            // for transient NotFound windows and let later file events refresh again.
            if (isTransientEntryReadError(e)) {
              console.log('[App] Metadata reload deferred due to transient file state');
            } else {
              console.warn('[App] Failed to reload entry after metadata change:', e);
            }
          }
        }

        // Refresh tree when:
        // 1. contents changed (local file added to parent)
        // 2. path is null (remote sync completed - we don't know what changed)
        // Use debounced version to prevent rapid refreshes during sync
        if ((metadata && metadata.contents) || path === null) {
          console.log('[App] File change detected - scheduling debounced tree refresh');
          debouncedRefreshTree();
        }
      });

      // Register callback to remap the currently open entry when a file is renamed.
      // This keeps metadata/body updates targeting the renamed canonical path.
      cleanupFileRenamed = onFileRenamed(async (oldPath, newPath) => {
        if (!api || !currentEntry) return;

        const currentCanonical = await getCanonicalPathForSync(currentEntry.path);
        if (currentCanonical !== oldPath) return;

        console.log('[App] Current entry renamed:', oldPath, '->', newPath);

        // Remap path immediately so upcoming metadata/body events match this entry.
        entryStore.setCurrentEntry({
          ...currentEntry,
          path: newPath,
        });

        // Keep collaboration tracking aligned even when we avoid a full reopen
        // (e.g. while preserving local unsaved edits).
        if (collaborationEnabled) {
          collaborationStore.setCollaborationPath(toCollaborationPath(newPath));
        }
        pendingCurrentRenameHint = null;

        // If there are no unsaved local edits, reopen the entry at its new path to
        // refresh frontmatter and keep body sync subscriptions aligned.
        if (!isDirty) {
          await openEntryController(api, newPath, tree, collaborationEnabled);
        }
      });

      // Register sync progress callback to update collaborationStore
      cleanupSyncProgress = onSyncProgress((completed, total) => {
        collaborationStore.setSyncProgress({ completed, total });
      });

      // Register sync status callback to update collaborationStore
      cleanupSyncStatus = onSyncStatus((status, error) => {
        if (error) {
          collaborationStore.setSyncError(error);
        } else {
          collaborationStore.setSyncStatus(status);
        }
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
      // - Swipe right from left edge: open left sidebar
      // - Swipe left from right edge: open right sidebar
      let touchStartY = 0;
      let touchStartX = 0;
      const EDGE_THRESHOLD = 30; // pixels from edge to start swipe
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
        const screenWidth = window.innerWidth;

        // Swipe down from top 100px of screen, mostly vertical → open command palette
        if (touchStartY < 100 && deltaY > SWIPE_THRESHOLD && absDeltaX < CROSS_AXIS_MAX) {
          uiStore.openCommandPalette();
          return;
        }

        // Swipe right from left edge, mostly horizontal → open left sidebar
        if (touchStartX < EDGE_THRESHOLD && deltaX > SWIPE_THRESHOLD && absDeltaY < CROSS_AXIS_MAX) {
          if (leftSidebarCollapsed) {
            toggleLeftSidebar();
          }
          return;
        }

        // Swipe left from right edge, mostly horizontal → open right sidebar
        if (touchStartX > screenWidth - EDGE_THRESHOLD && deltaX < -SWIPE_THRESHOLD && absDeltaY < CROSS_AXIS_MAX) {
          if (rightSidebarCollapsed) {
            toggleRightSidebar();
          }
          return;
        }
      };
      document.addEventListener("touchstart", handleTouchStart);
      document.addEventListener("touchend", handleTouchEnd);

    } catch (e) {
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
    // Cleanup CRDT bridge callbacks (prevents accumulation on HMR)
    cleanupSessionSync?.();
    cleanupBodyChange?.();
    cleanupFileChange?.();
    cleanupFileRenamed?.();
    cleanupSyncProgress?.();
    cleanupSyncStatus?.();
    // Disconnect workspace CRDT (keeps local state for quick reconnect)
    disconnectWorkspace();
    // Cleanup import:complete listener
    window.removeEventListener("import:complete", handleImportComplete);
  });

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
    rustApi = new RustCrdtApi(newBackend);
    // Refresh tree and validation from new workspace
    await refreshTree();
    await runValidation();
    entryStore.setLoading(false);
  }

  // Initialize the workspace CRDT
  async function setupWorkspaceCrdt() {
    if (!api || !backend || !rustApi) return;

    try {
      // Get workspace ID from auth store (server is source of truth)
      // When authenticated, the server generates and stores the workspace UUID
      // For local-only mode (not signed in), we use null
      const defaultWorkspace = getCurrentWorkspace();
      const sharedWorkspaceId = defaultWorkspace?.id ?? null;

      if (sharedWorkspaceId) {
        console.log("[App] Using workspace_id from server:", sharedWorkspaceId);
      } else {
        console.log("[App] No authenticated workspace, using local-only mode");
      }

      // Get the workspace directory from the backend, then find the actual root index
      const workspaceDir = backend.getWorkspacePath().replace(/\/index\.md$/, '').replace(/\/README\.md$/, '');
      console.log("[App] Workspace directory:", workspaceDir);

      let workspacePath: string | undefined;
      try {
        const foundRoot = await api.findRootIndex(workspaceDir);
        if (!foundRoot) {
          throw new Error("Root index not found");
        }
        workspacePath = foundRoot;
        console.log("[App] Found root index at:", workspacePath);
      } catch (e) {
        console.warn("[App] Could not find root index (workspace may be empty):", e);
        // No auto-creation — the EditorEmptyState will offer a "Create Root Index" button
      }

      // IMPORTANT: Populate CRDT from filesystem BEFORE connecting to server
      // This ensures our local files are available to sync to other devices
      // At startup, reconciles file mtime vs CRDT modified_at - if file is newer, CRDT is updated
      if (sharedWorkspaceId && workspacePath) {
        console.log("[App] Initializing CRDT from filesystem via Rust command...");
        try {
          const result = await api.initializeWorkspaceCrdt(workspacePath);
          console.log("[App] CRDT initialized:", result);
          // Show toast if files were updated from disk (external edits detected)
          if (result.includes("updated from disk")) {
            toast.info(result);
          }
        } catch (e) {
          console.warn("[App] Failed to initialize CRDT from filesystem:", e);
          // Continue anyway - server sync may bring in data
        }
      }

      // Set workspace ID for per-file document room naming
      // If null, rooms will be "doc:{path}" instead of "{id}:doc:{path}"
      setWorkspaceId(sharedWorkspaceId);

      // Initialize workspace CRDT using service with Rust API
      // Only if we have a valid workspace path (skip for empty workspaces)
      if (workspacePath) {
        const initialized = await initializeWorkspaceCrdt(
          sharedWorkspaceId,
          workspacePath,
          collaborationServerUrl,
          collaborationEnabled,
          rustApi,
          {
            onConnectionChange: (connected: boolean) => {
              console.log("[App] Workspace CRDT connection:", connected ? "online" : "offline");
              collaborationStore.setConnected(connected);
            },
          },
        );
        workspaceStore.setWorkspaceCrdtInitialized(initialized);
      } else {
        console.log("[App] Skipping CRDT init — no root index found (empty workspace)");
        workspaceStore.setWorkspaceCrdtInitialized(false);
      }
    } catch (e) {
      console.error("[App] Failed to initialize workspace CRDT:", e);
      workspaceStore.setWorkspaceCrdtInitialized(false);
    }
  }

  // Open an entry - thin wrapper that handles auto-save and delegates to controller
  async function openEntry(path: string) {
    if (!api || !backend) return;

    // Auto-save before switching documents
    if (isDirty) {
      cancelAutoSave();
      await save();
    }

    // Delegate to controller
    await openEntryController(api, path, tree, collaborationEnabled);

    // Check if this is a daily entry for prev/next navigation
    if (api) {
      isDailyEntry = await api.isDailyEntry(path);
    }
  }

  // Save current entry - delegates to controller with sync support
  async function save() {
    if (!api || !currentEntry || !editorRef) return;
    await saveEntryWithSync(api, currentEntry, editorRef, tree?.path);
  }

  // Cancel pending auto-save
  function cancelAutoSave() {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
  }

  // Schedule auto-save with debounce
  function scheduleAutoSave() {
    cancelAutoSave();
    autoSaveTimer = setTimeout(() => {
      autoSaveTimer = null;
      if (isDirty) {
        save();
      }
    }, AUTO_SAVE_DELAY_MS);
  }

  // Handle content changes - triggers debounced auto-save
  // Note: CRDT sync happens at save time via workspaceCrdtBridge, not on each keystroke
  function handleContentChange(_markdown: string) {
    entryStore.markDirty();
    scheduleAutoSave();
  }

  // Handle editor blur - save immediately if dirty
  function handleEditorBlur() {
    cancelAutoSave();
    if (isDirty) {
      save();
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

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
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
    // Navigate daily entries with Alt+Left/Right
    if (event.altKey && isDailyEntry) {
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        handlePrevDay();
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        handleNextDay();
      }
    }
  }

  /**
   * Handle magic link token verification from URL.
   * Verifies the token automatically and shows sync progress in SyncStatusIndicator.
   */
  async function handleMagicLinkToken(token: string) {
    // Show connecting status while verifying
    collaborationStore.setSyncStatus('connecting');

    try {
      // Verify the magic link token
      // Note: URL token is cleared before this function is called to prevent double verification
      await verifyMagicLink(token);

      // Set status to idle - workspace CRDT will update to 'synced' when connected
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

      // Refresh the tree after sync completes (handled by onSessionSync callback)
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
      rustApi = new RustCrdtApi(newBackend);

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

  async function handleDailyEntry() {
    if (!api || !tree) return;
    try {
      // daily_entry_folder and daily_template are now read from workspace config
      // by the command handler. Pass null to use workspace config defaults.
      const path = await api.ensureDailyEntry(tree.path, undefined, undefined);
      await refreshTree();
      await openEntry(path);
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Navigate to the previous day's entry
  async function handlePrevDay() {
    if (!api || !currentEntry) return;
    try {
      const prevPath = await api.getAdjacentDailyEntry(currentEntry.path, 'prev');
      if (prevPath) {
        // Check if entry exists before navigating
        const exists = await api.fileExists(prevPath);
        if (exists) {
          await openEntry(prevPath);
        } else {
          // Entry doesn't exist - show a subtle notification
          uiStore.setError("No entry for previous day");
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Navigate to the next day's entry
  async function handleNextDay() {
    if (!api || !currentEntry) return;
    try {
      const nextPath = await api.getAdjacentDailyEntry(currentEntry.path, 'next');
      if (nextPath) {
        // Check if entry exists before navigating
        const exists = await api.fileExists(nextPath);
        if (exists) {
          await openEntry(nextPath);
        } else {
          // Entry doesn't exist - show a subtle notification
          uiStore.setError("No entry for next day");
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Rename an entry - delegates to controller with sync support
  async function handleRenameEntry(path: string, newFilename: string): Promise<string> {
    if (!api) throw new Error("API not initialized");
    if (currentEntry?.path === path && isDirty && editorRef) {
      await saveEntryWithSync(api, currentEntry, editorRef, tree?.path);
    }
    const parentPath = workspaceStore.getParentNodePath(path);
    const newPath = await renameEntryController(api, path, newFilename, async () => {
      await refreshTree();
      if (parentPath) {
        await loadNodeChildren(parentPath);
      }
      await runValidation();
    });
    if (currentEntry?.path === path) {
      entryStore.setCurrentEntry({
        ...currentEntry,
        path: newPath,
      });
      if (collaborationEnabled) {
        collaborationStore.setCollaborationPath(toCollaborationPath(newPath));
      }
    }
    return newPath;
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

  // Delete an entry - delegates to controller with sync support
  async function handleDeleteEntry(path: string) {
    if (!api) return;
    const parentPath = workspaceStore.getParentNodePath(path);
    await deleteEntryWithSync(api, path, currentEntry?.path ?? null, async () => {
      await refreshTree();
      if (parentPath) {
        await loadNodeChildren(parentPath);
      }
      await runValidation();
    });
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
    await refreshTreeController(api, backend, showUnlinkedFiles, showHiddenFiles);
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
    await loadNodeChildrenController(api, nodePath, showUnlinkedFiles, showHiddenFiles);
  }

  // ========================================================================
  // Command Palette Handlers - Thin wrappers that delegate to controllers
  // ========================================================================

  async function handleValidateWorkspace() {
    if (!api || !backend) return;
    await validateWorkspaceHandler(api, tree, backend);
  }

  async function handleRefreshTreeCmd() {
    await refreshTreeHandler(refreshTree);
  }

  async function handleDuplicateCurrentEntry() {
    if (!api) return;
    await duplicateCurrentEntryHandler(api, currentEntry, handleDuplicateEntry, openEntry);
  }

  async function handleRenameCurrentEntry() {
    if (!api) return;
    await renameCurrentEntryHandler(api, currentEntry, handleRenameEntry, openEntry);
  }

  async function handleDeleteCurrentEntry() {
    await deleteCurrentEntryHandler(currentEntry, handleDeleteEntry);
  }

  async function handleMoveCurrentEntry() {
    if (!api) return;
    await moveCurrentEntryHandler(api, currentEntry, tree, handleMoveEntry);
  }

  async function handleCreateChildUnderCurrent() {
    await createChildUnderCurrentHandler(currentEntry, handleCreateChildEntry);
  }

  async function handleStartShareSession() {
    await startShareSessionHandler(
      (collapsed) => uiStore.setRightSidebarCollapsed(collapsed),
      (tab) => { requestedSidebarTab = tab; },
      (trigger) => { triggerStartSession = trigger; }
    );
  }

  async function handleJoinShareSessionCmd() {
    await joinShareSessionHandler();
  }

  function handleWordCount() {
    wordCountHandler(editorRef, currentEntry);
  }

  async function handleImportFromClipboard() {
    if (!api) return;
    await importFromClipboardHandler(api, tree, refreshTree, openEntry);
  }

  async function handleCopyAsMarkdown() {
    await copyAsMarkdownHandler(editorRef, currentEntry);
  }

  function handleReorderFootnotes() {
    reorderFootnotesHandler(editorRef);
  }

  function handleViewMarkdown() {
    const result = viewMarkdownHandler(editorRef, currentEntry);
    if (result !== null) {
      markdownPreviewBody = result.body;
      markdownPreviewFrontmatter = result.frontmatter;
      markdownPreviewOpen = true;
    }
  }

  // ========================================================================
  // Attachment Handlers - Thin wrappers that delegate to controllers
  // ========================================================================

  async function handlePopulateCrdtBeforeHost(audience: string | null = null) {
    if (!api) return;
    await populateCrdtBeforeHost(api, tree?.path ?? null, audience);
  }

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
            updateCrdtFileMetadata(path, updatedEntry.frontmatter);
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
        updateCrdtFileMetadata(path, updatedEntry.frontmatter);

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
      const path = currentEntry.path;
      await api.removeFrontmatterProperty(currentEntry.path, key);
      // Update local state
      const newFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);
      delete newFrontmatter[key];
      entryStore.setCurrentEntry({ ...currentEntry, frontmatter: newFrontmatter });

      // Update CRDT
      updateCrdtFileMetadata(path, newFrontmatter);
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handlePropertyAdd(key: string, value: unknown) {
    if (!api || !currentEntry) return;
    try {
      const path = currentEntry.path;
      await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue, tree?.path);
      const normalizedFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);
      // Update local state
      const updatedEntry = {
        ...currentEntry,
        frontmatter: { ...normalizedFrontmatter, [key]: value },
      };
      entryStore.setCurrentEntry(updatedEntry);

      // Update CRDT
      updateCrdtFileMetadata(path, updatedEntry.frontmatter);
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
  {tree}
  {api}
  currentEntryPath={currentEntry?.path ?? null}
  onOpenEntry={openEntry}
  onNewEntry={() => uiStore.openNewEntryModal()}
  onDailyEntry={handleDailyEntry}
  onSettings={() => (showSettingsDialog = true)}
  onExport={() => {
    exportPath = currentEntry?.path ?? tree?.path ?? "";
    if (exportPath) showExportDialog = true;
  }}
  onValidate={handleValidateWorkspace}
  onRefreshTree={handleRefreshTreeCmd}
  onDuplicateEntry={handleDuplicateCurrentEntry}
  onRenameEntry={handleRenameCurrentEntry}
  onDeleteEntry={handleDeleteCurrentEntry}
  onMoveEntry={handleMoveCurrentEntry}
  onCreateChildEntry={handleCreateChildUnderCurrent}
  onStartShare={handleStartShareSession}
  onJoinSession={handleJoinShareSessionCmd}
  onFindInFile={handleFindInFile}
  onWordCount={handleWordCount}
  onImportFromClipboard={handleImportFromClipboard}
  onCopyAsMarkdown={handleCopyAsMarkdown}
  onViewMarkdown={handleViewMarkdown}
  onReorderFootnotes={handleReorderFootnotes}
/>

<SettingsDialog
  bind:open={showSettingsDialog}
  bind:showUnlinkedFiles
  bind:showHiddenFiles
  bind:showEditorTitle
  bind:showEditorPath
  bind:readableLineLength
  bind:focusMode
  workspacePath={tree?.path}
  initialTab={settingsInitialTab}
  onAddWorkspace={async () => {
    showSettingsDialog = false;
    await tick();
    showAddWorkspace = true;
  }}
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
      // Let final sync writes settle, then refresh tree.
      debouncedRefreshTree();
    }
  }}
/>

<!-- Toast Notifications -->
<Toaster />

<!-- Tooltip Provider for keyboard shortcut hints -->
<Tooltip.Provider>

{#if showWelcomeScreen}
  <WelcomeScreen
    onGetStarted={() => { showAddWorkspace = true; }}
  />
{:else}
<div class="flex h-dvh bg-background overflow-hidden pt-[env(safe-area-inset-top)]">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
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
    onAddWorkspace={() => { showAddWorkspace = true; }}
    onMoveEntry={handleMoveEntry}
    onCreateChildEntry={handleCreateChildEntry}
    onDeleteEntry={handleDeleteEntry}
    onExport={(path) => {
      exportPath = path;
      showExportDialog = true;
    }}
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
  />

  <!-- Hidden file input for attachments (accepts all file types) -->
  <input
    type="file"
    bind:this={attachmentFileInput}
    onchange={handleAttachmentFileSelect}
    class="hidden"
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0 relative">
    {#if currentEntry}
      <EditorHeader
        title={getEntryTitle(currentEntry)}
        path={currentEntry.path}
        {isDirty}
        {isSaving}
        showTitle={showEditorTitle}
        showPath={showEditorPath}
        leftSidebarOpen={!leftSidebarCollapsed}
        rightSidebarOpen={!rightSidebarCollapsed}
        {focusMode}
        readonly={editorReadonly}
        {isDailyEntry}
        onSave={save}
        onToggleLeftSidebar={toggleLeftSidebar}
        onToggleRightSidebar={toggleRightSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        onPrevDay={handlePrevDay}
        onNextDay={handleNextDay}
        onAddWorkspace={() => (showAddWorkspace = true)}
      />

      <EditorContent
        {Editor}
        bind:editorRef
        content={displayContent}
        editorKey={currentEntry.path}
        {readableLineLength}
        readonly={editorReadonly}
        onchange={handleContentChange}
        onblur={handleEditorBlur}
        entryPath={currentEntry.path}
        {api}
        onAttachmentInsert={handleAttachmentInsert}
        onFileDrop={handleEditorFileDrop}
        onLinkClick={handleLinkClick}
        enableSpoilers={formattingStore.enableSpoilers}
      />
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
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
    {rustApi}
    onHistoryRestore={async () => {
      // Refresh current entry after restore
      if (currentEntry) {
        await openEntry(currentEntry.path);
      }
    }}
    onBeforeHost={async (audience) => await handlePopulateCrdtBeforeHost(audience)}
    onAddWorkspace={() => { showAddWorkspace = true; }}
    onOpenEntry={async (path) => await openEntry(path)}
    {api}
    requestedTab={requestedSidebarTab}
    onRequestedTabConsumed={() => (requestedSidebarTab = null)}
    {triggerStartSession}
    onTriggerStartSessionConsumed={() => (triggerStartSession = false)}
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
