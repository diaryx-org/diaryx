<script lang="ts">
  /**
   * SyncSetupWizard - Streamlined 2-screen wizard for sync setup
   *
   * Screens:
   * 1. Sign In - Email + auth (server URL in Advanced dropdown)
   * 2. Initialize Workspace - Data options based on server data status
   *
   * After initialization, the wizard closes and sync progress shows in SyncStatusIndicator.
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
    checkUserHasData,
    downloadWorkspaceSnapshot,
    uploadWorkspaceSnapshot,
    isAuthenticated,
    enableSync,
  } from "$lib/auth";
  import {
    Mail,
    Link,
    Loader2,
    AlertCircle,
    ArrowRight,
    ArrowLeft,
    ChevronDown,
    ChevronUp,
    Download,
    RefreshCw,
    Upload,
    CloudDownload,
    Merge,
    Settings2,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import { getBackend, createApi } from "./backend";
  import type { TreeNode } from "$lib/backend/interface";
  import {
    waitForInitialSync,
    onSyncProgress,
    onSyncStatus,
    setWorkspaceServer,
    setWorkspaceId,
    getAllFiles,
    proactivelySyncBodies,
    markAllCrdtFilesAsDeleted,
    setFreshFromServerLoad,
    discardQueuedLocalSyncUpdates,
  } from "$lib/crdt/workspaceCrdtBridge";
  import { getDefaultWorkspace, getServerUrl } from "$lib/auth";

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

  // Screen tracking (2 screens instead of 5 steps)
  type Screen = 'auth' | 'options';
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

  // Options screen state
  let userHasServerData = $state<boolean | null>(null);
  let serverFileCount = $state(0);
  type InitMode = 'load_server' | 'merge' | 'sync_local' | 'import';
  let initMode = $state<InitMode | null>(null);

  // Loading states
  let isValidatingServer = $state(false);
  let isSendingMagicLink = $state(false);
  let isInitializing = $state(false);
  let isDownloadingBackup = $state(false);
  let importProgress = $state(0);
  let isCheckingServerData = $state(false);
  let progressDetail = $state<string | null>(null);
  let suppressSyncProgress = $state(false);
  let progressMode = $state<'bytes' | 'files' | 'percent' | null>(null);

  // File input for import
  let fileInputRef: HTMLInputElement | null = $state(null);
  let selectedFile: File | null = $state(null);

  // Sync progress tracking
  let syncStatusText = $state<string | null>(null);
  let syncCompleted = $state(0);
  let syncTotal = $state(0);
  let unsubscribeProgress: (() => void) | null = null;
  let unsubscribeStatus: (() => void) | null = null;

  // Error state
  let error = $state<string | null>(null);

  // Magic link URL polling interval
  let urlCheckInterval: ReturnType<typeof setInterval> | null = null;
  let resendInterval: ReturnType<typeof setInterval> | null = null;

  // Skip auth screen if user is already signed in (e.g. opened from Sync settings)
  $effect(() => {
    if (open && isAuthenticated() && screen === 'auth') {
      checkServerData().then(() => { screen = 'options'; });
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

  // Validate and apply server URL
  async function validateServer(): Promise<boolean> {
    let url = serverUrl.trim();
    if (!url) {
      error = "Please enter a server URL";
      return false;
    }

    // Ensure proper protocol
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      url = "https://" + url;
      serverUrl = url;
    }

    isValidatingServer = true;
    error = null;

    try {
      // Validate by making a test request
      const response = await fetch(`${url}/health`, {
        method: "GET",
        signal: AbortSignal.timeout(5000),
      });

      if (!response.ok) {
        throw new Error("Server returned an error");
      }

      // Apply the server URL
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

    // Validate server first
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

      // Save device name
      localStorage.setItem("diaryx_device_name", deviceName.trim() || getDefaultDeviceName());

      // Start magic link detection
      startMagicLinkDetection();

      // Start resend cooldown
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
        // Clean up URL
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
      await verifyMagicLink(token.trim());

      // Check if user has server data
      await checkServerData();

      // Move to options screen
      screen = 'options';
    } catch (e) {
      error = e instanceof Error ? e.message : "Verification failed";
    }
  }

  // Check if user has data on server
  async function checkServerData() {
    isCheckingServerData = true;
    try {
      const result = await checkUserHasData();
      if (result) {
        userHasServerData = result.has_data;
        serverFileCount = result.file_count;

        // Pre-select default option based on server data
        initMode = result.has_data ? 'load_server' : 'sync_local';
      } else {
        userHasServerData = false;
        serverFileCount = 0;
        initMode = 'sync_local';
      }
    } catch (e) {
      console.error('[SyncWizard] Failed to check server data:', e);
      userHasServerData = false;
      serverFileCount = 0;
      initMode = 'sync_local';
    } finally {
      isCheckingServerData = false;
    }
  }

  // Download backup
  async function handleDownloadBackup() {
    isDownloadingBackup = true;

    try {
      const JSZip = (await import("jszip")).default;
      const zip = new JSZip();
      const backend = await getBackend();
      const api = createApi(backend);
      const workspacePath = backend.getWorkspacePath();

      // Export markdown files
      const files = await api.exportToMemory(workspacePath, "*");
      for (const file of files) {
        zip.file(file.path, file.content);
      }

      // Export binary attachments
      const binaries = await api.exportBinaryAttachments(workspacePath, "*");
      for (const info of binaries) {
        try {
          const data = await api.readBinary(info.source_path);
          zip.file(info.relative_path, data, { binary: true });
        } catch (e) {
          console.warn(`[SyncWizard] Failed to read binary ${info.source_path}:`, e);
        }
      }

      // Generate and download
      const blob = await zip.generateAsync({ type: "blob" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `diaryx-backup-${new Date().toISOString().slice(0,10)}.zip`;
      a.click();
      URL.revokeObjectURL(a.href);

      toast.success("Backup downloaded");
    } catch (e) {
      console.error('[SyncWizard] Backup download failed:', e);
      toast.error("Failed to download backup");
    } finally {
      isDownloadingBackup = false;
    }
  }

  // Trigger file input click
  function triggerFileInput() {
    fileInputRef?.click();
  }

  // Handle file selection for import
  async function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    selectedFile = file;
    initMode = 'import';
    if (input) input.value = "";
  }

  // Initialize and start syncing
  async function handleInitialize() {
    if (!initMode) {
      error = "Please select an initialization option";
      return;
    }

    if (initMode === 'import' && !selectedFile) {
      error = "Please select a zip file to import";
      return;
    }

    isInitializing = true;
    error = null;
    importProgress = 0;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const defaultWorkspace = getDefaultWorkspace();
      const workspaceId = defaultWorkspace?.id ?? null;
      let snapshotUploaded = false;

      // Resolve workspace path the same way App.svelte does
      const workspaceDir = backend.getWorkspacePath()
        .replace(/\/index\.md$/, '')
        .replace(/\/README\.md$/, '');

      let workspacePath: string;
      try {
        workspacePath = await api.findRootIndex(workspaceDir);
        console.log("[SyncWizard] Found root index at:", workspacePath);
      } catch (e) {
        console.warn("[SyncWizard] Could not find root index:", e);
        workspacePath = `${workspaceDir}/index.md`;
      }

      // Subscribe to sync progress for real-time updates
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

      switch (initMode) {
        case 'load_server':
          // Download server snapshot and replace local content
          console.log("[SyncWizard] Loading from server");
          syncStatusText = "Preparing workspace...";
          suppressSyncProgress = true;
          progressDetail = null;
          progressMode = 'bytes';

          // Step 1: Clear local workspace files from disk/OPFS
          await clearLocalWorkspace(api, workspaceDir);

          // Step 2: Mark all existing CRDT entries as deleted (tombstone)
          // This ensures local entries don't persist after sync
          const tombstoned = await markAllCrdtFilesAsDeleted();
          console.log(`[SyncWizard] Tombstoned ${tombstoned} local CRDT entries`);

          // Step 3: Download and import server snapshot
          // This happens BEFORE WebSocket connects, but AFTER clearing local state
          // The test waits 5s after clientA sync to ensure server has files
          syncStatusText = "Downloading snapshot...";
          if (workspaceId) {
            try {
              const snapshot = await downloadWorkspaceSnapshot(workspaceId, true);
              console.log("[SyncWizard] Snapshot download result:", snapshot ? `${snapshot.size} bytes` : "null");

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
                    progressMode = 'bytes';
                  },
                );

                if (result.success && result.files_imported > 0) {
                  console.log(`[SyncWizard] Snapshot import complete: ${result.files_imported} files`);
                  progressDetail = `${result.files_imported} files`;
                  progressMode = 'files';
                } else if (result.success) {
                  console.warn("[SyncWizard] Snapshot downloaded but no files imported");
                } else {
                  console.warn("[SyncWizard] Snapshot import failed:", result.error);
                }
              } else {
                console.log("[SyncWizard] Snapshot empty or unavailable");
              }
            } catch (e) {
              console.warn("[SyncWizard] Snapshot download/import error:", e);
            }
          }

          suppressSyncProgress = false;
          progressDetail = null;
          progressMode = null;

          // Step 4: Initialize CRDT from the downloaded files
          // If snapshot was imported, this populates CRDT from disk
          // If snapshot wasn't available, this may fail (empty disk) - WebSocket sync will handle it
          syncStatusText = "Initializing workspace...";
          try {
            await api.initializeWorkspaceCrdt(workspacePath);
            console.log("[SyncWizard] CRDT initialized from downloaded files");
          } catch (e) {
            console.log("[SyncWizard] CRDT init error (continuing anyway):", e);
          }
          break;

        case 'merge':
          // Initialize with local files, sync will merge
          console.log("[SyncWizard] Merging local and server data");
          syncStatusText = "Syncing files...";
          await api.initializeWorkspaceCrdt(workspacePath);
          break;

        case 'sync_local':
          // Initialize with local files, upload to server
          console.log("[SyncWizard] Syncing local content to server");
          setStageProgress(10, "Uploading snapshot...", "Step 1 of 4");
          suppressSyncProgress = true;
          progressDetail = null;
          progressMode = 'bytes';

          if (workspaceId && userHasServerData === false) {
            try {
              const JSZip = (await import("jszip")).default;
              const zip = new JSZip();

              const files = await api.exportToMemory(workspacePath, "*");
              for (const file of files) {
                zip.file(file.path, file.content);
              }

              const binaries = await api.exportBinaryAttachments(workspacePath, "*");
              let attachmentReadFailures = 0;
              for (const info of binaries) {
                try {
                  const data = await api.readBinary(info.source_path);
                  zip.file(info.relative_path, data, { binary: true });
                } catch (e) {
                  attachmentReadFailures += 1;
                  console.warn(
                    `[SyncWizard] Failed to read binary ${info.source_path}:`,
                    e,
                  );
                }
              }

              const blob = await zip.generateAsync({ type: "blob" });
              const result = await uploadWorkspaceSnapshot(
                workspaceId,
                blob,
                "replace",
                true,
              );

              if (result) {
                snapshotUploaded = true;
                console.log(
                  `[SyncWizard] Snapshot upload complete (${result.files_imported} files)`
                );
                if (attachmentReadFailures > 0) {
                  toast.warning("Some attachments were skipped", {
                    description: `${attachmentReadFailures} attachment file(s) could not be included in the snapshot.`,
                  });
                }
                setStageProgress(35, "Snapshot uploaded", `${result.files_imported} files`);
              }
            } catch (e) {
              console.warn("[SyncWizard] Snapshot upload failed:", e);
            }
          }

          suppressSyncProgress = false;
          setStageProgress(50, "Preparing local workspace...", "Step 2 of 4");
          await api.initializeWorkspaceCrdt(workspacePath);
          setStageProgress(65, "Connecting to sync server...", "Step 3 of 4");
          break;

        case 'import':
          // Import from zip file
          console.log("[SyncWizard] Importing from zip file");
          syncStatusText = "Importing files...";
          if (selectedFile) {
            const result = await backend.importFromZip(
              selectedFile,
              undefined,
              (uploaded, total) => {
                importProgress = Math.round((uploaded / total) * 100);
                progressDetail = total > 0
                  ? `${formatBytes(uploaded)} of ${formatBytes(total)}`
                  : null;
                progressMode = 'bytes';
              }
            );

            if (!result.success) {
              throw new Error(result.error || "Import failed");
            }

            progressDetail = `${result.files_imported} files`;
            progressMode = 'files';

            // Dispatch event for tree refresh
            window.dispatchEvent(
              new CustomEvent("import:complete", { detail: result })
            );
          }
          await api.initializeWorkspaceCrdt(workspacePath);
          break;
      }

      // IMPORTANT: Now establish the WebSocket sync connection
      // The CRDT is populated with local files, now we need to connect to the server
      if (workspaceId) {
        console.log("[SyncWizard] Establishing sync connection for workspace:", workspaceId);

        // Set workspace ID for proper document routing
        await setWorkspaceId(workspaceId);

        // Mark load_server mode so body CRDTs are cleared before sync
        // (prevents duplication when importFromZip populates body locally)
        if (initMode === 'load_server') {
          setFreshFromServerLoad(true);
        }

        // Snapshot-based flows already established workspace state (upload/download).
        // Drop pre-connect local updates captured during bootstrap to avoid replaying
        // stale create/delete/body changes immediately after connect.
        if (initMode === 'load_server' || (initMode === 'sync_local' && snapshotUploaded)) {
          discardQueuedLocalSyncUpdates(`sync wizard bootstrap (${initMode})`);
        }

        // Set server URL to create UnifiedSyncTransport and connect
        // This triggers the WebSocket connection that syncs CRDT data to server
        // Use getServerUrl() (auth store) as the canonical source — the wizard's
        // local serverUrl $state can become stale if the component re-renders.
        const syncServerUrl = getServerUrl() ?? serverUrl;
        console.log("[SyncWizard] Calling setWorkspaceServer with:", syncServerUrl);
        await setWorkspaceServer(syncServerUrl);
      } else {
        console.warn("[SyncWizard] No workspace ID available after authentication");
      }

      // Wait for metadata sync to complete (30 second timeout)
      // This ensures the wizard shows real progress and doesn't close prematurely
      setStageProgress(80, "Waiting for metadata sync...", "Step 4 of 4");
      console.log("[SyncWizard] Waiting for metadata sync to complete...");
      const syncResult = await waitForInitialSync(30000);

      if (!syncResult) {
        console.warn("[SyncWizard] Metadata sync timed out, continuing in background");
        toast.info("Sync continuing in background", {
          description: "Check the sync indicator in the header for progress.",
        });
      }

      // For sync_local and merge modes, proactively sync body content
      // This uploads the actual file content to the server, not just metadata
      if ((initMode === 'sync_local' && !snapshotUploaded) || initMode === 'merge') {
        console.log("[SyncWizard] Starting body content sync...");
        syncStatusText = "Uploading file contents...";
        progressMode = 'percent';

        try {
          const allFiles = await getAllFiles();
          const filePaths = Array.from(allFiles.keys());

          if (filePaths.length > 0) {
            console.log(`[SyncWizard] Syncing body content for ${filePaths.length} files`);

            // Two-phase progress:
            // Phase 1 (0-50%): Sending subscriptions and uploading
            // Phase 2 (50-100%): Waiting for server confirmation
            let subscriptionsSent = false;

            await proactivelySyncBodies(filePaths, {
              concurrency: 5,
              waitForComplete: true, // Wait for actual sync completion
              syncTimeout: 120000, // 2 minute timeout
              onProgress: (completed, total) => {
                syncCompleted = completed;
                syncTotal = total;
                if (total > 0) {
                  // Phase 1: subscriptions being sent (0-50%)
                  const subscriptionProgress = Math.round((completed / total) * 50);
                  importProgress = subscriptionProgress;

                  if (completed === total && !subscriptionsSent) {
                    subscriptionsSent = true;
                    syncStatusText = "Syncing file contents...";
                    // Phase 2 starts - waiting for server confirmation
                    importProgress = 50;
                  }
                  progressMode = 'percent';
                }
              }
            });

            // If we get here, body sync completed successfully
            importProgress = 100;
            console.log("[SyncWizard] Body content sync complete");
          }
        } catch (e) {
          console.warn("[SyncWizard] Body sync error (continuing anyway):", e);
        }
      }

      if (syncResult) {
        if (initMode === 'sync_local' && snapshotUploaded) {
          setStageProgress(100, "Sync initialized", "Ready");
        }
        toast.success("Sync setup complete", {
          description: "Your workspace is now syncing.",
        });
      }

      // Final progress (in case body sync was skipped)
      if (importProgress < 100) {
        importProgress = 100;
      }

      // Mark sync as explicitly enabled (persists across sessions)
      enableSync();

      // Cleanup subscriptions before closing
      cleanupSyncSubscriptions();

      // Close the wizard
      handleClose();
      onComplete?.();
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
    if (screen === 'options') {
      screen = 'auth';
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
        {:else}
          <Settings2 class="size-5" />
          Initialize Workspace
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if screen === 'auth'}
          {#if verificationSent}
            Check your email and click the sign-in link.
          {:else}
            Enter your email to sync across devices.
          {/if}
        {:else}
          {#if isCheckingServerData}
            Checking your synced data...
          {:else if userHasServerData}
            You have {serverFileCount} file{serverFileCount !== 1 ? 's' : ''} synced to the server.
          {:else}
            No existing data on server. Choose how to initialize.
          {/if}
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
                  <Label for="device-name" class="text-sm">Device Name</Label>
                  <Input
                    id="device-name"
                    type="text"
                    bind:value={deviceName}
                    placeholder="My Mac"
                    disabled={isSendingMagicLink}
                  />
                </div>
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

      <!-- Screen 2: Initialization Options -->
      {#if screen === 'options'}
        <!-- Backup download button -->
        <div class="p-3 bg-muted/50 rounded-lg">
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2">
              <Download class="size-4 text-muted-foreground" />
              <span class="text-sm">Download local backup first?</span>
            </div>
            <Button
              variant="outline"
              size="sm"
              onclick={handleDownloadBackup}
              disabled={isDownloadingBackup}
            >
              {#if isDownloadingBackup}
                <Loader2 class="size-4 mr-1 animate-spin" />
                Downloading...
              {:else}
                Download ZIP
              {/if}
            </Button>
          </div>
        </div>

        {#if isCheckingServerData}
          <div class="flex items-center justify-center py-8">
            <Loader2 class="size-6 animate-spin text-muted-foreground" />
          </div>
        {:else}
          <div class="space-y-3">
            {#if userHasServerData}
              <!-- User has server data: Load from server / Merge -->
              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {initMode === 'load_server' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => initMode = 'load_server'}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <CloudDownload class="size-5 {initMode === 'load_server' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">Load from server</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Replace local data with your synced files
                    </div>
                  </div>
                </div>
              </button>

              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {initMode === 'merge' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => initMode = 'merge'}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <Merge class="size-5 {initMode === 'merge' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">Merge</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Combine local and server files
                    </div>
                  </div>
                </div>
              </button>
            {:else}
              <!-- No server data: Sync local / Import -->
              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {initMode === 'sync_local' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => initMode = 'sync_local'}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <RefreshCw class="size-5 {initMode === 'sync_local' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div>
                    <div class="font-medium text-sm">Sync local content</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Upload your current files to the server
                    </div>
                  </div>
                </div>
              </button>

              <button
                type="button"
                class="w-full text-left p-3 rounded-lg border-2 transition-colors {initMode === 'import' ? 'border-primary bg-primary/5' : 'border-border hover:border-muted-foreground/50'}"
                onclick={() => initMode = 'import'}
              >
                <div class="flex items-start gap-3">
                  <div class="mt-0.5">
                    <Upload class="size-5 {initMode === 'import' ? 'text-primary' : 'text-muted-foreground'}" />
                  </div>
                  <div class="flex-1">
                    <div class="font-medium text-sm">Import from backup</div>
                    <div class="text-xs text-muted-foreground mt-0.5">
                      Import from a .zip file
                    </div>
                  </div>
                </div>
              </button>

              <!-- File picker for import mode -->
              {#if initMode === 'import'}
                <div class="mt-2 ml-8">
                  <input
                    type="file"
                    accept=".zip"
                    class="hidden"
                    bind:this={fileInputRef}
                    onchange={handleFileSelected}
                  />
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={triggerFileInput}
                  >
                    {#if selectedFile}
                      <Upload class="size-4 mr-2" />
                      {selectedFile.name}
                    {:else}
                      <Upload class="size-4 mr-2" />
                      Choose .zip File...
                    {/if}
                  </Button>
                </div>
              {/if}
            {/if}
          </div>
        {/if}

        <!-- Progress bar during initialization -->
        {#if isInitializing}
          <div class="space-y-2 pt-2">
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
              {:else if initMode === 'import'}
                Importing files...
              {:else if initMode === 'load_server'}
                Downloading from server...
              {:else if initMode === 'sync_local'}
                Uploading to server...
              {:else if initMode === 'merge'}
                Merging files...
              {:else}
                Initializing workspace...
              {/if}
            </p>
          </div>
        {/if}
      {/if}
    </div>

    <!-- Footer with navigation buttons -->
    <div class="flex justify-between pt-4 border-t">
      {#if screen === 'options'}
        <Button variant="ghost" size="sm" onclick={handleBack}>
          <ArrowLeft class="size-4 mr-1" />
          Back
        </Button>
      {:else if verificationSent && !devLink}
        <Button variant="ghost" size="sm" onclick={() => { verificationSent = false; stopMagicLinkDetection(); }}>
          <ArrowLeft class="size-4 mr-1" />
          Change Email
        </Button>
      {:else}
        <div></div>
      {/if}

      {#if screen === 'auth'}
        {#if !verificationSent}
          <Button onclick={handleSendMagicLink} disabled={isSendingMagicLink || isValidatingServer || !email.trim()}>
            {#if isSendingMagicLink || isValidatingServer}
              <Loader2 class="size-4 mr-2 animate-spin" />
              {isValidatingServer ? 'Connecting...' : 'Sending...'}
            {:else}
              <Mail class="size-4 mr-2" />
              Send Sign-in Link
            {/if}
          </Button>
        {:else if devLink}
          <div></div>
        {:else}
          <!-- Show waiting indicator -->
          <div class="flex items-center gap-2 text-muted-foreground text-sm">
            <Loader2 class="size-4 animate-spin" />
            Waiting for verification...
          </div>
        {/if}
      {:else}
        <Button
          onclick={handleInitialize}
          disabled={isInitializing || isCheckingServerData || (initMode === 'import' && !selectedFile)}
        >
          {#if isInitializing}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Initializing...
          {:else}
            Start Syncing
            <ArrowRight class="size-4 ml-1" />
          {/if}
        </Button>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
