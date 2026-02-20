<script lang="ts">
  /**
   * AddWorkspaceDialog - Workspace creation dialog
   *
   * Two orthogonal dimensions:
   * - Sync Mode: Local (this device only) or Remote (syncs to server)
   * - Content Source: From existing workspace, Import from ZIP, or Start fresh
   *
   * Screens:
   * - options (default) — name input, sync mode toggle, content source picker
   * - auth — triggered when selecting Remote mode without authentication
   * - upgrade — shown when authenticated but on free tier
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import {
    setServerUrl,
    requestMagicLink,
    verifyMagicLink,
    downloadWorkspaceSnapshot,
    uploadWorkspaceSnapshot,
    createServerWorkspace,
    isAuthenticated,
    enableSync,
    isSyncEnabled,
    getWorkspaces,
    getServerUrl,
    getAuthState,
    createCheckoutSession,
  } from "$lib/auth";
  import {
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    addLocalWorkspace,
    setCurrentWorkspaceId,
    promoteLocalWorkspace,
    createLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import {
    setActiveWorkspaceId,
    authenticateWithPasskey,
  } from "$lib/auth/authStore.svelte";
  import { isPasskeySupported } from "$lib/auth/webauthnUtils";
  import {
    Mail,
    Link,
    Loader2,
    AlertCircle,
    ArrowRight,
    ArrowLeft,
    ChevronDown,
    ChevronUp,
    Upload,
    Download,
    HardDrive,
    Plus,
    Settings2,
    Fingerprint,
    Cloud,
    FolderOpen,
  } from "@lucide/svelte";
  import VerificationCodeInput from "$lib/components/VerificationCodeInput.svelte";
  import { toast } from "svelte-sonner";
  import { untrack } from "svelte";
  import { getBackend, createApi } from "./backend";
  import { isTauri } from "$lib/backend/interface";
  import type { TreeNode } from "$lib/backend/interface";
  import {
    buildWorkspaceSnapshotUploadBlob,
    findWorkspaceRootPath,
  } from "$lib/settings/workspaceSnapshotUpload";
  import {
    isStorageTypeSupported,
    storeWorkspaceFileSystemHandle,
  } from "$lib/backend/storageType";
  import {
    waitForInitialSync,
    onSyncProgress,
    onSyncStatus,
    setWorkspaceServer,
    setWorkspaceId,
    switchWorkspace,
    getAllFiles,
    proactivelySyncBodies,
    markAllCrdtFilesAsDeleted,
  } from "$lib/crdt/workspaceCrdtBridge";

  interface Props {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
    onComplete?: () => void;
  }

  let {
    open = $bindable(false),
    onOpenChange,
    onComplete,
  }: Props = $props();

  // Screen tracking
  type Screen = 'auth' | 'upgrade' | 'options';
  let screen = $state<Screen>('options');

  // Auth screen state
  let email = $state("");
  let deviceName = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_device_name") || getDefaultDeviceName()
      : "My Device"
  );
  let serverUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || "https://sync.diaryx.org"
      : "https://sync.diaryx.org"
  );
  let showAdvanced = $state(false);
  let resendCooldown = $state(0);
  let verificationSent = $state(false);
  let devLink = $state<string | null>(null);

  // Options screen state — two dimensions
  type SyncMode = 'local' | 'remote';
  type ContentSource = 'existing_workspace' | 'import_zip' | 'start_fresh' | 'open_folder';

  let syncMode = $state<SyncMode>('local');
  let contentSource = $state<ContentSource>('start_fresh');
  let selectedSourceWorkspaceId = $state<string | null>(null);
  let selectedSourceIsServer = $state(false);
  let newWorkspaceName = $state("");

  // Import from ZIP state
  let importZipFile = $state<File | null>(null);
  let importZipFileInput = $state<HTMLInputElement | null>(null);

  // Open folder state
  let selectedFolderPath = $state<string | null>(null);
  let selectedFolderHandle = $state<FileSystemDirectoryHandle | null>(null);
  let selectedFolderName = $state<string | null>(null);

  // Tauri workspace path (only shown on Tauri)
  let workspacePath = $state('');

  // Server workspaces list (populated after auth)
  let serverWorkspacesList = $state<Array<{ id: string; name: string }>>([]);

  // Loading states
  let isValidatingServer = $state(false);
  let isSendingMagicLink = $state(false);
  let isInitializing = $state(false);
  let importProgress = $state(0);
  let progressDetail = $state<string | null>(null);
  let suppressSyncProgress = $state(false);
  let progressMode = $state<'bytes' | 'files' | 'percent' | null>(null);

  // Sync progress tracking
  let syncStatusText = $state<string | null>(null);
  let syncCompleted = $state(0);
  let syncTotal = $state(0);
  let unsubscribeProgress: (() => void) | null = null;
  let unsubscribeStatus: (() => void) | null = null;

  // Passkey state
  let passkeySupported = $state(false);
  let isAuthenticatingPasskey = $state(false);

  // Upgrade state
  let isUpgrading = $state(false);

  // Error state
  let error = $state<string | null>(null);

  // Magic link URL polling interval
  let urlCheckInterval: ReturnType<typeof setInterval> | null = null;
  let resendInterval: ReturnType<typeof setInterval> | null = null;

  // Derived: local workspaces for picker
  let localWorkspaces = $derived(getLocalWorkspaces());

  // Derived: available workspaces for "From existing workspace" content source
  let availableSourceWorkspaces = $derived.by(() => {
    const result: Array<{ id: string; name: string; isServer: boolean }> = [];
    // Server workspaces shown in both modes
    for (const ws of serverWorkspacesList) {
      result.push({ id: ws.id, name: ws.name, isServer: true });
    }
    // Local workspaces only shown in Remote mode (in Local mode they already exist)
    if (syncMode === 'remote') {
      for (const ws of localWorkspaces) {
        if (!serverWorkspacesList.some(s => s.id === ws.id)) {
          result.push({ id: ws.id, name: ws.name, isServer: false });
        }
      }
    }
    return result;
  });

  // Derived: whether the name input is read-only (auto-filled from selected workspace)
  let nameReadonly = $derived(
    contentSource === 'existing_workspace' && selectedSourceWorkspaceId !== null
  );

  // Derived: whether to show the "Open existing folder" option
  let showOpenFolder = $derived(isTauri() || isStorageTypeSupported('filesystem-access'));

  // Tauri: compute default workspace path from name
  $effect(() => {
    if (isTauri() && newWorkspaceName.trim()) {
      untrack(async () => {
        // Only auto-compute if path is empty or was auto-computed
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
          // Backend not ready yet — skip
        }
      });
    }
  });

  /** Open a native folder picker for workspace location (Tauri only). */
  async function browseFolder() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const folder = await open({ directory: true, title: 'Select Workspace Location' });
      if (folder) workspacePath = folder as string;
    } catch (e) {
      console.warn('[AddWorkspaceDialog] Browse folder error:', e);
    }
  }

  /** Open a folder picker for the "Open existing folder" content source. */
  async function openFolderPicker() {
    try {
      if (isTauri()) {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const folder = await open({ directory: true, title: 'Open Existing Folder' });
        if (folder) {
          selectedFolderPath = folder as string;
          const segments = (folder as string).replace(/[/\\]+$/, '').split(/[/\\]/);
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
      // User cancelled the picker — ignore AbortError
      if (e?.name !== 'AbortError') {
        console.warn('[AddWorkspaceDialog] Folder picker error:', e);
      }
    }
  }

  // Initialize dialog state when opened
  $effect(() => {
    if (open) {
      untrack(() => initializeDialog());
    }
  });

  /**
   * Set up initial state when the dialog opens.
   * Auto-detects the best defaults based on auth state and available workspaces.
   */
  function initializeDialog() {
    error = null;
    isInitializing = false;
    importProgress = 0;
    progressDetail = null;
    syncStatusText = null;
    importZipFile = null;
    selectedFolderPath = null;
    selectedFolderHandle = null;
    selectedFolderName = null;
    verificationSent = false;
    newWorkspaceName = getNextLocalWorkspaceName();

    if (isAuthenticated()) {
      const authState = getAuthState();
      serverWorkspacesList = getWorkspaces();

      if (authState.tier === 'plus') {
        if (serverWorkspacesList.length > 0) {
          // Server has workspaces — default to Remote + From existing
          syncMode = 'remote';
          contentSource = 'existing_workspace';
          selectedSourceIsServer = true;
          const preferred = (
            authState.activeWorkspaceId
              ? serverWorkspacesList.find(w => w.id === authState.activeWorkspaceId)
              : null
          ) ?? serverWorkspacesList[0];
          selectedSourceWorkspaceId = preferred.id;
          newWorkspaceName = preferred.name;
        } else if (localWorkspaces.length > 0) {
          // No server workspaces, but local ones exist — default to Remote + From existing (local)
          syncMode = 'remote';
          contentSource = 'existing_workspace';
          selectedSourceIsServer = false;
          selectedSourceWorkspaceId = localWorkspaces[0].id;
          newWorkspaceName = localWorkspaces[0].name;
        } else {
          // Nothing exists — default to Local + Start fresh
          syncMode = 'local';
          contentSource = 'start_fresh';
        }
      } else {
        // Free tier — default to Local
        syncMode = 'local';
        contentSource = 'start_fresh';
      }
    } else {
      // Not authenticated — default to Local + Start fresh
      syncMode = 'local';
      contentSource = 'start_fresh';
      serverWorkspacesList = [];
    }

    screen = 'options';
  }

  // Get a sensible default device name based on platform
  function getDefaultDeviceName(): string {
    if (typeof navigator === "undefined") return "My Device";

    const ua = navigator.userAgent;
    if (ua.includes("Mac")) return "Mac";
    if (ua.includes("Windows")) return "Windows PC";
    if (ua.includes("Linux")) return "Linux";
    if (ua.includes("iPhone")) return "iPhone";
    if (ua.includes("iPad")) return "iPad";
    if (ua.includes("Android")) return "Android";
    return "My Device";
  }

  // Check passkeys support on mount
  if (typeof window !== "undefined") {
    isPasskeySupported().then((v) => { passkeySupported = v; });
  }

  async function handlePasskeySignIn() {
    if (!(await validateServer())) return;
    isAuthenticatingPasskey = true;
    error = null;
    try {
      localStorage.setItem("diaryx_device_name", deviceName.trim() || getDefaultDeviceName());
      await authenticateWithPasskey(email.trim() || undefined);
      email = "";
      screen = await handlePostAuth();
    } catch (e) {
      error = e instanceof Error ? e.message : "Passkey authentication failed";
    } finally {
      isAuthenticatingPasskey = false;
    }
  }

  // Validate and apply server URL
  async function validateServer(): Promise<boolean> {
    let url = serverUrl.trim();
    if (!url) {
      error = "Please enter a server URL";
      return false;
    }

    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      url = "https://" + url;
      serverUrl = url;
    }

    isValidatingServer = true;
    error = null;

    try {
      const response = await fetch(`${url}/health`, {
        method: "GET",
        signal: AbortSignal.timeout(5000),
      });

      if (!response.ok) {
        throw new Error("Server returned an error");
      }

      setServerUrl(url);
      collaborationStore.setServerUrl(toWebSocketUrl(url));
      collaborationStore.setSyncStatus('idle');

      return true;
    } catch (e) {
      if (e instanceof Error && e.name === "TimeoutError") {
        error = "Connection timed out. Check the URL and try again.";
      } else {
        error = "Could not connect to server. Please check the URL.";
      }
      return false;
    } finally {
      isValidatingServer = false;
    }
  }

  // Send magic link
  async function handleSendMagicLink() {
    if (!email.trim()) {
      error = "Please enter your email address";
      return;
    }

    if (!(await validateServer())) {
      return;
    }

    isSendingMagicLink = true;
    error = null;
    devLink = null;

    try {
      const result = await requestMagicLink(email.trim());
      devLink = result.devLink || null;
      verificationSent = true;

      localStorage.setItem("diaryx_device_name", deviceName.trim() || getDefaultDeviceName());

      startMagicLinkDetection();
      startResendCooldown();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to send magic link";
    } finally {
      isSendingMagicLink = false;
    }
  }

  // Start polling for magic link token in URL
  function startMagicLinkDetection() {
    stopMagicLinkDetection();

    urlCheckInterval = setInterval(async () => {
      const params = new URLSearchParams(window.location.search);
      const token = params.get("token");
      if (token) {
        stopMagicLinkDetection();
        window.history.replaceState({}, "", location.pathname);
        await handleVerifyToken(token);
      }
    }, 1000);
  }

  function stopMagicLinkDetection() {
    if (urlCheckInterval) {
      clearInterval(urlCheckInterval);
      urlCheckInterval = null;
    }
  }

  // Start resend cooldown timer
  function startResendCooldown() {
    resendCooldown = 60;
    if (resendInterval) {
      clearInterval(resendInterval);
    }
    resendInterval = setInterval(() => {
      resendCooldown--;
      if (resendCooldown <= 0) {
        clearInterval(resendInterval!);
        resendInterval = null;
      }
    }, 1000);
  }

  // Verify token (from magic link or dev mode)
  async function handleVerifyToken(token: string) {
    if (!token.trim()) {
      error = "Please enter the verification code";
      return;
    }

    error = null;

    try {
      const savedDeviceName = localStorage.getItem("diaryx_device_name") || undefined;
      await verifyMagicLink(token.trim(), savedDeviceName);

      screen = await handlePostAuth();
    } catch (e) {
      error = e instanceof Error ? e.message : "Verification failed";
    }
  }

  /**
   * Populate the options screen after authentication.
   * Sets syncMode to 'remote' and pre-selects content source.
   */
  async function handlePostAuth(): Promise<Screen> {
    // Guard: if sync is already set up and server has workspaces, skip
    if (isSyncEnabled() && getWorkspaces().length > 0) {
      handleClose();
      onComplete?.();
      return 'options';
    }

    // Free users need Plus to sync — show upgrade screen
    const authState = getAuthState();
    if (authState.tier !== 'plus') {
      return 'upgrade';
    }

    const serverWs = getWorkspaces();
    serverWorkspacesList = serverWs;

    // User just authenticated — set to Remote mode
    syncMode = 'remote';

    // Pre-select the most sensible default
    if (serverWs.length > 0) {
      contentSource = 'existing_workspace';
      selectedSourceIsServer = true;
      const preferredServerWorkspace = (
        authState.activeWorkspaceId
          ? serverWs.find(w => w.id === authState.activeWorkspaceId)
          : null
      ) ?? serverWs[0];
      selectedSourceWorkspaceId = preferredServerWorkspace.id;
      newWorkspaceName = preferredServerWorkspace.name;
    } else if (localWorkspaces.length > 0) {
      contentSource = 'existing_workspace';
      selectedSourceIsServer = false;
      selectedSourceWorkspaceId = localWorkspaces[0].id;
      newWorkspaceName = localWorkspaces[0].name;
    } else {
      contentSource = 'start_fresh';
      newWorkspaceName = getNextLocalWorkspaceName();
    }

    return 'options';
  }

  /**
   * Handle sync mode toggle. Triggers auth flow if switching to Remote
   * without being authenticated.
   */
  function handleSyncModeChange(newMode: string) {
    if (newMode === 'remote' && !isAuthenticated()) {
      syncMode = 'remote';
      screen = 'auth';
      return;
    }

    if (newMode === 'remote' && isAuthenticated()) {
      const auth = getAuthState();
      if (auth.tier !== 'plus') {
        syncMode = 'remote';
        screen = 'upgrade';
        return;
      }
      serverWorkspacesList = getWorkspaces();
    }

    syncMode = newMode as SyncMode;

    // Reset invalid selection when switching to local
    // (local workspaces are hidden in Local mode since they already exist)
    if (syncMode === 'local' && contentSource === 'existing_workspace' && !selectedSourceIsServer) {
      contentSource = 'start_fresh';
      selectedSourceWorkspaceId = null;
      selectedSourceIsServer = false;
      newWorkspaceName = getNextLocalWorkspaceName();
    }
  }

  /**
   * Select a source workspace from the "From existing workspace" picker.
   */
  function selectSourceWorkspace(ws: { id: string; name: string; isServer: boolean }) {
    contentSource = 'existing_workspace';
    selectedSourceWorkspaceId = ws.id;
    selectedSourceIsServer = ws.isServer;
    newWorkspaceName = ws.name;
  }

  /**
   * Get the submit button text based on current syncMode + contentSource.
   */
  function getSubmitButtonText(): string {
    if (syncMode === 'local') {
      switch (contentSource) {
        case 'start_fresh': return 'Create Workspace';
        case 'import_zip': return 'Import Workspace';
        case 'existing_workspace': return 'Download Workspace';
        case 'open_folder': return 'Open Workspace';
      }
    } else {
      switch (contentSource) {
        case 'start_fresh': return 'Create & Sync';
        case 'import_zip': return 'Import & Sync';
        case 'existing_workspace':
          return selectedSourceIsServer ? 'Download & Sync' : 'Upload & Sync';
        case 'open_folder': return 'Open & Sync';
      }
    }
    return 'Create Workspace';
  }

  /**
   * Handle all initialization actions based on syncMode + contentSource.
   */
  async function handleInitialize() {
    if (contentSource === 'existing_workspace' && !selectedSourceWorkspaceId) {
      error = "Please select a workspace";
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

    if (
      (contentSource === 'start_fresh' || contentSource === 'import_zip' || contentSource === 'open_folder')
      && !newWorkspaceName.trim()
    ) {
      error = "Please enter a workspace name";
      return;
    }

    isInitializing = true;
    error = null;
    importProgress = 0;

    try {
      if (syncMode === 'local') {
        switch (contentSource) {
          case 'start_fresh':
            await handleCreateLocalWorkspace();
            break;
          case 'import_zip':
            await handleImportZipLocal();
            break;
          case 'existing_workspace':
            await handleDownloadServerLocal();
            break;
          case 'open_folder':
            await handleOpenFolder();
            break;
        }
      } else {
        switch (contentSource) {
          case 'start_fresh':
            await handleCreateNew();
            break;
          case 'import_zip':
            await handleImportZip();
            break;
          case 'existing_workspace':
            if (selectedSourceIsServer) {
              await handleDownloadServer();
            } else {
              await handleUploadLocal({
                forceCreateServerWorkspace: true,
                localWorkspaceId: selectedSourceWorkspaceId!,
              });
            }
            break;
          case 'open_folder':
            await handleOpenFolderRemote();
            break;
        }
      }
    } catch (e) {
      console.error("[AddWorkspace] Initialization error:", e);
      cleanupSyncSubscriptions();
      if (e instanceof Error) {
        error = e.message || "Unknown error";
      } else if (typeof e === "object" && e !== null) {
        error = JSON.stringify(e);
      } else {
        error = String(e) || "Initialization failed";
      }
    } finally {
      isInitializing = false;
    }
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

  function normalizeWorkspaceName(name: string): string {
    return name.trim().toLowerCase();
  }

  function getPreferredServerWorkspace(): { id: string; name: string } | null {
    const activeWorkspaceId = getAuthState().activeWorkspaceId;
    if (activeWorkspaceId) {
      const active = serverWorkspacesList.find(w => w.id === activeWorkspaceId);
      if (active) {
        return active;
      }
    }

    return serverWorkspacesList[0] ?? null;
  }

  function resolveWorkspaceNameForServerCreate(): string {
    const typedName = newWorkspaceName.trim();
    if (typedName) {
      return typedName;
    }

    const currentWorkspaceId = getCurrentWorkspaceId();
    const currentWorkspace = currentWorkspaceId
      ? getLocalWorkspaces().find(w => w.id === currentWorkspaceId)
      : null;
    const currentWorkspaceName = currentWorkspace?.name?.trim();
    if (currentWorkspaceName) {
      return currentWorkspaceName;
    }

    const firstLocalName = getLocalWorkspaces()[0]?.name?.trim();
    if (firstLocalName) {
      return firstLocalName;
    }

    return getNextLocalWorkspaceName();
  }

  function resolveCreationWorkspaceName(requireServerUnique: boolean): string {
    const workspaceName = newWorkspaceName.trim();
    if (!workspaceName) {
      throw new Error("Please enter a workspace name");
    }

    const normalized = normalizeWorkspaceName(workspaceName);

    if (getLocalWorkspaces().some(ws => normalizeWorkspaceName(ws.name) === normalized)) {
      throw new Error("A local workspace with that name already exists");
    }

    if (
      requireServerUnique
      && serverWorkspacesList.some(ws => normalizeWorkspaceName(ws.name) === normalized)
    ) {
      throw new Error("A synced workspace with that name already exists");
    }

    return workspaceName;
  }

  async function ensureRootIndexForCurrentWorkspace(workspaceName: string): Promise<void> {
    const backend = await getBackend();
    const api = createApi(backend);
    const existingRoot = await findWorkspaceRootPath(api, backend);
    if (existingRoot) {
      return;
    }
    // Use the actual workspace directory from the backend, not "." which
    // resolves to the process CWD on Tauri (wrong directory).
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');
    try {
      await api.createWorkspace(workspaceDir, workspaceName);
    } catch (e) {
      // If the workspace already has a root index, that's fine
      if (e instanceof Error && e.message.includes('already exists')) return;
      throw e;
    }
  }

  async function handleCreateLocalWorkspace() {
    const wsName = resolveCreationWorkspaceName(false);
    const localWs = createLocalWorkspace(wsName, undefined, isTauri() && workspacePath ? workspacePath : undefined);

    await switchWorkspace(localWs.id, localWs.name);
    await ensureRootIndexForCurrentWorkspace(localWs.name);

    toast.success("Local workspace created", {
      description: `"${localWs.name}" is ready on this device.`,
    });

    handleClose();
    onComplete?.();
  }

  /**
   * Open an existing folder as a local workspace.
   */
  async function handleOpenFolder() {
    const wsName = resolveCreationWorkspaceName(false);

    if (isTauri()) {
      if (!selectedFolderPath) throw new Error("No folder selected");
      const localWs = createLocalWorkspace(wsName, undefined, selectedFolderPath);
      await switchWorkspace(localWs.id, localWs.name);
    } else {
      if (!selectedFolderHandle) throw new Error("No folder selected");
      const localWs = createLocalWorkspace(wsName, 'filesystem-access');
      await storeWorkspaceFileSystemHandle(localWs.id, selectedFolderHandle);
      await switchWorkspace(localWs.id, localWs.name);
    }

    await ensureRootIndexForCurrentWorkspace(wsName);

    // Re-initialize CRDT from the filesystem now that the root index exists.
    // switchWorkspace's initializeWorkspaceCrdt ran before ensureRootIndex created
    // the root, so the CRDT may be missing the workspace structure.
    const backend = await getBackend();
    const api = createApi(backend);
    const rootPath = await findWorkspaceRootPath(api, backend);
    if (rootPath) {
      try {
        await api.initializeWorkspaceCrdt(rootPath);
      } catch (e) {
        console.warn('[AddWorkspace] CRDT re-init after open folder:', e);
      }
    }

    toast.success("Workspace opened", {
      description: `"${wsName}" is ready.`,
    });

    handleClose();
    onComplete?.();
  }

  /**
   * Open an existing folder as a workspace and sync it to the server.
   */
  async function handleOpenFolderRemote() {
    const wsName = resolveCreationWorkspaceName(true);

    if (isTauri()) {
      if (!selectedFolderPath) throw new Error("No folder selected");
      const localWs = createLocalWorkspace(wsName, undefined, selectedFolderPath);
      await switchWorkspace(localWs.id, localWs.name);
    } else {
      if (!selectedFolderHandle) throw new Error("No folder selected");
      const localWs = createLocalWorkspace(wsName, 'filesystem-access');
      await storeWorkspaceFileSystemHandle(localWs.id, selectedFolderHandle);
      await switchWorkspace(localWs.id, localWs.name);
    }

    await ensureRootIndexForCurrentWorkspace(wsName);

    // Re-initialize CRDT with the root index (same rationale as handleOpenFolder)
    const backend = await getBackend();
    const api = createApi(backend);
    const rootPath = await findWorkspaceRootPath(api, backend);
    if (rootPath) {
      try {
        await api.initializeWorkspaceCrdt(rootPath);
      } catch (e) {
        console.warn('[AddWorkspace] CRDT re-init after open folder remote:', e);
      }
    }

    const currentLocalId = getCurrentWorkspaceId();
    await handleUploadLocal({
      forceCreateServerWorkspace: true,
      localWorkspaceId: currentLocalId!,
      workspaceNameOverride: wsName,
    });
  }

  /**
   * Download a server workspace to this device (one-time copy, no sync).
   */
  async function handleDownloadServerLocal() {
    const serverWs = serverWorkspacesList.find(w => w.id === selectedSourceWorkspaceId);
    if (!serverWs) throw new Error("No server workspace selected");

    const localWs = createLocalWorkspace(serverWs.name, undefined, isTauri() && workspacePath ? workspacePath : undefined);
    await switchWorkspace(localWs.id, localWs.name);

    const backend = await getBackend();
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    syncStatusText = "Downloading workspace...";
    progressMode = 'bytes';

    try {
      const snapshot = await downloadWorkspaceSnapshot(serverWs.id, true);
      if (snapshot && snapshot.size > 100) {
        const snapshotFile = new File(
          [snapshot],
          `diaryx-snapshot-${serverWs.id}.zip`,
          { type: "application/zip" },
        );

        syncStatusText = "Importing files...";
        const result = await backend.importFromZip(
          snapshotFile,
          workspaceDir,
          (uploaded, total) => {
            importProgress = total > 0 ? Math.round((uploaded / total) * 100) : 0;
            progressDetail = total > 0
              ? `${formatBytes(uploaded)} of ${formatBytes(total)}`
              : null;
          },
        );

        if (result.success && result.files_imported > 0) {
          console.log(`[AddWorkspace] Downloaded ${result.files_imported} files`);
        }
      }
    } catch (e) {
      console.warn("[AddWorkspace] Snapshot download/import error:", e);
    }

    await ensureRootIndexForCurrentWorkspace(serverWs.name);

    toast.success("Workspace downloaded", {
      description: `"${serverWs.name}" is ready on this device.`,
    });

    handleClose();
    onComplete?.();
  }

  /**
   * Import a ZIP file as a local-only workspace (no sync).
   */
  async function handleImportZipLocal() {
    if (!importZipFile) throw new Error("No ZIP file selected");
    const zipFile = importZipFile;

    const wsName = resolveCreationWorkspaceName(false);
    const localWs = createLocalWorkspace(wsName, undefined, isTauri() && workspacePath ? workspacePath : undefined);
    await switchWorkspace(localWs.id, localWs.name);

    const backend = await getBackend();
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    syncStatusText = "Importing ZIP...";
    progressMode = 'bytes';

    const result = await backend.importFromZip(
      zipFile,
      workspaceDir,
      (uploaded, total) => {
        importProgress = total > 0 ? Math.round((uploaded / total) * 100) : 0;
        progressDetail = total > 0
          ? `${formatBytes(uploaded)} of ${formatBytes(total)}`
          : null;
      },
    );

    if (!result.success) {
      throw new Error(result.error || "Failed to import ZIP");
    }

    await ensureRootIndexForCurrentWorkspace(localWs.name);

    toast.success("Import complete", {
      description: `Imported ${result.files_imported} files.`,
    });

    handleClose();
    onComplete?.();
  }

  /**
   * Download a server workspace to this device and enable sync.
   */
  async function handleDownloadServer() {
    const serverWs = serverWorkspacesList.find(w => w.id === selectedSourceWorkspaceId);
    if (!serverWs) throw new Error("No server workspace selected");

    const workspaceId = serverWs.id;

    const backend = await getBackend();
    const api = createApi(backend);
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    let workspacePath: string;
    try {
      workspacePath = await api.findRootIndex(workspaceDir);
    } catch {
      workspacePath = `${workspaceDir}/index.md`;
    }

    // Step 1: Clear local workspace
    syncStatusText = "Preparing workspace...";
    suppressSyncProgress = true;
    await clearLocalWorkspace(api, workspaceDir);

    // Step 2: Tombstone old CRDT entries
    const tombstoned = await markAllCrdtFilesAsDeleted();
    console.log(`[AddWorkspace] Tombstoned ${tombstoned} local CRDT entries`);

    // Step 3: Download and import server snapshot
    syncStatusText = "Downloading workspace...";
    try {
      const snapshot = await downloadWorkspaceSnapshot(workspaceId, true);
      if (snapshot && snapshot.size > 100) {
        const snapshotFile = new File(
          [snapshot],
          `diaryx-snapshot-${workspaceId}.zip`,
          { type: "application/zip" },
        );

        const result = await backend.importFromZip(
          snapshotFile,
          workspaceDir,
          (uploaded, total) => {
            importProgress = total > 0 ? Math.round((uploaded / total) * 100) : 0;
            progressDetail = total > 0
              ? `${formatBytes(uploaded)} of ${formatBytes(total)}`
              : null;
          },
        );

        if (result.success && result.files_imported > 0) {
          console.log(`[AddWorkspace] Downloaded ${result.files_imported} files`);
        }
      }
    } catch (e) {
      console.warn("[AddWorkspace] Snapshot download/import error:", e);
    }

    suppressSyncProgress = false;

    // Step 4: Initialize CRDT from downloaded files
    syncStatusText = "Initializing...";
    try {
      await api.initializeWorkspaceCrdt(workspacePath);
    } catch (e) {
      console.log("[AddWorkspace] CRDT init error (continuing):", e);
    }

    // Step 5: Connect WebSocket
    syncStatusText = "Connecting to sync...";
    await setWorkspaceId(workspaceId);
    const syncServerUrl = getServerUrl() ?? serverUrl;
    await setWorkspaceServer(syncServerUrl);

    // Step 6: Wait for initial metadata sync
    syncStatusText = "Syncing metadata...";
    importProgress = 80;
    const syncResult = await waitForInitialSync(30000);
    if (!syncResult) {
      console.warn("[AddWorkspace] Metadata sync timed out, continuing in background");
    }

    importProgress = 100;

    // Step 7: Register workspace locally & enable sync
    registerWorkspaceLocally(workspaceId, serverWs.name);
    enableSync();

    toast.success("Sync setup complete", {
      description: "Your workspace is now syncing.",
    });

    cleanupSyncSubscriptions();
    handleClose();
    onComplete?.();
  }

  /**
   * Upload local workspace to an existing or new server workspace.
   */
  async function handleUploadLocal(options?: {
    forceCreateServerWorkspace?: boolean;
    localWorkspaceId?: string;
    workspaceNameOverride?: string;
  }) {
    const backend = await getBackend();
    const api = createApi(backend);
    const workspacePath = await findWorkspaceRootPath(api, backend);

    subscribeSyncProgress();

    // Determine workspace name from selected local workspace
    const localWorkspaceId = options?.localWorkspaceId;
    const selectedLocal = localWorkspaceId
      ? getLocalWorkspaces().find(w => w.id === localWorkspaceId)
      : null;
    const workspaceName = (options?.workspaceNameOverride ?? selectedLocal?.name)?.trim();
    if (!workspaceName) {
      throw new Error("Please select a local workspace with a valid name");
    }

    // Use existing server workspace when allowed, otherwise create one.
    let workspaceId: string;
    if (!options?.forceCreateServerWorkspace && serverWorkspacesList.length > 0) {
      const existing = getPreferredServerWorkspace();
      if (!existing) {
        throw new Error("No synced workspace available");
      }
      workspaceId = existing.id;
      console.log(`[AddWorkspace] Uploading to existing server workspace: ${existing.name} (${workspaceId})`);
    } else {
      syncStatusText = "Creating workspace on server...";
      let serverWs;
      try {
        serverWs = await createServerWorkspace(workspaceName);
      } catch (e: any) {
        if (e?.statusCode === 409) {
          throw new Error("A workspace with that name already exists on the server");
        }
        throw e;
      }
      workspaceId = serverWs.id;
    }

    // Upload local workspace to server
    setStageProgress(10, "Preparing snapshot...", "Step 1 of 4");
    suppressSyncProgress = true;
    progressMode = 'files';

    if (workspacePath) {
      const snapshot = await buildWorkspaceSnapshotUploadBlob(
        api,
        workspacePath,
        (progress) => {
          const ratio = progress.totalFiles > 0
            ? progress.completedFiles / progress.totalFiles
            : 0;
          importProgress = Math.max(importProgress, 10 + Math.round(ratio * 16));
          progressDetail = progress.totalFiles > 0
            ? `${progress.completedFiles} of ${progress.totalFiles}`
            : null;
          progressMode = progress.totalFiles > 0 ? "files" : null;
          syncStatusText = "Preparing snapshot...";
        },
      );

      if (snapshot.filesPlanned > 0 && snapshot.blob.size > 0) {
        setStageProgress(28, "Uploading snapshot...", "Step 1 of 4");
        progressMode = 'bytes';

        const result = await uploadWorkspaceSnapshot(
          workspaceId,
          snapshot.blob,
          "replace",
          true,
          (uploadedBytes, totalBytes) => {
            const ratio = totalBytes > 0 ? uploadedBytes / totalBytes : 0;
            importProgress = Math.max(importProgress, 28 + Math.round(ratio * 7));
            progressDetail = totalBytes > 0
              ? `${formatBytes(uploadedBytes)} of ${formatBytes(totalBytes)}`
              : null;
          },
        );

        if (!result) {
          throw new Error("Snapshot upload failed");
        }

        console.log(`[AddWorkspace] Snapshot upload complete (${result.files_imported} files)`);
        if (snapshot.attachmentReadFailures > 0) {
          toast.warning("Some attachments were skipped", {
            description: `${snapshot.attachmentReadFailures} attachment file(s) could not be included in the snapshot.`,
          });
        }
        setStageProgress(35, "Snapshot uploaded", `${result.files_imported} files`);
      } else {
        setStageProgress(35, "Workspace is empty", "0 files");
      }
    } else {
      setStageProgress(35, "No root index found", "Skipping snapshot upload");
    }

    suppressSyncProgress = false;
    setStageProgress(50, "Preparing local workspace...", "Step 2 of 4");
    if (workspacePath) {
      await api.initializeWorkspaceCrdt(workspacePath);
    }
    setStageProgress(65, "Connecting to sync server...", "Step 3 of 4");

    // Connect WebSocket
    await setWorkspaceId(workspaceId);
    const syncServerUrl = getServerUrl() ?? serverUrl;
    await setWorkspaceServer(syncServerUrl);

    // Wait for metadata sync
    setStageProgress(80, "Waiting for metadata sync...", "Step 4 of 4");
    const syncResult = await waitForInitialSync(30000);

    if (!syncResult) {
      console.warn("[AddWorkspace] Metadata sync timed out, continuing in background");
      toast.info("Sync continuing in background", {
        description: "Check the sync indicator in the header for progress.",
      });
    }

    // Proactively sync body content
    syncStatusText = "Uploading file contents...";
    progressMode = 'percent';

    try {
      const allFiles = await getAllFiles();
      const filePaths = Array.from(allFiles.keys());

      if (filePaths.length > 0) {
        let subscriptionsSent = false;

        await proactivelySyncBodies(filePaths, {
          concurrency: 5,
          waitForComplete: true,
          syncTimeout: 120000,
          onProgress: (completed, total) => {
            syncCompleted = completed;
            syncTotal = total;
            if (total > 0) {
              const subscriptionProgress = Math.round((completed / total) * 50);
              importProgress = subscriptionProgress;

              if (completed === total && !subscriptionsSent) {
                subscriptionsSent = true;
                syncStatusText = "Syncing file contents...";
                importProgress = 50;
              }
              progressMode = 'percent';
            }
          }
        });

        importProgress = 100;
      }
    } catch (e) {
      console.warn("[AddWorkspace] Body sync error (continuing anyway):", e);
    }

    if (importProgress < 100) importProgress = 100;

    if (syncResult) {
      toast.success("Sync setup complete", {
        description: "Your workspace is now syncing.",
      });
    }

    registerWorkspaceLocally(workspaceId, workspaceName, localWorkspaceId);
    enableSync();
    cleanupSyncSubscriptions();
    handleClose();
    onComplete?.();
  }

  /**
   * Create a named local workspace with root index, then create a synced
   * workspace from that local content.
   */
  async function handleCreateNew() {
    const workspaceName = resolveCreationWorkspaceName(true);
    const localWs = createLocalWorkspace(workspaceName, undefined, isTauri() && workspacePath ? workspacePath : undefined);

    await switchWorkspace(localWs.id, localWs.name);
    await ensureRootIndexForCurrentWorkspace(localWs.name);

    await handleUploadLocal({
      forceCreateServerWorkspace: true,
      localWorkspaceId: localWs.id,
      workspaceNameOverride: localWs.name,
    });
  }

  /**
   * Import from a ZIP file: upload to server, apply ZIP locally, init CRDT, sync.
   */
  async function handleImportZip() {
    if (!importZipFile) throw new Error("No ZIP file selected");
    const zipFile = importZipFile;

    const backend = await getBackend();
    const api = createApi(backend);

    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    subscribeSyncProgress();

    // Step 1: Create or reuse server workspace
    let workspaceId: string;
    let workspaceName: string;
    if (serverWorkspacesList.length > 0) {
      const existing = getPreferredServerWorkspace();
      if (!existing) {
        throw new Error("No synced workspace available");
      }
      workspaceId = existing.id;
      workspaceName = existing.name;
      console.log(`[AddWorkspace] Importing ZIP to existing server workspace: ${existing.name} (${workspaceId})`);
    } else {
      setStageProgress(5, "Creating workspace on server...");
      let serverWs;
      const nameForServerWorkspace = resolveWorkspaceNameForServerCreate();
      try {
        serverWs = await createServerWorkspace(nameForServerWorkspace);
      } catch (e: any) {
        if (e?.statusCode === 409) {
          throw new Error("A workspace with that name already exists on the server");
        }
        throw e;
      }
      workspaceId = serverWs.id;
      workspaceName = serverWs.name;
    }

    // Step 2: Upload ZIP to server
    setStageProgress(10, "Uploading ZIP to server...");
    suppressSyncProgress = true;
    progressMode = 'bytes';

    const uploadResult = await uploadWorkspaceSnapshot(
      workspaceId,
      zipFile,
      'replace',
      true,
      (uploadedBytes, totalBytes) => {
        const ratio = totalBytes > 0 ? uploadedBytes / totalBytes : 0;
        importProgress = Math.max(importProgress, 10 + Math.round(ratio * 25));
        if (totalBytes > 0 && ratio >= 1) {
          syncStatusText = "Upload complete. Server is importing ZIP...";
          progressDetail = `${formatBytes(totalBytes)} uploaded`;
          progressMode = null;
        } else {
          progressDetail = totalBytes > 0
            ? `${formatBytes(uploadedBytes)} of ${formatBytes(totalBytes)}`
            : null;
          progressMode = 'bytes';
        }
      },
    );
    if (!uploadResult) throw new Error("Upload failed — server returned no result");

    console.log(`[AddWorkspace] Server imported ${uploadResult.files_imported} files from ZIP`);
    setStageProgress(35, "Server import complete", `${uploadResult.files_imported} files`);

    // Step 3: Apply ZIP locally so this device is ready before sync connects.
    setStageProgress(40, "Applying ZIP locally...");
    progressMode = 'bytes';
    const localImportResult = await backend.importFromZip(
      zipFile,
      workspaceDir,
      (uploaded, total) => {
        if (total > 0) {
          const pct = Math.round((uploaded / total) * 100);
          importProgress = Math.max(importProgress, 40 + Math.round(pct * 0.28));
          progressDetail = `${formatBytes(uploaded)} of ${formatBytes(total)}`;
          progressMode = 'bytes';
          syncStatusText = "Applying ZIP locally...";
        }
      },
    );
    if (!localImportResult.success) {
      throw new Error(localImportResult.error || "Failed to apply ZIP locally");
    }

    if (localImportResult.files_imported > 0) {
      console.log(`[AddWorkspace] Applied ${localImportResult.files_imported} files locally`);
    }

    suppressSyncProgress = false;

    // Step 4: Initialize CRDT from downloaded files
    setStageProgress(68, "Initializing workspace...");
    const workspacePath = await findWorkspaceRootPath(api, backend);

    if (workspacePath) {
      try {
        await api.initializeWorkspaceCrdt(workspacePath);
      } catch (e) {
        console.log("[AddWorkspace] CRDT init error (continuing):", e);
      }
    }

    // Step 5: Connect WebSocket
    setStageProgress(78, "Connecting to sync server...");
    await setWorkspaceId(workspaceId);
    const syncServerUrl = getServerUrl() ?? serverUrl;
    await setWorkspaceServer(syncServerUrl);

    // Step 6: Wait for initial metadata sync
    setStageProgress(88, "Syncing metadata...");
    const syncResult = await waitForInitialSync(30000);
    if (!syncResult) {
      console.warn("[AddWorkspace] Metadata sync timed out, continuing in background");
    }

    importProgress = 100;

    // Step 7: Register workspace locally & enable sync
    registerWorkspaceLocally(workspaceId, workspaceName);
    enableSync();

    toast.success("Import complete", {
      description: `Imported ${uploadResult.files_imported} files and sync is now active.`,
    });

    cleanupSyncSubscriptions();
    handleClose();
    onComplete?.();
  }

  /**
   * Subscribe to sync progress/status events for the wizard UI.
   */
  function subscribeSyncProgress() {
    unsubscribeProgress = onSyncProgress((completed, total) => {
      if (suppressSyncProgress) return;
      syncCompleted = completed;
      syncTotal = total;
      if (total > 0) {
        importProgress = Math.max(importProgress, Math.round((completed / total) * 100));
      }
      progressDetail = total > 0 ? `${completed} of ${total}` : null;
      progressMode = total > 0 ? 'files' : null;
    });

    unsubscribeStatus = onSyncStatus((status, statusError) => {
      if (status === 'error' && statusError) {
        console.warn("[AddWorkspace] Sync error:", statusError);
      }
    });
  }

  /**
   * Register workspace in local registry and set as active.
   */
  function registerWorkspaceLocally(
    serverWorkspaceId: string,
    name: string,
    localIdToPromote?: string,
  ) {
    const currentLocalId = localIdToPromote ?? getCurrentWorkspaceId();
    if (currentLocalId && currentLocalId.startsWith('local-')) {
      promoteLocalWorkspace(currentLocalId, serverWorkspaceId);
    } else {
      addLocalWorkspace({ id: serverWorkspaceId, name });
    }

    setCurrentWorkspaceId(serverWorkspaceId);
    setActiveWorkspaceId(serverWorkspaceId);
    localStorage.setItem('diaryx-workspace-name', name);
  }

  // Cleanup sync subscriptions
  function cleanupSyncSubscriptions() {
    if (unsubscribeProgress) {
      unsubscribeProgress();
      unsubscribeProgress = null;
    }
    if (unsubscribeStatus) {
      unsubscribeStatus();
      unsubscribeStatus = null;
    }
    syncStatusText = null;
    syncCompleted = 0;
    syncTotal = 0;
  }

  // Handle dialog close
  function handleClose() {
    stopMagicLinkDetection();
    if (resendInterval) {
      clearInterval(resendInterval);
      resendInterval = null;
    }
    cleanupSyncSubscriptions();
    open = false;
    onOpenChange?.(false);
  }

  // Go back from auth/upgrade to options
  function handleBack() {
    if (screen === 'auth') {
      screen = 'options';
      syncMode = 'local';
      error = null;
    } else if (screen === 'upgrade') {
      screen = 'options';
      error = null;
    }
  }

  // Convert HTTP URL to WebSocket URL
  function toWebSocketUrl(httpUrl: string): string {
    return httpUrl
      .replace(/^https:\/\//, "wss://")
      .replace(/^http:\/\//, "ws://")
      + "/sync2";
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const index = Math.floor(Math.log(bytes) / Math.log(1024));
    const value = bytes / Math.pow(1024, index);
    return `${value.toFixed(value < 10 && index > 0 ? 1 : 0)} ${units[index]}`;
  }

  function setStageProgress(percent: number, status: string, detail?: string) {
    importProgress = Math.max(importProgress, percent);
    syncStatusText = status;
    progressDetail = detail ?? null;
    progressMode = null;
  }

  function collectFilePaths(node: TreeNode, paths: string[]) {
    if (!node.children || node.children.length === 0) {
      paths.push(node.path);
      return;
    }
    for (const child of node.children) {
      collectFilePaths(child, paths);
    }
  }

  async function clearLocalWorkspace(api: ReturnType<typeof createApi>, workspaceDir: string) {
    try {
      const tree = await api.getFilesystemTree(workspaceDir, true);
      const files: string[] = [];
      collectFilePaths(tree, files);
      if (files.length === 0) return;

      console.log(`[AddWorkspace] Clearing ${files.length} local file(s) before download`);
      for (const path of files) {
        try {
          await api.deleteFile(path);
        } catch (e) {
          console.warn(`[AddWorkspace] Failed to delete ${path}:`, e);
        }
      }
    } catch (e) {
      console.warn("[AddWorkspace] Failed to clear local workspace:", e);
    }
  }

  // Cleanup on destroy
  $effect(() => {
    return () => {
      stopMagicLinkDetection();
      if (resendInterval) {
        clearInterval(resendInterval);
      }
      cleanupSyncSubscriptions();
    };
  });
</script>

<Dialog.Root bind:open onOpenChange={(o) => onOpenChange?.(o)}>
  <Dialog.Content class="sm:max-w-[450px]">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        {#if screen === 'auth'}
          <Mail class="size-5" />
          Sign In to Sync
        {:else if screen === 'upgrade'}
          <Settings2 class="size-5" />
          Sync Requires Plus
        {:else if isInitializing}
          <Loader2 class="size-5 animate-spin" />
          Adding Workspace
        {:else}
          <Plus class="size-5" />
          Add Workspace
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if screen === 'auth'}
          {#if verificationSent}
            Check your email and click the sign-in link.
          {:else}
            Enter your email to enable remote sync.
          {/if}
        {:else if screen === 'upgrade'}
          Upgrade your account to enable sync.
        {:else if isInitializing}
          {syncStatusText ?? "Setting up..."}
        {:else if syncMode === 'remote'}
          Create a workspace that syncs across devices.
        {:else}
          Create a workspace on this device.<br/>You can change the name later.
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="py-4 space-y-4">
      <!-- Error message -->
      {#if error}
        <div class="flex items-center gap-2 text-destructive text-sm p-3 bg-destructive/10 rounded-md">
          <AlertCircle class="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      {/if}

      <!-- Screen: Authentication -->
      {#if screen === 'auth'}
        {#if !verificationSent}
          <!-- Email input -->
          <div class="space-y-3">
            <div class="space-y-2">
              <Label for="email" class="text-sm">Email Address</Label>
              <Input
                id="email"
                type="email"
                bind:value={email}
                placeholder="you@example.com"
                disabled={isSendingMagicLink || isValidatingServer}
                onkeydown={(e) => e.key === "Enter" && handleSendMagicLink()}
              />
            </div>

            <!-- Device name -->
            <div class="space-y-2">
              <Label for="device-name" class="text-sm">Device Name</Label>
              <Input
                id="device-name"
                type="text"
                bind:value={deviceName}
                placeholder="My Mac"
                disabled={isSendingMagicLink}
              />
              <p class="text-xs text-muted-foreground">
                A name to identify this device. You can change it later in Settings.
              </p>
            </div>
          </div>

          <!-- Advanced settings (toggle) -->
          <div>
            <Button
              variant="ghost"
              size="sm"
              class="w-full justify-between"
              onclick={() => showAdvanced = !showAdvanced}
            >
              <span>Advanced</span>
              {#if showAdvanced}
                <ChevronUp class="size-4" />
              {:else}
                <ChevronDown class="size-4" />
              {/if}
            </Button>
            {#if showAdvanced}
              <div class="space-y-3 mt-2">
                <div class="space-y-2">
                  <Label for="server-url" class="text-sm">Server URL</Label>
                  <Input
                    id="server-url"
                    type="text"
                    bind:value={serverUrl}
                    placeholder="https://sync.diaryx.org"
                    disabled={isSendingMagicLink || isValidatingServer}
                  />
                </div>
              </div>
            {/if}
          </div>
        {:else}
          <!-- Waiting for verification -->
          <div class="space-y-4">
            {#if devLink}
              <!-- Dev mode: show link directly -->
              <div class="space-y-2 p-3 bg-amber-500/10 rounded-md">
                <p class="text-xs text-amber-700 dark:text-amber-400 font-medium">
                  Development mode: Email not configured
                </p>
                <a
                  href={devLink}
                  class="text-xs text-primary hover:underline flex items-center gap-1 break-all"
                  onclick={(e) => {
                    e.preventDefault();
                    handleVerifyToken(new URL(devLink!).searchParams.get("token") || "");
                  }}
                >
                  <Link class="size-3 shrink-0" />
                  Click here to verify
                </a>
              </div>
            {:else}
              <div class="text-center space-y-2 py-4">
                <Mail class="size-12 mx-auto text-muted-foreground" />
                <p class="text-sm font-medium">
                  Check your email at <span class="text-primary">{email}</span>
                </p>
                <p class="text-xs text-muted-foreground">
                  Click the link in your email to continue.
                </p>
              </div>

              <VerificationCodeInput
                {email}
                onVerified={async () => {
                  verificationSent = false;
                  stopMagicLinkDetection();
                  email = "";
                  screen = await handlePostAuth();
                }}
                onError={(msg) => { error = msg; }}
              />

              <!-- Resend button with cooldown -->
              <div class="flex justify-center">
                <Button
                  variant="outline"
                  size="sm"
                  onclick={handleSendMagicLink}
                  disabled={resendCooldown > 0 || isSendingMagicLink}
                >
                  {#if isSendingMagicLink}
                    <Loader2 class="size-4 mr-2 animate-spin" />
                    Sending...
                  {:else if resendCooldown > 0}
                    Resend in {resendCooldown}s
                  {:else}
                    Resend Email
                  {/if}
                </Button>
              </div>
            {/if}
          </div>
        {/if}
      {/if}

      <!-- Screen: Upgrade required -->
      {#if screen === 'upgrade'}
        <div class="space-y-4 py-2">
          <div class="text-center space-y-2">
            <p class="text-sm text-muted-foreground">
              Multi-device sync, live collaboration, and publishing are Plus features.
            </p>
          </div>
          <Button
            class="w-full"
            onclick={async () => {
              isUpgrading = true;
              error = null;
              try {
                const url = await createCheckoutSession();
                window.location.href = url;
              } catch (e) {
                error = e instanceof Error ? e.message : "Failed to start checkout";
              } finally {
                isUpgrading = false;
              }
            }}
            disabled={isUpgrading}
          >
            {#if isUpgrading}
              <Loader2 class="size-4 mr-2 animate-spin" />
              Loading...
            {:else}
              Upgrade to Plus — $5/month
            {/if}
          </Button>
          <button
            type="button"
            class="w-full text-sm text-muted-foreground hover:text-foreground transition-colors py-1"
            onclick={() => {
              screen = 'options';
              syncMode = 'local';
              contentSource = 'start_fresh';
              if (!newWorkspaceName.trim()) {
                newWorkspaceName = getNextLocalWorkspaceName();
              }
            }}
          >
            Create local workspace instead
          </button>
        </div>
      {/if}

      <!-- Screen: Options -->
      {#if screen === 'options'}
        {#if isInitializing}
          <!-- Progress bar during initialization -->
          <div class="space-y-3 py-4">
            <Progress value={importProgress} class="h-2" />
            <p class="text-xs text-muted-foreground text-center">
              {#if syncStatusText}
                {syncStatusText}
                {#if progressDetail}
                  {#if progressMode === 'bytes'}
                    ({progressDetail})
                  {:else if progressMode === 'files'}
                    ({progressDetail} files)
                  {:else if progressMode === 'percent'}
                    ({importProgress}%)
                  {:else}
                    ({progressDetail})
                  {/if}
                {:else if syncTotal > 0}
                  ({syncCompleted} of {syncTotal} files)
                {/if}
              {:else}
                Initializing workspace...
              {/if}
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
                disabled={nameReadonly}
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

            <!-- Sync Mode Toggle -->
            <div class="space-y-2">
              <Label class="text-sm">Sync Mode</Label>
              <div class="flex rounded-lg bg-muted p-[3px]">
                <button
                  type="button"
                  class="flex-1 rounded-md px-3 py-1.5 text-sm font-medium transition-all flex items-center justify-center gap-1.5
                    {syncMode === 'local'
                      ? 'bg-background shadow-sm text-foreground'
                      : 'text-muted-foreground hover:text-foreground'}"
                  onclick={() => handleSyncModeChange('local')}
                >
                  <HardDrive class="size-3.5" />
                  Local
                </button>
                <button
                  type="button"
                  class="flex-1 rounded-md px-3 py-1.5 text-sm font-medium transition-all flex items-center justify-center gap-1.5
                    {syncMode === 'remote'
                      ? 'bg-background shadow-sm text-foreground'
                      : 'text-muted-foreground hover:text-foreground'}"
                  onclick={() => handleSyncModeChange('remote')}
                >
                  <Cloud class="size-3.5" />
                  Remote
                </button>
              </div>
            </div>

            <!-- Content Source -->
            <div class="space-y-2">
              <!-- From existing workspace -->
              {#if availableSourceWorkspaces.length > 0}
                <button
                  type="button"
                  class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'existing_workspace' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                  onclick={() => {
                    contentSource = 'existing_workspace';
                    if (!selectedSourceWorkspaceId && availableSourceWorkspaces.length > 0) {
                      selectSourceWorkspace(availableSourceWorkspaces[0]);
                    }
                  }}
                >
                  <div class="flex items-start gap-3">
                    <div class="mt-0.5">
                      <Download class="size-5 {contentSource === 'existing_workspace' ? 'text-primary' : 'text-muted-foreground'}" />
                    </div>
                    <div>
                      <div class="font-medium text-sm">From existing workspace</div>
                      <div class="text-xs text-muted-foreground mt-0.5">
                        {#if syncMode === 'local'}
                          Download a one-time copy from the server
                        {:else if selectedSourceWorkspaceId && !selectedSourceIsServer}
                          Upload local workspace to server and sync
                        {:else}
                          Download from server and keep in sync
                        {/if}
                      </div>
                    </div>
                  </div>
                </button>

                {#if contentSource === 'existing_workspace'}
                  <div class="pl-8 space-y-1.5 max-h-32 overflow-y-auto">
                    {#each availableSourceWorkspaces as ws (ws.id)}
                      <button
                        type="button"
                        class="w-full text-left p-2 rounded-md border transition-colors {selectedSourceWorkspaceId === ws.id ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                        onclick={() => selectSourceWorkspace(ws)}
                      >
                        <div class="flex items-center gap-2">
                          {#if ws.isServer}
                            <Cloud class="size-3.5 {selectedSourceWorkspaceId === ws.id ? 'text-primary' : 'text-muted-foreground'}" />
                          {:else}
                            <HardDrive class="size-3.5 {selectedSourceWorkspaceId === ws.id ? 'text-primary' : 'text-muted-foreground'}" />
                          {/if}
                          <span class="text-sm truncate">{ws.name}</span>
                          {#if ws.isServer}
                            <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">server</span>
                          {:else}
                            <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">local</span>
                          {/if}
                        </div>
                      </button>
                    {/each}
                  </div>
                {/if}
              {/if}

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
                    selectedSourceWorkspaceId = null;
                    selectedSourceIsServer = false;
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
                    selectedSourceWorkspaceId = null;
                    selectedSourceIsServer = false;
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
                      selectedSourceWorkspaceId = null;
                      selectedSourceIsServer = false;
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

              <!-- Start fresh -->
              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {contentSource === 'start_fresh' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => {
                  contentSource = 'start_fresh';
                  selectedSourceWorkspaceId = null;
                  selectedSourceIsServer = false;
                  newWorkspaceName = getNextLocalWorkspaceName();
                }}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <Plus class="size-5 {contentSource === 'start_fresh' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">Start fresh</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Create an empty workspace with a root index file
                    </div>
                  </div>
                </div>
              </button>
            </div>
          </div>
        {/if}
      {/if}
    </div>

    <!-- Footer with navigation buttons -->
    <div class="flex justify-between pt-4 border-t">
      <!-- Left side: Back / Change Email -->
      {#if screen === 'auth' && verificationSent && !devLink}
        <Button variant="ghost" size="sm" onclick={() => { verificationSent = false; stopMagicLinkDetection(); }}>
          <ArrowLeft class="size-4 mr-1" />
          Change Email
        </Button>
      {:else if screen === 'auth' && !verificationSent}
        <Button variant="ghost" size="sm" onclick={handleBack}>
          <ArrowLeft class="size-4 mr-1" />
          Back
        </Button>
      {:else if screen === 'upgrade' && !isInitializing}
        <Button variant="ghost" size="sm" onclick={handleBack}>
          <ArrowLeft class="size-4 mr-1" />
          Back
        </Button>
      {:else}
        <div></div>
      {/if}

      <!-- Right side: Action buttons -->
      {#if screen === 'auth'}
        {#if !verificationSent}
          <div class="flex items-center gap-2">
            {#if passkeySupported}
              <Button
                variant="outline"
                onclick={handlePasskeySignIn}
                disabled={isAuthenticatingPasskey || isSendingMagicLink || isValidatingServer}
              >
                {#if isAuthenticatingPasskey}
                  <Loader2 class="size-4 mr-2 animate-spin" />
                  Verifying...
                {:else}
                  <Fingerprint class="size-4 mr-2" />
                  Passkey
                {/if}
              </Button>
            {/if}
            <Button onclick={handleSendMagicLink} disabled={isSendingMagicLink || isValidatingServer || !email.trim()}>
              {#if isSendingMagicLink || isValidatingServer}
                <Loader2 class="size-4 mr-2 animate-spin" />
                {isValidatingServer ? 'Connecting...' : 'Sending...'}
              {:else}
                <Mail class="size-4 mr-2" />
                Send Sign-in Link
              {/if}
            </Button>
          </div>
        {:else if devLink}
          <div></div>
        {:else}
          <div class="flex items-center gap-2 text-muted-foreground text-sm">
            <Loader2 class="size-4 animate-spin" />
            Waiting for verification...
          </div>
        {/if}
      {:else if screen === 'options' && !isInitializing}
        <Button onclick={handleInitialize}>
          {getSubmitButtonText()}
          {#if syncMode === 'remote'}
            <ArrowRight class="size-4 ml-1" />
          {/if}
        </Button>
      {:else}
        <div></div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
