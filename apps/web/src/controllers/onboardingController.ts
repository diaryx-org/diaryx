/**
 * Onboarding Controller
 *
 * Handles onboarding orchestration logic including:
 * - E2E welcome screen bypass
 * - Starter workspace seeding
 * - iOS first-run bootstrap
 * - Default workspace auto-creation
 * - Bundle application during onboarding
 * - Welcome screen callback orchestration (onGetStarted, onSignInCreateNew, onCreateWithProvider)
 */

import type { Api } from '../lib/backend/api';
import type { JsonValue } from '../lib/backend/generated/serde_json/JsonValue';
import { resolveStorageType, type StorageType } from '../lib/backend/storageType';
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
  return backendInstance
    .getWorkspacePath()
    .replace(/\/index\.md$/, '')
    .replace(/\/README\.md$/, '');
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
  createLocalWorkspace: (name: string, storageType?: StorageType) => { id: string; name: string; storageType?: StorageType };
  setCurrentWorkspaceId: (id: string) => void;
  getBackend: (id: string, name: string, storageType?: StorageType) => Promise<any>;
  createApi: (backend: any) => Api;
  setBackend: (backend: any) => void;
  clearRustApi: () => void;
  initEventSubscription: (backend: any) => (() => void);
  setCleanupEventSubscription: (cleanup: (() => void)) => void;
  refreshTree: () => Promise<void>;
  setupPermissions: () => void;
  persistPermissionDefaults: (pluginId: string, defaults: any) => Promise<void>;
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
    deps.clearRustApi();

    deps.setCleanupEventSubscription(deps.initEventSubscription(backendInstance));

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
): Promise<OnCreateWithProviderResult> {
  if (restoreNamespace) {
    // Restore from remote: download plugin bytes, create workspace from remote, then apply bundle
    const providerId = providerPluginId ?? restoreNamespace.metadata?.provider ?? "diaryx.sync";

    // Check if the user provided a local override for the provider plugin
    const providerOverride = pluginOverrides?.find(
      (o) => o.targetPluginId === providerId,
    );

    let pluginWasm: Uint8Array | null = null;
    if (providerOverride) {
      // Use the user-provided override instead of fetching from the marketplace
      pluginWasm = new Uint8Array(providerOverride.bytes);
      console.info(`[onboarding] Using local override for provider plugin "${providerId}"`);
    } else {
      // Fetch the sync plugin wasm bytes from registry (don't install yet — no workspace context)
      const { fetchPluginRegistry } = await import("$lib/plugins/pluginRegistry");
      const registry = await fetchPluginRegistry();
      const syncPlugin = registry.plugins.find((p) => p.id === providerId);
      if (syncPlugin) {
        const { proxyFetch } = await import("$lib/backend/proxyFetch");
        const resp = await proxyFetch(syncPlugin.artifact.url);
        if (resp.ok) {
          pluginWasm = new Uint8Array(await resp.arrayBuffer());
        }
      }
      if (!pluginWasm) {
        throw new Error(
          `Could not download the sync plugin${syncPlugin ? "" : ` "${providerId}"`}. Check your network connection and try again.`,
        );
      }
    }

    // Download workspace from remote, providing pre-fetched plugin bytes
    const { downloadWorkspace } = await import("$lib/sync/workspaceProviderService");
    const name = restoreNamespace.metadata?.name ?? "Restored Workspace";
    let result: { localId: string; filesImported: number };
    try {
      result = await downloadWorkspace(providerId, {
        remoteId: restoreNamespace.id,
        name,
        link: true,
      }, undefined, undefined, pluginWasm);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      const isUnsupported = msg.includes("only available in host-integrated runtimes")
        || msg.includes("not implemented")
        || msg.includes("Unknown command")
        || /No plugin .* handles command/.test(msg);
      if (isUnsupported) {
        throw new Error(
          "The selected storage provider does not support downloading workspaces. Try a different provider or create a new local workspace instead.",
        );
      }
      throw err;
    }

    // Wire up the UI to the workspace that downloadWorkspace already created.
    // Do NOT call switchWorkspace — it calls resetBackend() which would
    // destroy the backend instance that the downloaded content was written
    // through, potentially creating a fresh empty OPFS directory.
    const { setActiveWorkspaceId } = await import("$lib/auth");
    setActiveWorkspaceId(result.localId);
    const dlBackend = await (await import("$lib/backend")).getBackend();
    deps.autoCreateDeps.setBackend(dlBackend);
    deps.autoCreateDeps.clearRustApi();
    deps.autoCreateDeps.setCleanupEventSubscription(
      deps.autoCreateDeps.initEventSubscription(dlBackend),
    );

    // Reload plugins with full lifecycle now that the workspace has content
    const { loadAllPlugins } = await import("$lib/plugins/browserPluginManager.svelte");
    await loadAllPlugins();

    // Install local overrides first so they take precedence over marketplace versions
    if (pluginOverrides?.length) {
      for (const o of pluginOverrides) {
        await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
      }
    }

    // Apply bundle plugins on top — exclude any plugins the user overrode locally
    if (bundle && bundle.plugins.length > 0) {
      try {
        const effectiveBundle = excludeOverriddenPlugins(bundle, pluginOverrides);
        await applyOnboardingBundle(effectiveBundle, deps.persistPermissionDefaults);
      } catch (e) {
        console.warn("[App] Bundle apply on restored workspace failed (non-fatal):", e);
      }
    }
  } else {
    // New workspace from bundle — install overrides before bundle apply so they aren't overwritten
    const overrideIds = new Set(pluginOverrides?.map((o) => o.targetPluginId) ?? []);
    const effectiveBundle = bundle && overrideIds.size > 0
      ? excludeOverriddenPlugins(bundle, pluginOverrides)
      : bundle;
    const { id, name } = await autoCreateDefaultWorkspace(deps.autoCreateDeps, effectiveBundle);
    if (pluginOverrides?.length) {
      for (const o of pluginOverrides) {
        await deps.installLocalPlugin(o.bytes, o.fileName.replace(/\.wasm$/, ""));
      }
    }
    if (providerPluginId) {
      const { linkWorkspace } = await import("$lib/sync/workspaceProviderService");
      await linkWorkspace(providerPluginId, { localId: id, name });
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
    spotlightSteps: bundle?.spotlight?.length ? bundle.spotlight : null,
  };
}

// ---------------------------------------------------------------------------
// Post-welcome transition
// ---------------------------------------------------------------------------

export interface HandleWelcomeCompleteDeps {
  getBackend: () => Promise<any>;
  setBackend: (backend: any) => void;
  clearRustApi: () => void;
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
  deps.clearRustApi();

  await deps.refreshTree();

  const tree = deps.getTree();
  const currentEntry = deps.getCurrentEntry();
  if (tree && !currentEntry) {
    deps.expandNode(tree.path);
    await deps.openEntry(tree.path);
  }

  await deps.runValidation();
}
