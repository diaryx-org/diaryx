<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience.
   *
   * Views:
   * - main: Two-button welcome
   * - sign-in: Embedded SignInForm
   * - workspace-picker: List of user's synced workspaces after auth
   * - bundles: Full-screen bundle picker
   * - provider-choice: Choose where workspace lives (after bundle selection)
   */
  import { Button } from "$lib/components/ui/button";
  import { Progress } from "$lib/components/ui/progress";
  import { ArrowLeft, LogIn, Loader2, Cloud, Download, HardDrive, Lock, Plus } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import type { NamespaceEntry } from "$lib/auth/authService";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import AnimatedLogo from "./AnimatedLogo.svelte";
  import BundleCarousel, { type BundleSelectInfo, type PluginOverride } from "./BundleCarousel.svelte";
  import { isAuthenticated, listUserWorkspaceNamespaces } from "$lib/auth/authStore.svelte";
  import {
    fetchPluginRegistry,
    getRegistryWorkspaceProviders,
    type RegistryPlugin,
    type RegistryWorkspaceProvider,
  } from "$lib/plugins/pluginRegistry";
  import { installRegistryPlugin } from "$lib/plugins/pluginInstallService";
  import {
    getBuiltinWorkspaceProviders,
    getProviderDisplayLabel,
    getProviderUnavailableReason,
    isProviderAvailableHere,
  } from "$lib/sync/builtinProviders";
  import { listRemoteWorkspaces } from "$lib/sync/workspaceProviderService";

  interface Props {
    /** Called to create a local workspace (no account) */
    onGetStarted: (selectedBundle: BundleRegistryEntry | null, pluginOverrides?: PluginOverride[]) => void | Promise<void>;
    /** Called after sign-in when user has no existing workspaces — create first synced workspace */
    onSignInCreateNew: () => void | Promise<void>;
    /** Called when user picks a provider for a new workspace (or restores from remote) */
    onCreateWithProvider: (
      bundle: BundleRegistryEntry | null,
      providerPluginId: string | null,
      pluginOverrides?: PluginOverride[],
      restoreNamespace?: NamespaceEntry,
      onProgress?: (progress: { percent: number; message: string; detail?: string }) => void,
    ) => void | Promise<void>;
    /** Called when changing storage location for the currently open workspace. */
    onMoveCurrentWorkspace?: (
      providerPluginId: string | null,
      onProgress?: (progress: { percent: number; message: string; detail?: string }) => void,
    ) => void | Promise<void>;
    /** Called to show the launch zoom overlay — App.svelte owns rendering */
    onLaunch?: (info: BundleSelectInfo) => void;
    /** When set, user navigated here from an existing workspace — show a "Return" button */
    returnWorkspaceName?: string | null;
    onReturn?: () => void;
    /** When set, jump directly to a specific view on mount. */
    initialView?: WelcomeView | null;
  }

  let {
    onGetStarted,
    onSignInCreateNew,
    onCreateWithProvider,
    onMoveCurrentWorkspace,
    onLaunch,
    returnWorkspaceName = null,
    onReturn,
    initialView = null,
  }: Props = $props();

  // View state machine
  type WelcomeView = 'main' | 'sign-in' | 'workspace-picker' | 'bundles' | 'provider-choice';
  let currentView = $state<WelcomeView>('main');
  let transitionDirection = $state<'forward' | 'back'>('forward');

  // Data
  let bundles = $state<BundleRegistryEntry[]>([]);
  let themes = $state<ThemeRegistryEntry[]>([]);
  let loading = $state(true);
  let settingUp = $state(false);
  let setupProgress = $state<{ percent: number; message: string; detail?: string | null } | null>(null);
  let setupError = $state<string | null>(null);

  // Bundle + provider choice state
  let selectedBundle = $state<BundleRegistryEntry | null>(null);
  let pendingOverrides = $state<PluginOverride[] | undefined>(undefined);
  type WelcomeProvider = {
    pluginId: string;
    label: string;
    description: string | null;
    source: "plugin" | "builtin";
    requiresAuth: boolean;
  };

  let bundleProviders = $state<WelcomeProvider[]>([]);
  let registryProviderPlugins = $state<RegistryPlugin[]>([]);
  let signInForProvider = $state<string | null>(null);
  let checkingProvider = $state(false);
  let showInstallableProviders = $state(false);
  let installingProviderPluginId = $state<string | null>(null);
  let workspacePickerProviderId = $state<string | null>(null);
  let workspacePickerBackView = $state<WelcomeView>('main');
  /** True when provider-choice was reached via the "Move" button (not "Create new"). */
  let isMovingCurrentWorkspace = $state(false);
  const pluginStore = getPluginStore();
  let workspaceProviders = $derived(pluginStore.workspaceProviders);
  const managingCurrentWorkspace = $derived(
    !!returnWorkspaceName && typeof onMoveCurrentWorkspace === "function",
  );

  // Deferred zoom animation state
  let launchInfo = $state<BundleSelectInfo | null>(null);
  let fadingOut = $state(false);

  // Onboarding animation state — buttons disabled until complete
  const ANIMATION_DURATION_MS = 2800;
  let animationDone = $state(false);
  let animationSkipped = $state(false);
  let animationTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    animationTimer = setTimeout(() => { animationDone = true; }, ANIMATION_DURATION_MS);
    return () => clearTimeout(animationTimer);
  });

  function skipAnimation() {
    if (animationDone || currentView !== 'main') return;
    clearTimeout(animationTimer);
    animationSkipped = true;
    animationDone = true;
  }

  // Workspace picker state
  let workspaceNamespaces = $state<NamespaceEntry[]>([]);
  let loadingWorkspaces = $state(false);
  const builtinProviders = $derived.by(() =>
    getBuiltinWorkspaceProviders().map((provider): WelcomeProvider => ({
      pluginId: String(provider.pluginId),
      label: provider.contribution.label,
      description: provider.contribution.description ?? null,
      source: provider.source,
      requiresAuth: false,
    })),
  );
  const manageModeProviders = $derived.by(() =>
    workspaceProviders.map((provider): WelcomeProvider => ({
      pluginId: String(provider.pluginId),
      label: provider.contribution.label,
      description: provider.contribution.description ?? null,
      source: provider.source,
      requiresAuth: provider.source !== "builtin",
    })),
  );
  const providerChoiceProviders = $derived(
    isMovingCurrentWorkspace ? manageModeProviders : bundleProviders,
  );
  const installableProviderPlugins = $derived.by(() => {
    const installedProviderIds = new Set(
      workspaceProviders.map((provider) => String(provider.pluginId)),
    );
    return registryProviderPlugins.filter((plugin) => !installedProviderIds.has(plugin.id));
  });

  function toWelcomeProvider(provider: RegistryWorkspaceProvider): WelcomeProvider {
    return {
      pluginId: provider.pluginId,
      label: provider.label,
      description: provider.description ?? null,
      source: "plugin",
      requiresAuth: true,
    };
  }

  function toSyntheticNamespace(
    providerId: string,
    remote: { id: string; name: string },
  ): NamespaceEntry {
    return {
      id: remote.id,
      owner_user_id: "builtin",
      created_at: Date.now(),
      metadata: {
        type: "workspace",
        kind: "workspace",
        provider: providerId,
        name: remote.name,
      },
    };
  }

  async function playZoomThen(callback: () => void | Promise<void>) {
    if (launchInfo) {
      // Fade out the current view
      fadingOut = true;
      await new Promise((r) => setTimeout(r, 350));
      // Tell App.svelte to show the zoom overlay
      onLaunch?.(launchInfo);
      // Wait for zoom animation before starting work
      await new Promise((r) => setTimeout(r, 700));
    }
    try {
      await callback();
    } catch (err) {
      fadingOut = false;
      throw err;
    }
  }

  function beginSetup(initialMessage: string): void {
    setupError = null;
    setupProgress = { percent: 8, message: initialMessage, detail: null };
    settingUp = true;
  }

  function updateSetupProgress(progress: { percent: number; message: string; detail?: string }): void {
    setupProgress = {
      percent: progress.percent,
      message: progress.message,
      detail: progress.detail ?? null,
    };
  }

  function failSetup(error: unknown, fallbackTitle: string): void {
    settingUp = false;
    setupError = error instanceof Error ? error.message : String(error);
    toast.error(fallbackTitle, {
      description: setupError,
    });
  }

  async function handleGetStarted(bundle: BundleRegistryEntry | null, overrides?: PluginOverride[]) {
    beginSetup("Creating workspace...");
    try {
      await playZoomThen(() => onGetStarted(bundle, overrides));
    } catch (e) {
      failSetup(e, "Failed to create workspace");
    }
  }

  async function handleBundleSelected(info: BundleSelectInfo, overrides?: PluginOverride[]) {
    launchInfo = info;
    const bundle = info.bundle;
    pendingOverrides = overrides;

    selectedBundle = bundle;

    const pluginIds = bundle.plugins.map((p) => p.plugin_id);

    // If overrides include a plugin not in the bundle, add its ID to the check list
    if (overrides) {
      for (const o of overrides) {
        if (o.targetPluginId === "__new__" || !pluginIds.includes(o.targetPluginId)) {
          // New plugin added — we can't know its ID from the bundle, but it
          // will be inspected during install. For provider detection, we still
          // check the bundle's declared plugins.
        }
      }
    }

    try {
      const registry = await fetchPluginRegistry();
      registryProviderPlugins = registry.plugins.filter((plugin) =>
        Array.isArray(plugin.ui) && plugin.ui.some((entry) => entry.slot === "WorkspaceProvider"),
      );
      bundleProviders = [
        ...getRegistryWorkspaceProviders(registry.plugins, pluginIds).map(toWelcomeProvider),
        ...builtinProviders,
      ];
    } catch {
      registryProviderPlugins = [];
      bundleProviders = [...builtinProviders];
    }

    if (bundleProviders.length === 0 && registryProviderPlugins.length === 0) {
      await handleGetStarted(bundle, overrides);
    } else {
      isMovingCurrentWorkspace = false;
      navigateTo('provider-choice');
    }
  }

  async function installWorkspaceProvider(plugin: RegistryPlugin) {
    installingProviderPluginId = plugin.id;
    try {
      await installRegistryPlugin(plugin);
      showInstallableProviders = false;
      toast.success(`${plugin.name} installed.`);
    } catch (e) {
      toast.error("Failed to install workspace provider", {
        description: e instanceof Error ? e.message : String(e),
      });
    } finally {
      installingProviderPluginId = null;
    }
  }

  async function handleProviderSelected(provider: WelcomeProvider) {
    setupError = null;
    if (provider.requiresAuth && !isAuthenticated()) {
      signInForProvider = provider.pluginId;
      navigateTo('sign-in');
      return;
    }

    if (isMovingCurrentWorkspace) {
      await moveCurrentWorkspace(provider.pluginId);
      return;
    }

    await checkProviderNamespaces(provider);
  }

  async function moveCurrentWorkspace(providerPluginId: string | null) {
    if (!onMoveCurrentWorkspace) return;

    beginSetup(providerPluginId ? "Moving workspace..." : "Moving workspace to this device...");
    try {
      await playZoomThen(() => onMoveCurrentWorkspace(providerPluginId, updateSetupProgress));
    } catch (e) {
      failSetup(e, "Failed to move workspace");
    }
  }

  async function checkProviderNamespaces(provider: WelcomeProvider) {
    checkingProvider = true;
    try {
      if (provider.source === "builtin") {
        const remote = await listRemoteWorkspaces(provider.pluginId);
        if (remote.length > 0) {
          workspaceNamespaces = remote.map((entry) =>
            toSyntheticNamespace(provider.pluginId, entry),
          );
          workspacePickerProviderId = provider.pluginId;
          workspacePickerBackView = currentView === 'main' ? 'main' : 'provider-choice';
          navigateTo('workspace-picker');
          return;
        }
      } else {
        const allNamespaces = await listUserWorkspaceNamespaces();
        const providerNs = allNamespaces.filter(
          (ns) => ns.metadata?.provider === provider.pluginId,
        );

        if (providerNs.length > 0) {
          workspaceNamespaces = providerNs;
          workspacePickerProviderId = provider.pluginId;
          workspacePickerBackView = currentView === 'main' ? 'main' : 'provider-choice';
          navigateTo('workspace-picker');
          return;
        }
      }

      beginSetup("Setting up workspace...");
      try {
        await playZoomThen(() => onCreateWithProvider(
          selectedBundle,
          provider.pluginId,
          pendingOverrides,
          undefined,
          updateSetupProgress,
        ));
      } catch (e) {
        failSetup(e, "Failed to set up workspace");
      }
    } catch {
      // Provider listing failed — fall through to create new workspace
      beginSetup("Setting up workspace...");
      try {
        await playZoomThen(() => onCreateWithProvider(
          selectedBundle,
          provider.pluginId,
          pendingOverrides,
          undefined,
          updateSetupProgress,
        ));
      } catch (e) {
        failSetup(e, "Failed to set up workspace");
      }
    } finally {
      checkingProvider = false;
    }
  }

  export async function handleSignInComplete() {
    if (signInForProvider) {
      const providerPluginId = signInForProvider;
      signInForProvider = null;
      const provider = providerChoiceProviders.find((entry) => entry.pluginId === providerPluginId);
      if (provider) {
        if (managingCurrentWorkspace) {
          await moveCurrentWorkspace(provider.pluginId);
          return;
        }
        await checkProviderNamespaces(provider);
      }
      return;
    }

    // When called after sign-in (from the sign-in view), return to main so the
    // user can pick a provider for their current workspace.  But when called
    // directly from the main view (e.g. "Download remote workspace" button),
    // fall through to the workspace-download flow.
    if (managingCurrentWorkspace && currentView !== 'main') {
      navigateTo('main');
      return;
    }

    // Original flow: signed in from main screen
    loadingWorkspaces = true;
    workspacePickerProviderId = null;
    workspacePickerBackView = 'main';
    navigateTo('workspace-picker');
    try {
      workspaceNamespaces = await listUserWorkspaceNamespaces();
    } catch {
      workspaceNamespaces = [];
    } finally {
      loadingWorkspaces = false;
      if (workspaceNamespaces.length === 0) {
        navigateTo('bundles');
      }
    }
  }

  async function handlePickNamespace(ns: NamespaceEntry) {
    if (!workspaceAvailableHere(ns)) {
      toast.error("This workspace is not available on this device.", {
        description: workspaceUnavailableReason(ns) ?? undefined,
      });
      return;
    }

    beginSetup("Restoring workspace...");
    try {
      await playZoomThen(() =>
        onCreateWithProvider(
          null,
          workspaceProviderId(ns),
          undefined,
          ns,
          updateSetupProgress,
        ),
      );
    } catch (e) {
      failSetup(e, "Failed to restore workspace");
    }
  }

  async function handleWorkspacePickerCreateAction() {
    if (workspacePickerProviderId) {
      beginSetup("Creating workspace...");
      try {
        await playZoomThen(() =>
          onCreateWithProvider(
            selectedBundle,
            workspacePickerProviderId,
            pendingOverrides,
            undefined,
            updateSetupProgress,
          ),
        );
      } catch (e) {
        failSetup(e, "Failed to set up workspace");
      }
      return;
    }

    await handleCreateFirstWorkspace();
  }

  async function handleCreateFirstWorkspace() {
    beginSetup("Creating workspace...");
    try {
      await onSignInCreateNew();
    } catch (e) {
      failSetup(e, "Failed to create workspace");
    }
  }

  $effect(() => {
    loadData();
  });

  // When returning from an existing workspace while signed in,
  // skip the main view and jump to the appropriate screen.
  async function autoNavigateIfSignedIn() {
    if (!returnWorkspaceName || !isAuthenticated() || managingCurrentWorkspace) return;
    loadingWorkspaces = true;
    workspacePickerProviderId = null;
    workspacePickerBackView = 'main';
    navigateTo('workspace-picker');
    try {
      workspaceNamespaces = await listUserWorkspaceNamespaces();
    } catch {
      workspaceNamespaces = [];
    } finally {
      loadingWorkspaces = false;
      if (workspaceNamespaces.length === 0) {
        navigateTo('bundles');
      }
    }
  }

  // Run once on mount — honour initialView if provided, otherwise auto-navigate
  if (initialView === 'workspace-picker' && isAuthenticated()) {
    // Jump directly to workspace picker and load namespaces
    loadingWorkspaces = true;
    workspacePickerProviderId = null;
    workspacePickerBackView = 'main';
    navigateTo('workspace-picker');
    listUserWorkspaceNamespaces()
      .then((ns) => { workspaceNamespaces = ns; })
      .catch(() => { workspaceNamespaces = []; })
      .finally(() => { loadingWorkspaces = false; });
  } else if (initialView && initialView !== 'main') {
    navigateTo(initialView);
  } else {
    autoNavigateIfSignedIn();
  }

  async function loadData() {
    loading = true;
    try {
      const [bundleReg, themeReg] = await Promise.all([
        fetchBundleRegistry(),
        fetchThemeRegistry().catch(() => ({ themes: [] as ThemeRegistryEntry[] })),
      ]);
      bundles = bundleReg.bundles;
      themes = themeReg.themes;
      try {
        const registry = await fetchPluginRegistry();
        registryProviderPlugins = registry.plugins.filter((plugin) =>
          Array.isArray(plugin.ui) && plugin.ui.some((entry) => entry.slot === "WorkspaceProvider"),
        );
      } catch {
        registryProviderPlugins = [];
      }
    } catch {
      bundles = [];
      themes = [];
      registryProviderPlugins = [];
    } finally {
      loading = false;
    }
  }

  function navigateTo(view: WelcomeView) {
    const backViews: WelcomeView[] = ['main', 'bundles'];
    transitionDirection = backViews.includes(view) && currentView !== 'main' ? 'back' : 'forward';
    currentView = view;
  }

  function workspaceName(ns: NamespaceEntry): string {
    return ns.metadata?.name ?? ns.id;
  }

  function workspaceProviderId(ns: NamespaceEntry): string {
    return ns.metadata?.provider ?? "diaryx.sync";
  }

  function workspaceProviderLabel(ns: NamespaceEntry): string {
    const providerId = workspaceProviderId(ns);
    return getProviderDisplayLabel(providerId) ?? providerId;
  }

  function workspaceAvailableHere(ns: NamespaceEntry): boolean {
    return isProviderAvailableHere(workspaceProviderId(ns));
  }

  function workspaceUnavailableReason(ns: NamespaceEntry): string | null {
    return getProviderUnavailableReason(workspaceProviderId(ns));
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex items-center justify-center min-h-full welcome-bg px-4 overflow-hidden select-none"
  class:fading-out={fadingOut}
  class:animation-skipped={animationSkipped}
  onclick={skipAnimation}
>
  {#key currentView}
    <div class="w-full view-content {transitionDirection === 'forward' ? 'slide-in-right' : 'slide-in-left'}">

      {#if currentView === 'main'}
        <!-- ============ MAIN VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div class="text-center space-y-4">
            <div class="mx-auto size-24 fade-in" style="animation-delay: 0s">
              <AnimatedLogo size={96} skipAnimation={animationSkipped} />
            </div>
            <h1 class="text-3xl font-bold tracking-tight text-foreground fade-in" style="animation-delay: 2.0s">
              Welcome to Diaryx
            </h1>
            <p class="text-muted-foreground text-sm fade-in" style="animation-delay: 2.2s">
              Your writing. Worth sharing.
            </p>
          </div>

          <div class="space-y-3 fade-in" style="animation-delay: 2.4s">
            {#if isAuthenticated()}
              <Button
                class="w-full get-started-btn"
                disabled={!animationDone || loadingWorkspaces}
                onclick={() => handleSignInComplete()}
              >
                {#if loadingWorkspaces}
                  <Loader2 class="size-4 animate-spin mr-2" />
                  Looking for workspaces…
                {:else}
                  <Download class="size-4 mr-2" />
                  Download remote workspace
                {/if}
              </Button>

              <Button
                variant="ghost"
                class="w-full text-muted-foreground"
                disabled={!animationDone || loading}
                onclick={() => navigateTo('bundles')}
              >
                <Plus class="size-4 mr-2" />
                Create new workspace
              </Button>
            {:else}
              <Button
                class="w-full get-started-btn"
                disabled={!animationDone}
                onclick={() => navigateTo('sign-in')}
              >
                <LogIn class="size-4 mr-2" />
                Sign in to get your workspace
              </Button>

              <Button
                variant="ghost"
                class="w-full text-muted-foreground"
                disabled={!animationDone || loading}
                onclick={() => navigateTo('bundles')}
              >
                Continue without an account
              </Button>
            {/if}

            {#if returnWorkspaceName && onReturn}
              <div class="flex items-center justify-center gap-2 w-full">
                <Button
                  variant="ghost"
                  class="text-muted-foreground"
                  disabled={!animationDone}
                  onclick={onReturn}
                >
                  <ArrowLeft class="size-4 mr-2" />
                  Return to {returnWorkspaceName}
                </Button>
                {#if managingCurrentWorkspace}
                  <button
                    type="button"
                    class="text-xs text-muted-foreground/70 hover:text-foreground transition-colors disabled:opacity-50"
                    disabled={!animationDone}
                    onclick={() => { isMovingCurrentWorkspace = true; navigateTo('provider-choice'); }}
                  >
                    Move
                  </button>
                {/if}
              </div>
            {/if}
          </div>
        </div>

      {:else if currentView === 'sign-in'}
        <!-- ============ SIGN-IN VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors fade-in"
              onclick={() => navigateTo(signInForProvider ? 'provider-choice' : 'main')}
            >
              <ArrowLeft class="size-4" />
              Back
            </button>
          </div>

          <div class="text-center space-y-2 fade-in" style="animation-delay: 0.1s">
            <h1 class="text-2xl font-bold tracking-tight text-foreground">
              Sign in to Diaryx
            </h1>
            <p class="text-muted-foreground text-sm">
              Access your synced workspaces from any device.
            </p>
          </div>

          <div class="fade-in" style="animation-delay: 0.2s">
            <SignInForm compact={true} onAuthenticated={() => handleSignInComplete()} />
          </div>
        </div>

      {:else if currentView === 'provider-choice'}
        <!-- ============ PROVIDER CHOICE VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors fade-in"
              onclick={() => navigateTo(isMovingCurrentWorkspace ? 'main' : 'bundles')}
            >
              <ArrowLeft class="size-4" />
              Back
            </button>
          </div>

          <div class="text-center space-y-2 fade-in" style="animation-delay: 0.1s">
            <h1 class="text-2xl font-bold tracking-tight text-foreground">
              Where should your workspace live?
            </h1>
            <p class="text-muted-foreground text-sm">
              You can change this later in settings.
            </p>
          </div>

          <div class="space-y-3 fade-in" style="animation-delay: 0.2s">
            <button
              type="button"
              class="w-full text-left p-4 rounded-lg border border-border hover:border-primary/50 hover:bg-secondary/50 transition-colors disabled:opacity-50"
              disabled={settingUp}
              onclick={() => isMovingCurrentWorkspace ? moveCurrentWorkspace(null) : handleGetStarted(selectedBundle)}
            >
              <div class="flex items-center gap-3">
                <HardDrive class="size-5 text-muted-foreground shrink-0" />
                <div class="min-w-0">
                  <div class="font-medium text-sm">This device only</div>
                  <div class="text-xs text-muted-foreground">
                    {#if isMovingCurrentWorkspace}
                      Keep this workspace local to this device.
                    {:else}
                      Your workspace stays on this device.
                    {/if}
                  </div>
                </div>
              </div>
            </button>

            {#each providerChoiceProviders as provider (provider.pluginId)}
              <button
                type="button"
                class="w-full text-left p-4 rounded-lg border border-border hover:border-primary/50 hover:bg-secondary/50 transition-colors disabled:opacity-50"
                disabled={settingUp || checkingProvider}
                onclick={() => handleProviderSelected(provider)}
              >
                <div class="flex items-center gap-3">
                  {#if checkingProvider}
                    <Loader2 class="size-5 animate-spin text-primary shrink-0" />
                  {:else}
                    <Cloud class="size-5 text-muted-foreground shrink-0" />
                  {/if}
                  <div class="min-w-0 flex-1">
                    <div class="font-medium text-sm">{provider.label}</div>
                    {#if provider.description}
                      <div class="text-xs text-muted-foreground">
                        {provider.description}
                      </div>
                    {:else}
                      <div class="text-xs text-muted-foreground">
                        Sync across your devices.
                      </div>
                    {/if}
                  </div>
                  {#if provider.requiresAuth && !isAuthenticated()}
                    <Lock class="size-4 text-muted-foreground shrink-0" />
                  {/if}
                </div>
              </button>
            {/each}

            {#if installableProviderPlugins.length > 0}
              <div class="rounded-lg border border-dashed border-border/80 bg-secondary/20 p-3 space-y-3">
                <button
                  type="button"
                  class="w-full flex items-center justify-between text-left"
                  disabled={settingUp || checkingProvider || installingProviderPluginId !== null}
                  onclick={() => { showInstallableProviders = !showInstallableProviders; }}
                >
                  <div>
                    <div class="font-medium text-sm text-foreground">Add another workspace provider</div>
                    <div class="text-xs text-muted-foreground">
                      Install a provider plugin from the registry.
                    </div>
                  </div>
                  <Download class="size-4 text-muted-foreground shrink-0" />
                </button>

                {#if showInstallableProviders}
                  <div class="space-y-2">
                    {#each installableProviderPlugins as plugin (plugin.id)}
                      <div class="rounded-md border border-border/70 bg-background/75 p-3">
                        <div class="flex items-start gap-3">
                          <Cloud class="size-4 text-muted-foreground shrink-0 mt-0.5" />
                          <div class="min-w-0 flex-1">
                            <div class="font-medium text-sm text-foreground">{plugin.name}</div>
                            <div class="text-xs text-muted-foreground mt-1">
                              {plugin.summary}
                            </div>
                          </div>
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={installingProviderPluginId !== null}
                            onclick={() => installWorkspaceProvider(plugin)}
                          >
                            {#if installingProviderPluginId === plugin.id}
                              <Loader2 class="size-3.5 animate-spin mr-1" />
                              Installing…
                            {:else}
                              Install
                            {/if}
                          </Button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>

{#if settingUp}
            <div class="space-y-2">
              <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 class="size-4 animate-spin" />
                {setupProgress?.message ?? "Setting up…"}
              </div>
              {#if setupProgress}
                <Progress value={setupProgress.percent} class="h-1.5" />
                {#if setupProgress.detail}
                  <p class="text-center text-xs text-muted-foreground">
                    {setupProgress.detail}
                  </p>
                {/if}
              {/if}
            </div>
          {/if}

          {#if setupError}
            <div class="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
              {setupError}
            </div>
          {/if}
        </div>

      {:else if currentView === 'workspace-picker'}
        <!-- ============ WORKSPACE PICKER VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors fade-in"
              onclick={() => navigateTo(workspacePickerBackView)}
            >
              <ArrowLeft class="size-4" />
              Back
            </button>
          </div>

          {#if loadingWorkspaces}
            <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground py-12 fade-in">
              <Loader2 class="size-4 animate-spin" />
              Looking for your workspaces…
            </div>
          {:else if workspaceNamespaces.length > 0}
            <div class="text-center space-y-2 fade-in" style="animation-delay: 0.1s">
              <h1 class="text-2xl font-bold tracking-tight text-foreground">
                Your workspaces
              </h1>
              <p class="text-muted-foreground text-sm">
                Pick a workspace to restore.
              </p>
            </div>

            <div class="space-y-2 fade-in" style="animation-delay: 0.15s">
              {#each workspaceNamespaces as ns (ns.id)}
                <button
                  type="button"
                  class="w-full text-left p-4 rounded-lg border border-border transition-colors disabled:opacity-70 disabled:cursor-not-allowed {workspaceAvailableHere(ns)
                    ? 'hover:border-primary/50 hover:bg-secondary/50'
                    : 'border-dashed bg-secondary/30'}"
                  disabled={settingUp || !workspaceAvailableHere(ns)}
                  onclick={() => handlePickNamespace(ns)}
                >
                  <div class="flex items-center gap-3">
                    <Cloud class="size-5 text-muted-foreground shrink-0" />
                    <div class="min-w-0">
                      <div class="font-medium text-sm truncate">
                        {workspaceName(ns)}
                      </div>
                      {#if ns.metadata?.provider}
                        <div class="text-xs text-muted-foreground">
                          via {workspaceProviderLabel(ns)}
                        </div>
                      {/if}
                      {#if !workspaceAvailableHere(ns) && workspaceUnavailableReason(ns)}
                        <div class="text-xs text-muted-foreground mt-1">
                          {workspaceUnavailableReason(ns)}
                        </div>
                      {/if}
                    </div>
                  </div>
                </button>
              {/each}
            </div>

            <div class="text-center fade-in" style="animation-delay: 0.25s">
              <button
                type="button"
                class="text-xs text-muted-foreground hover:text-foreground transition-colors"
                disabled={settingUp}
                onclick={handleWorkspacePickerCreateAction}
              >
                {#if settingUp}
                  Creating…
                {:else if workspacePickerProviderId}
                  Or create a new workspace here
                {:else}
                  Or create a new workspace
                {/if}
              </button>
            </div>

            {#if settingUp}
              <div class="space-y-2 fade-in">
                <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Loader2 class="size-4 animate-spin" />
                  {setupProgress?.message ?? "Restoring workspace…"}
                </div>
                {#if setupProgress}
                  <Progress value={setupProgress.percent} class="h-1.5" />
                  {#if setupProgress.detail}
                    <p class="text-center text-xs text-muted-foreground">
                      {setupProgress.detail}
                    </p>
                  {/if}
                {/if}
              </div>
            {/if}

            {#if setupError}
              <div class="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive fade-in">
                {setupError}
              </div>
            {/if}
          {/if}
        </div>

      {:else if currentView === 'bundles'}
        <!-- ============ BUNDLE CAROUSEL VIEW ============ -->
        {#if loading}
          <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground py-20">
            <Loader2 class="size-4 animate-spin" />
            Loading…
          </div>
        {:else}
          <BundleCarousel
            {bundles}
            {themes}
            deferZoom={true}
            onDeferredSelect={(info) => handleBundleSelected(info, info.pluginOverrides)}
            onSelect={(bundle, overrides) => handleGetStarted(bundle, overrides)}
            onBack={() => navigateTo('main')}
          />
        {/if}
      {/if}

    </div>
  {/key}
</div>

<style>
  @property --orb1-x { syntax: '<percentage>'; initial-value: 20%; inherits: false; }
  @property --orb1-y { syntax: '<percentage>'; initial-value: 25%; inherits: false; }
  @property --orb2-x { syntax: '<percentage>'; initial-value: 75%; inherits: false; }
  @property --orb2-y { syntax: '<percentage>'; initial-value: 65%; inherits: false; }

  @keyframes ambientShift {
    0%   { --orb1-x: 20%; --orb1-y: 25%; --orb2-x: 75%; --orb2-y: 65%; }
    25%  { --orb1-x: 55%; --orb1-y: 40%; --orb2-x: 30%; --orb2-y: 35%; }
    50%  { --orb1-x: 45%; --orb1-y: 65%; --orb2-x: 60%; --orb2-y: 25%; }
    75%  { --orb1-x: 25%; --orb1-y: 55%; --orb2-x: 70%; --orb2-y: 60%; }
    100% { --orb1-x: 20%; --orb1-y: 25%; --orb2-x: 75%; --orb2-y: 65%; }
  }

  .welcome-bg {
    background-color: var(--background);
    background-image:
      radial-gradient(
        ellipse 70% 60% at var(--orb1-x) var(--orb1-y),
        color-mix(in oklch, var(--primary) 12%, transparent) 0%,
        transparent 60%
      ),
      radial-gradient(
        ellipse 60% 70% at var(--orb2-x) var(--orb2-y),
        color-mix(in oklch, var(--ring) 8%, transparent) 0%,
        transparent 60%
      );
    animation: ambientShift 22s ease-in-out infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .welcome-bg { animation: none; }
    .slide-in-right, .slide-in-left { animation: none !important; }
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
  }

  @keyframes slideInRight {
    from { opacity: 0; transform: translateX(40px); }
    to { opacity: 1; transform: translateX(0); }
  }

  @keyframes slideInLeft {
    from { opacity: 0; transform: translateX(-40px); }
    to { opacity: 1; transform: translateX(0); }
  }

  .view-content {
    max-height: 100vh;
    overflow-y: auto;
    padding-top: calc(env(safe-area-inset-top) + var(--titlebar-area-height) + 2rem);
    padding-bottom: calc(env(safe-area-inset-bottom) + 2rem);
  }

  .slide-in-right { animation: slideInRight 0.3s ease-out; }
  .slide-in-left { animation: slideInLeft 0.3s ease-out; }
  .fade-in { animation: fadeIn 0.4s ease-out backwards; }

  .animation-skipped .fade-in {
    animation: none !important;
    opacity: 1 !important;
    transform: none !important;
  }

  :global(.get-started-btn) {
    transition: transform 0.2s ease-out, box-shadow 0.2s ease-out;
  }

  :global(.get-started-btn:hover) {
    transform: scale(1.02);
    box-shadow: 0 4px 20px color-mix(in oklch, var(--primary) 35%, transparent);
  }

  /* ---- Fade out current view before zoom ---- */

  .fading-out {
    animation: viewFadeOut 0.35s ease-in forwards;
  }

  @keyframes viewFadeOut {
    from { opacity: 1; transform: scale(1); }
    to   { opacity: 0; transform: scale(0.97); }
  }

</style>
