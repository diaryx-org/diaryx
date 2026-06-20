/**
 * Onboarding Controller
 *
 * Handles onboarding orchestration logic including:
 * - E2E welcome screen bypass
 * - Starter workspace seeding
 * - iOS first-run bootstrap
 * - Default workspace auto-creation
 * - Bundle application during onboarding
 * - Folder workspace callback orchestration
 */

import type { Api } from '../lib/backend/api';
import type { YamlValue } from '../lib/backend/generated/YamlValue';
import {
  resolveStorageType,
  storeWorkspaceFileSystemHandle,
  type StorageType,
} from '../lib/backend/storageType';
import type { BundleRegistryEntry } from '$lib/marketplace/types';
import {
  fetchStarterWorkspaceRegistry,
} from '$lib/marketplace/starterWorkspaceRegistry';
import {
  fetchStarterWorkspaceZip,
} from '$lib/marketplace/starterWorkspaceApply';
import {
  planBundleApply,
  executeBundleApply,
  createDefaultBundleApplyRuntime,
} from '$lib/marketplace/bundleApply';
import {
  hydrateOnboardingPluginPermissionDefaults,
} from '$lib/marketplace/onboardingPluginPermissions';
import { fetchThemeRegistry } from '$lib/marketplace/themeRegistry';
import { fetchTypographyRegistry } from '$lib/marketplace/typographyRegistry';
import { fetchPluginRegistry } from '$lib/plugins/pluginRegistry';
import { isTauri, resetBackend } from '../lib/backend';
import { isIOS } from '$lib/hooks/useMobile.svelte';
import { removeLocalWorkspace } from '$lib/storage/localWorkspaceRegistry.svelte';
import { getWorkspaceDirectoryPath as getWorkspaceDirectoryPathFromRoot } from '$lib/utils/path';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function normalizeFrontmatter(frontmatter: any): Record<string, any> {
  if (!frontmatter) return {};
  if (frontmatter instanceof Map) {
    return Object.fromEntries(frontmatter.entries());
  }
  return frontmatter;
}

export function getWorkspaceDirectoryPath(backendInstance: { getWorkspacePath(): string }): string {
  return getWorkspaceDirectoryPathFromRoot(backendInstance.getWorkspacePath());
}

/**
 * Return a shallow copy of the bundle with any plugins that the user
 * overrode locally removed from the plugins list.  This prevents
 * `applyOnboardingBundle` from re-downloading and overwriting them.
 */
function excludeOverriddenPlugins(
  bundle: BundleRegistryEntry,
  overrides: Array<{ targetPluginId: string }> | null | undefined,
): BundleRegistryEntry {
  if (!overrides?.length) return bundle;
  const overrideIds = new Set(overrides.map((o) => o.targetPluginId));
  return {
    ...bundle,
    plugins: bundle.plugins.filter((p) => !overrideIds.has(p.plugin_id)),
  };
}

export function isWorkspaceAlreadyExistsError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes("Workspace already exists") ||
    message.includes("WorkspaceAlreadyExists")
  );
}

export function shouldBypassWelcomeScreenForE2E(): boolean {
  return (
    import.meta.env.DEV &&
    typeof window !== "undefined" &&
    window.location.hostname === "localhost" &&
    typeof localStorage !== "undefined" &&
    localStorage.getItem("diaryx_e2e_skip_onboarding") === "1"
  );
}

// ---------------------------------------------------------------------------
// Starter content seeding
// ---------------------------------------------------------------------------

/**
 * Seed a freshly-created workspace with hard-coded welcome entries.
 * Returns the root index path.
 */
export async function seedStarterWorkspaceContent(
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
    "Detailed Guide" as YamlValue,
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

// ---------------------------------------------------------------------------
// iOS first-run bootstrap
// ---------------------------------------------------------------------------

/**
 * On iOS/Tauri, bootstrap starter workspace content if the workspace is empty.
 * Returns true if content was seeded.
 */
export async function maybeBootstrapIosStarterWorkspace(
  apiInstance: Api,
  backendInstance: { getWorkspacePath(): string; getFilesystemTree?: never } & Record<string, any>,
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

// ---------------------------------------------------------------------------
// Bundle application
// ---------------------------------------------------------------------------

/**
 * Apply a bundle's plugins/theme/typography during onboarding.
 * Fetches the necessary registries and executes the bundle plan.
 */
export async function applyOnboardingBundle(
  bundle: BundleRegistryEntry,
  persistPermissionDefaults: (pluginId: string, defaults: any) => Promise<void>,
): Promise<void> {
  // Fetch registries in parallel for the bundle plan context
  const [themeReg, typoReg, pluginReg] = await Promise.all([
    fetchThemeRegistry().catch(() => ({ themes: [] })),
    fetchTypographyRegistry().catch(() => ({ typographies: [] })),
    fetchPluginRegistry().catch(() => ({ plugins: [] })),
  ]);

  await hydrateOnboardingPluginPermissionDefaults(
    bundle.plugins,
    pluginReg.plugins,
    persistPermissionDefaults,
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

// ---------------------------------------------------------------------------
// Default workspace auto-creation
// ---------------------------------------------------------------------------

export interface AutoCreateWorkspaceDeps {
  createLocalWorkspace: (name: string, storageType?: StorageType, path?: string) => { id: string; name: string; storageType?: StorageType; path?: string };
  setCurrentWorkspaceId: (id: string) => void;
  getBackend: (id: string, name: string, storageType?: StorageType) => Promise<any>;
  createApi: (backend: any) => Api;
  setBackend: (backend: any) => void;
  initEventSubscription: (backend: any) => (() => void);
  setCleanupEventSubscription: (cleanup: (() => void)) => void;
  refreshTree: () => Promise<void>;
  setupPermissions: () => void;
  persistPermissionDefaults: (pluginId: string, defaults: any) => Promise<void>;
}

export interface FolderWorkspaceTarget {
  name: string;
  path?: string;
  storageType?: StorageType;
  directoryHandle?: FileSystemDirectoryHandle;
}

async function initializeRegisteredWorkspace(
  deps: AutoCreateWorkspaceDeps,
  target: FolderWorkspaceTarget,
): Promise<{ id: string; name: string; backend: any; api: Api; workspaceDir: string }> {
  const name = target.name.trim() || "My Workspace";
  const ws = deps.createLocalWorkspace(name, target.storageType, target.path);
  deps.setCurrentWorkspaceId(ws.id);

  try {
    if (target.directoryHandle) {
      await storeWorkspaceFileSystemHandle(ws.id, target.directoryHandle);
    }

    const backendInstance = await deps.getBackend(ws.id, ws.name, ws.storageType);
    deps.setBackend(backendInstance);

    const apiInstance = deps.createApi(backendInstance);
    deps.setCleanupEventSubscription(deps.initEventSubscription(backendInstance));

    return {
      id: ws.id,
      name: ws.name,
      backend: backendInstance,
      api: apiInstance,
      workspaceDir: getWorkspaceDirectoryPath(backendInstance),
    };
  } catch (err) {
    console.error("[onboarding] folder workspace initialization failed, rolling back:", err);
    removeLocalWorkspace(ws.id);
    resetBackend();
    throw err;
  }
}

async function seedWorkspaceFromBundle(
  apiInstance: Api,
  backendInstance: any,
  workspaceDir: string,
  workspaceName: string,
  bundle?: BundleRegistryEntry | null,
): Promise<void> {
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
    await seedStarterWorkspaceContent(apiInstance, workspaceDir, workspaceName);
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
export async function autoCreateDefaultWorkspace(
  deps: AutoCreateWorkspaceDeps,
  bundle?: BundleRegistryEntry | null,
): Promise<{ id: string; name: string }> {
  const ws = deps.createLocalWorkspace(
    "My Workspace",
    await resolveStorageType(),
  );
  deps.setCurrentWorkspaceId(ws.id);

  try {
    const backendInstance = await deps.getBackend(ws.id, ws.name, ws.storageType);
    deps.setBackend(backendInstance);

    const apiInstance = deps.createApi(backendInstance);

    deps.setCleanupEventSubscription(deps.initEventSubscription(backendInstance));

    const workspaceDir = getWorkspaceDirectoryPath(backendInstance);

    await seedWorkspaceFromBundle(apiInstance, backendInstance, workspaceDir, ws.name, bundle);

    // Load the workspace tree and permission config before installing plugins.
    // The starter workspace frontmatter may contain pre-configured plugin
    // permissions so that plugins can be installed without prompting the user.
    await deps.refreshTree();
    deps.setupPermissions();

    // Apply the bundle (plugins, theme, typography) — best-effort, non-blocking
    if (bundle && bundle.plugins.length > 0) {
      try {
        await applyOnboardingBundle(bundle, deps.persistPermissionDefaults);
      } catch (e) {
        console.warn("[App] Bundle apply during onboarding failed (non-fatal):", e);
      }
    }

    return { id: ws.id, name: ws.name };
  } catch (err) {
    // Rollback: remove the half-created workspace from the registry and reset backend
    console.error("[onboarding] autoCreateDefaultWorkspace failed, rolling back:", err);
    removeLocalWorkspace(ws.id);
    resetBackend();
    throw err;
  }
}

// ---------------------------------------------------------------------------
// Welcome screen callback orchestration
// ---------------------------------------------------------------------------

export interface OnGetStartedDeps {
  autoCreateDeps: AutoCreateWorkspaceDeps;
  installLocalPlugin: (bytes: ArrayBuffer, name: string) => Promise<void>;
  refreshTree: () => Promise<void>;
  getTree: () => { path: string } | null;
  expandNode: (path: string) => void;
  openEntry: (path: string) => Promise<void>;
  runValidation: () => Promise<void>;
  dismissLaunchOverlay: () => Promise<void>;
}

export interface OnGetStartedResult {
  /** Non-null when the bundle defines a spotlight tour to trigger. */
  spotlightSteps: any[] | null;
}

export type OnFolderWorkspaceResult = OnGetStartedResult;

async function resolveExistingWorkspaceRootIndex(
  apiInstance: Api,
  workspaceDir: string,
): Promise<string | null> {
  return await apiInstance.resolveWorkspaceRootIndexPath(workspaceDir);
}

async function openInitializedFolderWorkspace(
  deps: OnOpenFolderWorkspaceDeps,
  rootIndexPath: string,
): Promise<void> {
  deps.autoCreateDeps.setupPermissions();
  await deps.refreshTree();

  const tree = deps.getTree();
  if (tree) {
    deps.expandNode(tree.path);
  }
  await deps.openEntry(rootIndexPath);
  await deps.runValidation();
}

/**
 * Orchestrates the "Get Started" flow from the welcome screen.
 * Creates workspace from bundle, installs overrides, triggers spotlight.
 *
 * Caller is responsible for:
 * - Setting entryStore.setLoading(true/false)
 * - Setting showWelcomeScreen = false
 * - Assigning spotlightSteps to the returned value (after tick + rAF)
 */
export async function handleGetStarted(
  deps: OnGetStartedDeps,
  selectedBundle: BundleRegistryEntry | null,
  pluginOverrides: Array<{ targetPluginId: string; bytes: ArrayBuffer; fileName: string }> | null | undefined,
): Promise<OnGetStartedResult> {
  const effectiveBundle = selectedBundle && pluginOverrides?.length
    ? excludeOverriddenPlugins(selectedBundle, pluginOverrides)
    : selectedBundle;
  await autoCreateDefaultWorkspace(deps.autoCreateDeps, effectiveBundle);

  if (pluginOverrides?.length) {
    for (const o of pluginOverrides) {
      await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
    }
  }

  await deps.refreshTree();
  const tree = deps.getTree();
  if (tree) {
    deps.expandNode(tree.path);
    await deps.openEntry(tree.path);
  }
  await deps.runValidation();
  await deps.dismissLaunchOverlay();

  return {
    spotlightSteps: selectedBundle?.spotlight?.length ? selectedBundle.spotlight : null,
  };
}

export async function handleCreateFolderWorkspace(
  deps: OnGetStartedDeps,
  target: FolderWorkspaceTarget,
  selectedBundle: BundleRegistryEntry | null,
  pluginOverrides: Array<{ targetPluginId: string; bytes: ArrayBuffer; fileName: string }> | null | undefined,
): Promise<OnFolderWorkspaceResult> {
  const effectiveBundle = selectedBundle && pluginOverrides?.length
    ? excludeOverriddenPlugins(selectedBundle, pluginOverrides)
    : selectedBundle;

  const initialized = await initializeRegisteredWorkspace(deps.autoCreateDeps, target);
  await seedWorkspaceFromBundle(
    initialized.api,
    initialized.backend,
    initialized.workspaceDir,
    initialized.name,
    effectiveBundle,
  );

  await deps.refreshTree();
  deps.autoCreateDeps.setupPermissions();

  if (pluginOverrides?.length) {
    for (const o of pluginOverrides) {
      await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
    }
  }

  if (effectiveBundle && effectiveBundle.plugins.length > 0) {
    try {
      await applyOnboardingBundle(effectiveBundle, deps.autoCreateDeps.persistPermissionDefaults);
    } catch (e) {
      console.warn("[App] Bundle apply during folder onboarding failed (non-fatal):", e);
    }
  }

  await deps.refreshTree();
  const tree = deps.getTree();
  const rootIndexPath = await initialized.api.resolveWorkspaceRootIndexPath(
    initialized.backend.getWorkspacePath(),
  );
  if (tree) {
    deps.expandNode(tree.path);
  }
  if (rootIndexPath) {
    await deps.openEntry(rootIndexPath);
  }
  await deps.runValidation();
  await deps.dismissLaunchOverlay();

  return {
    spotlightSteps: selectedBundle?.spotlight?.length ? selectedBundle.spotlight : null,
  };
}

export async function handleChooseFolderWorkspace(
  deps: OnGetStartedDeps,
  target: FolderWorkspaceTarget,
  selectedBundle: BundleRegistryEntry | null,
  pluginOverrides: Array<{ targetPluginId: string; bytes: ArrayBuffer; fileName: string }> | null | undefined,
): Promise<OnFolderWorkspaceResult> {
  const initialized = await initializeRegisteredWorkspace(deps.autoCreateDeps, target);
  const existingRootIndexPath = await resolveExistingWorkspaceRootIndex(
    initialized.api,
    initialized.workspaceDir,
  );

  if (existingRootIndexPath) {
    await openInitializedFolderWorkspace(deps, existingRootIndexPath);
    await deps.dismissLaunchOverlay();
    return { spotlightSteps: null };
  }

  const effectiveBundle = selectedBundle && pluginOverrides?.length
    ? excludeOverriddenPlugins(selectedBundle, pluginOverrides)
    : selectedBundle;

  await seedWorkspaceFromBundle(
    initialized.api,
    initialized.backend,
    initialized.workspaceDir,
    initialized.name,
    effectiveBundle,
  );

  await deps.refreshTree();
  deps.autoCreateDeps.setupPermissions();

  if (pluginOverrides?.length) {
    for (const o of pluginOverrides) {
      await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
    }
  }

  if (effectiveBundle && effectiveBundle.plugins.length > 0) {
    try {
      await applyOnboardingBundle(effectiveBundle, deps.autoCreateDeps.persistPermissionDefaults);
    } catch (e) {
      console.warn("[App] Bundle apply during folder onboarding failed (non-fatal):", e);
    }
  }

  await deps.refreshTree();
  const tree = deps.getTree();
  const rootIndexPath = await initialized.api.resolveWorkspaceRootIndexPath(
    initialized.backend.getWorkspacePath(),
  );
  if (tree) {
    deps.expandNode(tree.path);
  }
  if (rootIndexPath) {
    await deps.openEntry(rootIndexPath);
  }
  await deps.runValidation();
  await deps.dismissLaunchOverlay();

  return {
    spotlightSteps: selectedBundle?.spotlight?.length ? selectedBundle.spotlight : null,
  };
}

export interface OnOpenFolderWorkspaceDeps {
  autoCreateDeps: AutoCreateWorkspaceDeps;
  refreshTree: () => Promise<void>;
  getTree: () => { path: string } | null;
  expandNode: (path: string) => void;
  openEntry: (path: string) => Promise<void>;
  runValidation: () => Promise<void>;
}

export async function handleOpenFolderWorkspace(
  deps: OnOpenFolderWorkspaceDeps,
  target: FolderWorkspaceTarget,
): Promise<void> {
  const initialized = await initializeRegisteredWorkspace(deps.autoCreateDeps, target);
  deps.autoCreateDeps.setupPermissions();
  await deps.refreshTree();

  try {
    const rootIndexPath = await initialized.api.resolveWorkspaceRootIndexPath(
      initialized.backend.getWorkspacePath(),
    );
    if (rootIndexPath) {
      const tree = deps.getTree();
      if (tree) {
        deps.expandNode(tree.path);
      }
      await deps.openEntry(rootIndexPath);
      await deps.runValidation();
    }
  } catch (e) {
    console.warn("[onboarding] Opened folder has no Diaryx root index yet:", e);
  }
}

export interface OnSignInCreateNewDeps {
  autoCreateDeps: AutoCreateWorkspaceDeps;
  refreshTree: () => Promise<void>;
  getTree: () => { path: string } | null;
  expandNode: (path: string) => void;
  openEntry: (path: string) => Promise<void>;
  runValidation: () => Promise<void>;
}

/**
 * Orchestrates workspace creation after sign-in when user has no existing workspaces.
 *
 * Caller is responsible for:
 * - Setting entryStore.setLoading(true/false)
 * - Setting showWelcomeScreen = false
 */
export async function handleSignInCreateNew(
  deps: OnSignInCreateNewDeps,
): Promise<void> {
  await autoCreateDefaultWorkspace(deps.autoCreateDeps, null);

  await deps.refreshTree();
  const tree = deps.getTree();
  if (tree) {
    deps.expandNode(tree.path);
    await deps.openEntry(tree.path);
  }
  await deps.runValidation();
}

export interface OnCreateWithProviderDeps {
  autoCreateDeps: AutoCreateWorkspaceDeps;
  installLocalPlugin: (bytes: ArrayBuffer, name: string) => Promise<void>;
  refreshTree: () => Promise<void>;
  getTree: () => { path: string } | null;
  expandNode: (path: string) => void;
  openEntry: (path: string) => Promise<void>;
  runValidation: () => Promise<void>;
  dismissLaunchOverlay: () => Promise<void>;
  persistPermissionDefaults: (pluginId: string, defaults: any) => Promise<void>;
  switchWorkspace: (localId: string, name: string) => Promise<void>;
}

export interface OnCreateWithProviderResult {
  spotlightSteps: any[] | null;
}

export interface OnboardingProgress {
  percent: number;
  message: string;
  detail?: string;
}

/**
 * Orchestrates workspace creation/restore with a sync provider.
 *
 * Caller is responsible for:
 * - Setting entryStore.setLoading(true/false)
 * - Setting showWelcomeScreen = false
 * - Assigning spotlightSteps to the returned value (after tick + rAF)
 */
export async function handleCreateWithProvider(
  deps: OnCreateWithProviderDeps,
  bundle: BundleRegistryEntry | null | undefined,
  providerPluginId: string | null | undefined,
  pluginOverrides: Array<{ targetPluginId: string; bytes: ArrayBuffer; fileName: string }> | null | undefined,
  restoreNamespace: { id: string; metadata?: { provider?: string; name?: string; [key: string]: unknown } | null } | null | undefined,
  onProgress?: (progress: OnboardingProgress) => void,
): Promise<OnCreateWithProviderResult> {
  if (restoreNamespace) {
    throw new Error("Remote workspace restore has been removed. Create or open a local folder workspace instead.");
  }
  if (providerPluginId) {
    throw new Error("Provider-backed workspace creation has been removed. Create or open a local folder workspace instead.");
  }

  // New workspace from bundle — install overrides before bundle apply so they
  // are not overwritten by marketplace artifacts.
  const overrideIds = new Set(pluginOverrides?.map((o) => o.targetPluginId) ?? []);
  const effectiveBundle = bundle && overrideIds.size > 0
    ? excludeOverriddenPlugins(bundle, pluginOverrides)
    : bundle;
  onProgress?.({ percent: 10, message: "Creating workspace..." });
  await autoCreateDefaultWorkspace(deps.autoCreateDeps, effectiveBundle);
  if (pluginOverrides?.length) {
    onProgress?.({ percent: 36, message: "Installing selected plugins..." });
    for (const o of pluginOverrides) {
      await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
    }
  }

  onProgress?.({ percent: 94, message: "Finishing setup..." });
  await deps.refreshTree();
  const tree = deps.getTree();
  if (tree) {
    deps.expandNode(tree.path);
    await deps.openEntry(tree.path);
  }
  await deps.runValidation();
  await deps.dismissLaunchOverlay();

  return {
    spotlightSteps: bundle?.spotlight?.length ? bundle.spotlight : null,
  };
}

// ---------------------------------------------------------------------------
// Post-welcome transition
// ---------------------------------------------------------------------------

export interface HandleWelcomeCompleteDeps {
  getBackend: () => Promise<any>;
  setBackend: (backend: any) => void;
  refreshTree: () => Promise<void>;
  getTree: () => { path: string } | null;
  getCurrentEntry: () => any | null;
  expandNode: (path: string) => void;
  openEntry: (path: string) => Promise<void>;
  runValidation: () => Promise<void>;
}

/**
 * Post-welcome transition — backend already initialized by switchWorkspace.
 *
 * Caller is responsible for:
 * - Setting showWelcomeScreen = false
 * - Setting entryStore.setLoading(true/false)
 * - Setting uiStore.setError on failure
 */
export async function handleWelcomeComplete(
  deps: HandleWelcomeCompleteDeps,
  _id: string,
  _name: string,
): Promise<void> {
  // Backend already initialized by switchWorkspace.
  // Just refresh UI state.
  const newBackend = await deps.getBackend();
  deps.setBackend(newBackend);

  await deps.refreshTree();

  const tree = deps.getTree();
  const currentEntry = deps.getCurrentEntry();
  if (tree && !currentEntry) {
    deps.expandNode(tree.path);
    await deps.openEntry(tree.path);
  }

  await deps.runValidation();
}
