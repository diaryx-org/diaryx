<script lang="ts">
  import { proxyFetch } from "$lib/backend/proxyFetch";
  /**
   * SyncSetupWizard - Unified sync setup wizard
   *
   * Screens:
   * 1. Sign In - Email + auth (server URL in Advanced dropdown)
   * 2. Options  - Context-aware picker (no auto-download):
   *    - Download server workspace (shown when server has workspaces)
   *    - Upload local workspace (shown when local workspaces exist)
   *    - Create new workspace (always shown)
   *    - Create local workspace (always shown)
   *
   * Options are intelligently pre-selected based on what exists.
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
    getLocalWorkspace,
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
  } from "@lucide/svelte";
  import VerificationCodeInput from "$lib/components/VerificationCodeInput.svelte";
  import { toast } from "svelte-sonner";
  import { getBackend, createApi } from "./backend";
  import type { TreeNode } from "$lib/backend/interface";
  import {
    buildWorkspaceSnapshotUploadBlob,
    findWorkspaceRootPath,
  } from "$lib/settings/workspaceSnapshotUpload";
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
  let screen = $state<Screen>('auth');

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

  // Options screen state — unified picker
  type PostAuthAction = 'upload_local' | 'download_server' | 'create_new' | 'import_zip' | 'create_local';
  let postAuthAction = $state<PostAuthAction>('upload_local');
  let selectedLocalWorkspaceId = $state<string | null>(null);
  let selectedServerWorkspaceId = $state<string | null>(null);
  let newWorkspaceName = $state("");

  // Import from ZIP state
  let importZipFile = $state<File | null>(null);
  let importZipFileInput = $state<HTMLInputElement | null>(null);

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

  // Skip auth screen if user is already signed in (e.g. opened from Sync settings)
  $effect(() => {
    if (open && isAuthenticated() && screen === 'auth') {
      handlePostAuth().then((s) => { screen = s; });
    }
  });

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
    const backend = await getBackend();
    const api = createApi(backend);

    let url = await api.normalizeServerUrl(serverUrl);
    if (!url) {
      error = "Please enter a server URL";
      return false;
    }
    serverUrl = url;

    isValidatingServer = true;
    error = null;

    try {
      const response = await proxyFetch(`${url}/health`, {
        method: "GET",
        timeout_ms: 5000,
      });

      if (!response.ok) {
        throw new Error("Server returned an error");
      }

      setServerUrl(url);
      collaborationStore.setServerUrl(await api.toWebSocketSyncUrl(url));
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
   * Never auto-starts initialization — always shows the options screen.
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
    const localWs = getLocalWorkspaces();

    serverWorkspacesList = serverWs;

    // Always populate local workspace selection (needed if user picks "upload")
    selectedLocalWorkspaceId = getCurrentWorkspaceId() ?? localWs[0]?.id ?? null;

    // Pre-select the most sensible default
    if (serverWs.length > 0) {
      // Server has workspaces — pre-select download
      postAuthAction = 'download_server';
      const preferredServerWorkspace = (
        authState.activeWorkspaceId
          ? serverWs.find(w => w.id === authState.activeWorkspaceId)
          : null
      ) ?? serverWs[0];
      selectedServerWorkspaceId = preferredServerWorkspace.id;
    } else if (localWs.length > 0) {
      // No server workspaces, but local ones exist — pre-select upload
      postAuthAction = 'upload_local';
    } else {
      // Nothing exists — default to creating a new local workspace.
      postAuthAction = 'create_local';
    }

    // Default name for create-local/create-synced actions.
    newWorkspaceName = getNextLocalWorkspaceName();

    return 'options';
  }

  /**
   * Handle all initialization actions based on user selection.
   */
  async function handleInitialize() {
    if (postAuthAction === 'upload_local' && !selectedLocalWorkspaceId) {
      error = "Please select a workspace to upload";
      return;
    }

    if (postAuthAction === 'download_server' && !selectedServerWorkspaceId) {
      error = "Please select a workspace to download";
      return;
    }

    if (postAuthAction === 'import_zip' && !importZipFile) {
      error = "Please select a ZIP file to import";
      return;
    }

    if (
      (postAuthAction === 'create_local' || postAuthAction === 'create_new')
      && !newWorkspaceName.trim()
    ) {
      error = "Please enter a workspace name";
      return;
    }

    isInitializing = true;
    error = null;
    importProgress = 0;

    try {
      if (postAuthAction === 'create_local') {
        await handleCreateLocalWorkspace();
        return;
      } else if (postAuthAction === 'download_server') {
        await handleDownloadServer();
      } else if (postAuthAction === 'upload_local') {
        await handleUploadLocal();
      } else if (postAuthAction === 'import_zip') {
        await handleImportZip();
      } else {
        await handleCreateNew();
      }
    } catch (e) {
      console.error("[SyncWizard] Initialization error:", e);
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

  function getPreferredServerWorkspace(): { id: string; name: string } | null {
    if (selectedServerWorkspaceId) {
      const selected = serverWorkspacesList.find(w => w.id === selectedServerWorkspaceId);
      if (selected) {
        return selected;
      }
    }

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

    const selectedLocalWorkspace = selectedLocalWorkspaceId
      ? getLocalWorkspaces().find(w => w.id === selectedLocalWorkspaceId)
      : null;
    const selectedLocalName = selectedLocalWorkspace?.name?.trim();
    if (selectedLocalName) {
      return selectedLocalName;
    }

    const firstLocalName = getLocalWorkspaces()[0]?.name?.trim();
    if (firstLocalName) {
      return firstLocalName;
    }

    return getNextLocalWorkspaceName();
  }

  async function resolveCreationWorkspaceName(requireServerUnique: boolean): Promise<string> {
    const backend = await getBackend();
    const api = createApi(backend);
    const localNames = getLocalWorkspaces().map(ws => ws.name);
    const serverNames = requireServerUnique ? serverWorkspacesList.map(ws => ws.name) : undefined;
    return api.validateWorkspaceName(newWorkspaceName, localNames, serverNames);
  }

  async function ensureRootIndexForCurrentWorkspace(workspaceName: string): Promise<void> {
    const backend = await getBackend();
    const api = createApi(backend);
    const existingRoot = await findWorkspaceRootPath(api, backend);
    if (existingRoot) {
      return;
    }
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');
    try {
      await api.createWorkspace(workspaceDir, workspaceName);
    } catch (e) {
      if (e instanceof Error && e.message.includes('already exists')) return;
      throw e;
    }
  }

  async function handleCreateLocalWorkspace() {
    const wsName = await resolveCreationWorkspaceName(false);
    const localWs = createLocalWorkspace(wsName);

    await switchWorkspace(localWs.id, localWs.name);
    await ensureRootIndexForCurrentWorkspace(localWs.name);

    toast.success("Local workspace created", {
      description: `"${localWs.name}" is ready on this device.`,
    });

    handleClose();
    onComplete?.();
  }

  /**
   * Download a server workspace to this device.
   */
  async function handleDownloadServer() {
    const serverWs = serverWorkspacesList.find(w => w.id === selectedServerWorkspaceId);
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
    console.log(`[SyncWizard] Tombstoned ${tombstoned} local CRDT entries`);

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
          console.log(`[SyncWizard] Downloaded ${result.files_imported} files`);
        }
      }
    } catch (e) {
      console.warn("[SyncWizard] Snapshot download/import error:", e);
    }

    suppressSyncProgress = false;

    // Step 4: Initialize CRDT from downloaded files
    syncStatusText = "Initializing...";
    try {
      await api.initializeWorkspaceCrdt(workspacePath);
    } catch (e) {
      console.log("[SyncWizard] CRDT init error (continuing):", e);
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
      console.warn("[SyncWizard] Metadata sync timed out, continuing in background");
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
    const localWorkspaceId = options?.localWorkspaceId ?? selectedLocalWorkspaceId ?? undefined;
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
      console.log(`[SyncWizard] Uploading to existing server workspace: ${existing.name} (${workspaceId})`);
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

        console.log(`[SyncWizard] Snapshot upload complete (${result.files_imported} files)`);
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
      console.warn("[SyncWizard] Metadata sync timed out, continuing in background");
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
      console.warn("[SyncWizard] Body sync error (continuing anyway):", e);
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
    const workspaceName = await resolveCreationWorkspaceName(true);
    const localWs = createLocalWorkspace(workspaceName);

    await switchWorkspace(localWs.id, localWs.name);
    await ensureRootIndexForCurrentWorkspace(localWs.name);

    selectedLocalWorkspaceId = localWs.id;
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
      console.log(`[SyncWizard] Importing ZIP to existing server workspace: ${existing.name} (${workspaceId})`);
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
      importZipFile,
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

    console.log(`[SyncWizard] Server imported ${uploadResult.files_imported} files from ZIP`);
    setStageProgress(35, "Server import complete", `${uploadResult.files_imported} files`);

    // Step 3: Apply ZIP locally so this device is ready before sync connects.
    setStageProgress(40, "Applying ZIP locally...");
    progressMode = 'bytes';
    const localImportResult = await backend.importFromZip(
      importZipFile,
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
      console.log(`[SyncWizard] Applied ${localImportResult.files_imported} files locally`);
    }

    suppressSyncProgress = false;

    // Step 4: Initialize CRDT from downloaded files
    setStageProgress(68, "Initializing workspace...");
    const workspacePath = await findWorkspaceRootPath(api, backend);

    if (workspacePath) {
      try {
        await api.initializeWorkspaceCrdt(workspacePath);
      } catch (e) {
        console.log("[SyncWizard] CRDT init error (continuing):", e);
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
      console.warn("[SyncWizard] Metadata sync timed out, continuing in background");
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
        console.warn("[SyncWizard] Sync error:", statusError);
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
      // Preserve the filesystem path from the current workspace (Tauri).
      // Without this, the new workspace entry has no path and the Tauri backend
      // falls back to the default workspace directory on next startup, which may
      // point to a different workspace's files.
      const currentWs = currentLocalId ? getLocalWorkspace(currentLocalId) : null;
      addLocalWorkspace({ id: serverWorkspaceId, name, path: currentWs?.path });
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

  // Go back to auth screen
  function handleBack() {
    if (screen === 'options' || screen === 'upgrade') {
      screen = 'auth';
      error = null;
    }
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

      console.log(`[SyncWizard] Clearing ${files.length} local file(s) before download`);
      for (const path of files) {
        try {
          await api.deleteFile(path);
        } catch (e) {
          console.warn(`[SyncWizard] Failed to delete ${path}:`, e);
        }
      }
    } catch (e) {
      console.warn("[SyncWizard] Failed to clear local workspace:", e);
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
          Setting Up Sync
        {:else}
          <Settings2 class="size-5" />
          Set Up Sync
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if screen === 'auth'}
          {#if verificationSent}
            Check your email and click the sign-in link.
          {:else}
            Enter your email to sync across devices.
          {/if}
        {:else if screen === 'upgrade'}
          Upgrade your account to enable sync.
        {:else if isInitializing}
          {syncStatusText ?? "Setting up..."}
        {:else if serverWorkspacesList.length > 0 && localWorkspaces.length > 0}
          Your workspace was found on the server. Download it, or upload your local data instead.
        {:else if serverWorkspacesList.length > 0}
          Your workspace was found on the server.
        {:else if localWorkspaces.length > 0}
          No data found on the server. Upload your local workspace or start fresh.
        {:else}
          You're signed in. Set up sync now, or create a local-only workspace.
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

      <!-- Screen 1: Authentication -->
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
              postAuthAction = 'create_local';
              if (!newWorkspaceName.trim()) {
                newWorkspaceName = getNextLocalWorkspaceName();
              }
            }}
          >
            Create local workspace instead
          </button>
        </div>
      {/if}

      <!-- Screen 2: Options -->
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
          <!-- Unified workspace picker -->
          <div class="space-y-3">
            <!-- Server workspaces (download) -->
            {#if serverWorkspacesList.length > 0}
              <div class="space-y-2">
                <p class="text-xs font-medium text-muted-foreground">Download from server</p>
                <div class="space-y-1.5 max-h-32 overflow-y-auto">
                  {#each serverWorkspacesList as ws (ws.id)}
                    <button
                      type="button"
                      class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'download_server' && selectedServerWorkspaceId === ws.id ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                      onclick={() => { postAuthAction = 'download_server'; selectedServerWorkspaceId = ws.id; }}
                    >
                      <div class="flex items-start gap-3">
                        <div class="mt-0.5">
                          <Download class="size-5 {postAuthAction === 'download_server' && selectedServerWorkspaceId === ws.id ? 'text-primary' : 'text-muted-foreground'}" />
                        </div>
                        <div>
                          <div class="font-medium text-sm">{ws.name}</div>
                          <div class="text-xs text-muted-foreground mt-0.5">
                            Download from server to this device
                          </div>
                        </div>
                      </div>
                    </button>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Upload local workspace -->
            {#if localWorkspaces.length > 0}
              <div class="space-y-2">
                {#if serverWorkspacesList.length > 0}
                  <p class="text-xs font-medium text-muted-foreground">Or copy from this device</p>
                {/if}

                {#if localWorkspaces.length === 1}
                  <!-- Single local workspace — show as one card -->
                  <button
                    type="button"
                    class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'upload_local' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                    onclick={() => { postAuthAction = 'upload_local'; selectedLocalWorkspaceId = localWorkspaces[0].id; }}
                  >
                    <div class="flex items-start gap-3">
                      <div class="mt-0.5">
                        <Upload class="size-5 {postAuthAction === 'upload_local' ? 'text-primary' : 'text-muted-foreground'}" />
                      </div>
                      <div>
                        <div class="font-medium text-sm">Create synced workspace from "{localWorkspaces[0].name}"</div>
                        <div class="text-xs text-muted-foreground mt-0.5">
                          Copy this local workspace to the server and start syncing (local files stay on this device)
                        </div>
                      </div>
                    </div>
                  </button>
                {:else}
                  <!-- Multiple local workspaces — expandable picker -->
                  <button
                    type="button"
                    class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'upload_local' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                    onclick={() => { postAuthAction = 'upload_local'; }}
                  >
                    <div class="flex items-start gap-3">
                      <div class="mt-0.5">
                        <Upload class="size-5 {postAuthAction === 'upload_local' ? 'text-primary' : 'text-muted-foreground'}" />
                      </div>
                      <div>
                        <div class="font-medium text-sm">
                          Create synced workspace from local workspace
                        </div>
                        <div class="text-xs text-muted-foreground mt-0.5">
                          {#if postAuthAction === 'upload_local' && selectedLocalWorkspaceId}
                            Copy "{localWorkspaces.find(w => w.id === selectedLocalWorkspaceId)?.name ?? 'workspace'}" to the server and start syncing
                          {:else}
                            Choose which local workspace to copy to the server
                          {/if}
                        </div>
                      </div>
                    </div>
                  </button>

                  {#if postAuthAction === 'upload_local'}
                    <div class="pl-8 space-y-1.5 max-h-32 overflow-y-auto">
                      {#each localWorkspaces as ws (ws.id)}
                        <button
                          type="button"
                          class="w-full text-left p-2 rounded-md border transition-colors {selectedLocalWorkspaceId === ws.id ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                          onclick={() => { selectedLocalWorkspaceId = ws.id; }}
                        >
                          <div class="flex items-center gap-2">
                            <HardDrive class="size-3.5 {selectedLocalWorkspaceId === ws.id ? 'text-primary' : 'text-muted-foreground'}" />
                            <span class="text-sm truncate">{ws.name}</span>
                          </div>
                        </button>
                      {/each}
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}

            <!-- Create new workspace -->
            <button
              type="button"
              class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'create_new' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
              onclick={() => {
                postAuthAction = 'create_new';
                if (!newWorkspaceName.trim()) {
                  newWorkspaceName = getNextLocalWorkspaceName();
                }
              }}
            >
              <div class="flex items-start gap-3">
                <div class="mt-0.5">
                  <Plus class="size-5 {postAuthAction === 'create_new' ? 'text-primary' : 'text-muted-foreground'}" />
                </div>
                <div>
                  <div class="font-medium text-sm">Create new synced workspace</div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    Create a named synced workspace with a root index file
                  </div>
                </div>
              </div>
            </button>

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
                  postAuthAction = 'import_zip';
                }
                input.value = "";
              }}
            />
            <button
              type="button"
              class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'import_zip' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
              onclick={() => {
                if (importZipFile) {
                  postAuthAction = 'import_zip';
                } else {
                  importZipFileInput?.click();
                }
              }}
            >
              <div class="flex items-start gap-3">
                <div class="mt-0.5">
                  <Upload class="size-5 {postAuthAction === 'import_zip' ? 'text-primary' : 'text-muted-foreground'}" />
                </div>
                <div>
                  <div class="font-medium text-sm">
                    {#if importZipFile}
                      Import "{importZipFile.name}"
                    {:else}
                      Import from ZIP backup
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
                      >Change file</span> — Upload this ZIP to the server and sync
                    {:else}
                      Upload a ZIP export to the server and sync to this device
                    {/if}
                  </div>
                </div>
              </div>
            </button>

            <!-- Create local workspace -->
            <button
              type="button"
              class="w-full text-left p-3 rounded-lg border-2 transition-colors {postAuthAction === 'create_local' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
              onclick={() => {
                postAuthAction = 'create_local';
                if (!newWorkspaceName.trim()) {
                  newWorkspaceName = getNextLocalWorkspaceName();
                }
              }}
            >
              <div class="flex items-start gap-3">
                <div class="mt-0.5">
                  <HardDrive class="size-5 {postAuthAction === 'create_local' ? 'text-primary' : 'text-muted-foreground'}" />
                </div>
                <div>
                  <div class="font-medium text-sm">Create local workspace</div>
                  <div class="text-xs text-muted-foreground mt-0.5">
                    Create a named local workspace with a root index file
                  </div>
                </div>
              </div>
            </button>

            {#if postAuthAction === 'create_new' || postAuthAction === 'create_local'}
              <div class="pl-8 space-y-1.5">
                <Label for="new-workspace-name" class="text-xs text-muted-foreground">
                  Workspace Name
                </Label>
                <Input
                  id="new-workspace-name"
                  bind:value={newWorkspaceName}
                  placeholder="Workspace name"
                />
              </div>
            {/if}

          </div>
        {/if}
      {/if}
    </div>

    <!-- Footer with navigation buttons -->
    <div class="flex justify-between pt-4 border-t">
      {#if (screen === 'options' || screen === 'upgrade') && !isInitializing}
        <Button variant="ghost" size="sm" onclick={handleBack}>
          <ArrowLeft class="size-4 mr-1" />
          Back
        </Button>
      {:else if screen === 'auth' && verificationSent && !devLink}
        <Button variant="ghost" size="sm" onclick={() => { verificationSent = false; stopMagicLinkDetection(); }}>
          <ArrowLeft class="size-4 mr-1" />
          Change Email
        </Button>
      {:else}
        <div></div>
      {/if}

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
      {:else if !isInitializing}
        <Button onclick={handleInitialize}>
          {#if postAuthAction === 'create_local'}
            Create Local Workspace
          {:else if postAuthAction === 'create_new'}
            Create Synced Workspace
          {:else}
            Start Syncing
            <ArrowRight class="size-4 ml-1" />
          {/if}
        </Button>
      {:else}
        <div></div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
