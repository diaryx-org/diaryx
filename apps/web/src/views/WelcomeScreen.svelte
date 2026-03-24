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
  import { ArrowLeft, LogIn, Loader2, Cloud, Download, Check, HardDrive, Lock } from "@lucide/svelte";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import type { NamespaceEntry } from "$lib/auth/authService";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import AnimatedLogo from "./AnimatedLogo.svelte";
  import BundleCarousel, { type BundleSelectInfo } from "./BundleCarousel.svelte";
  import { isAuthenticated, listUserWorkspaceNamespaces } from "$lib/auth/authStore.svelte";
  import {
    fetchPluginRegistry,
    getRegistryWorkspaceProviders,
    type RegistryPlugin,
    type RegistryWorkspaceProvider,
  } from "$lib/plugins/pluginRegistry";
  import { installRegistryPlugin } from "$lib/plugins/pluginInstallService";

  interface Props {
    /** Called to create a local workspace (no account) */
    onGetStarted: (selectedBundle: BundleRegistryEntry | null) => void | Promise<void>;
    /** Called after sign-in when user has no existing workspaces — create first synced workspace */
    onSignInCreateNew: () => void | Promise<void>;
    /** Called when user picks an existing workspace to restore */
    onRestoreWorkspace: (namespace: NamespaceEntry) => void | Promise<void>;
    /** Called when user picks a provider for a new workspace */
    onCreateWithProvider: (
      bundle: BundleRegistryEntry | null,
      providerPluginId: string | null,
    ) => void | Promise<void>;
    /** Called to show the launch zoom overlay — App.svelte owns rendering */
    onLaunch?: (info: BundleSelectInfo) => void;
    /** When set, user navigated here from an existing workspace — show a "Return" button */
    returnWorkspaceName?: string | null;
    onReturn?: () => void;
  }

  let {
    onGetStarted,
    onSignInCreateNew,
    onRestoreWorkspace,
    onCreateWithProvider,
    onLaunch,
    returnWorkspaceName = null,
    onReturn,
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

  // Bundle + provider choice state
  let selectedBundle = $state<BundleRegistryEntry | null>(null);
  let bundleProviders = $state<RegistryWorkspaceProvider[]>([]);
  let signInForProvider = $state<string | null>(null);
  let checkingProvider = $state(false);

  // Deferred zoom animation state
  let launchInfo = $state<BundleSelectInfo | null>(null);
  let fadingOut = $state(false);

  // Workspace picker state
  let workspaceNamespaces = $state<NamespaceEntry[]>([]);
  let loadingWorkspaces = $state(false);
  let restoringNamespace = $state<string | null>(null);

  // Provider plugin install state (for workspace-picker)
  let providerPlugin = $state<RegistryPlugin | null>(null);
  let providerPluginInstalled = $state(false);
  let installingProviderPlugin = $state(false);
  let providerPluginError = $state<string | null>(null);

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
    await callback();
  }

  async function handleGetStarted(bundle: BundleRegistryEntry | null) {
    settingUp = true;
    try {
      await playZoomThen(() => onGetStarted(bundle));
    } catch {
      settingUp = false;
    }
  }

  async function handleBundleSelected(info: BundleSelectInfo) {
    launchInfo = info;
    const bundle = info.bundle;

    selectedBundle = bundle;
    const pluginIds = bundle.plugins.map((p) => p.plugin_id);

    try {
      const registry = await fetchPluginRegistry();
      bundleProviders = getRegistryWorkspaceProviders(registry.plugins, pluginIds);
    } catch {
      bundleProviders = [];
    }

    if (bundleProviders.length === 0) {
      await handleGetStarted(bundle);
    } else {
      navigateTo('provider-choice');
    }
  }

  async function handleProviderSelected(provider: RegistryWorkspaceProvider) {
    if (!isAuthenticated()) {
      signInForProvider = provider.pluginId;
      navigateTo('sign-in');
      return;
    }

    await checkProviderNamespaces(provider.pluginId);
  }

  async function checkProviderNamespaces(providerPluginId: string) {
    checkingProvider = true;
    try {
      const allNamespaces = await listUserWorkspaceNamespaces();
      const providerNs = allNamespaces.filter(
        (ns) => ns.metadata?.provider === providerPluginId,
      );

      if (providerNs.length > 0) {
        workspaceNamespaces = providerNs;
        await fetchProviderPlugin(providerPluginId);
        navigateTo('workspace-picker');
      } else {
        settingUp = true;
        try {
          await playZoomThen(() => onCreateWithProvider(selectedBundle, providerPluginId));
        } catch {
          settingUp = false;
        }
      }
    } catch {
      settingUp = true;
      try {
        await playZoomThen(() => onCreateWithProvider(selectedBundle, providerPluginId));
      } catch {
        settingUp = false;
      }
    } finally {
      checkingProvider = false;
    }
  }

  async function handleSignInComplete() {
    if (signInForProvider) {
      // User signed in specifically to use a provider
      const providerPluginId = signInForProvider;
      signInForProvider = null;
      await checkProviderNamespaces(providerPluginId);
      return;
    }

    // Original flow: signed in from main screen
    loadingWorkspaces = true;
    navigateTo('workspace-picker');
    try {
      workspaceNamespaces = await listUserWorkspaceNamespaces();
    } catch {
      workspaceNamespaces = [];
    } finally {
      loadingWorkspaces = false;
      if (workspaceNamespaces.length === 0) {
        navigateTo('bundles');
      } else {
        // Determine provider plugin(s) needed from namespace metadata
        const providerIds = new Set(
          workspaceNamespaces
            .map((ns) => ns.metadata?.provider)
            .filter((p): p is string => typeof p === "string"),
        );
        const firstProvider = providerIds.values().next().value ?? "diaryx.sync";
        await fetchProviderPlugin(firstProvider);
      }
    }
  }

  async function handleRestoreWorkspace(ns: NamespaceEntry) {
    restoringNamespace = ns.id;
    try {
      await onRestoreWorkspace(ns);
    } catch {
      restoringNamespace = null;
    }
  }

  async function handleCreateFirstWorkspace() {
    settingUp = true;
    try {
      await onSignInCreateNew();
    } catch {
      settingUp = false;
    }
  }

  async function fetchProviderPlugin(pluginId: string) {
    try {
      const registry = await fetchPluginRegistry();
      providerPlugin = registry.plugins.find((p) => p.id === pluginId) ?? null;
    } catch {
      providerPlugin = null;
    }
  }

  async function handleInstallProviderPlugin() {
    if (!providerPlugin) return;
    installingProviderPlugin = true;
    providerPluginError = null;
    try {
      await installRegistryPlugin(providerPlugin);
      providerPluginInstalled = true;
    } catch (e) {
      providerPluginError = e instanceof Error ? e.message : "Installation failed";
    } finally {
      installingProviderPlugin = false;
    }
  }

  $effect(() => {
    loadData();
  });

  async function loadData() {
    loading = true;
    try {
      const [bundleReg, themeReg] = await Promise.all([
        fetchBundleRegistry(),
        fetchThemeRegistry().catch(() => ({ themes: [] as ThemeRegistryEntry[] })),
      ]);
      bundles = bundleReg.bundles;
      themes = themeReg.themes;
    } catch {
      bundles = [];
      themes = [];
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
</script>

<div class="flex items-center justify-center min-h-full welcome-bg px-4 overflow-hidden select-none"
  class:fading-out={fadingOut}
>
  {#key currentView}
    <div class="w-full view-content {transitionDirection === 'forward' ? 'slide-in-right' : 'slide-in-left'}">

      {#if currentView === 'main'}
        <!-- ============ MAIN VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div class="text-center space-y-4">
            <div class="mx-auto size-24 fade-in" style="animation-delay: 0s">
              <AnimatedLogo size={96} />
            </div>
            <h1 class="text-3xl font-bold tracking-tight text-foreground fade-in" style="animation-delay: 2.0s">
              Welcome to Diaryx
            </h1>
            <p class="text-muted-foreground text-sm fade-in" style="animation-delay: 2.2s">
              Your writing. Worth sharing.
            </p>
          </div>

          <div class="space-y-3 fade-in" style="animation-delay: 2.4s">
            {#if returnWorkspaceName && onReturn}
              <Button
                class="w-full get-started-btn"
                onclick={onReturn}
              >
                <ArrowLeft class="size-4 mr-2" />
                Return to {returnWorkspaceName}
              </Button>
            {:else}
              <Button
                class="w-full get-started-btn"
                onclick={() => navigateTo('sign-in')}
              >
                <LogIn class="size-4 mr-2" />
                Sign in to get your workspace
              </Button>

              <Button
                variant="ghost"
                class="w-full text-muted-foreground"
                disabled={loading}
                onclick={() => navigateTo('bundles')}
              >
                Continue without an account
              </Button>
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
              onclick={() => navigateTo('bundles')}
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
              onclick={() => handleGetStarted(selectedBundle)}
            >
              <div class="flex items-center gap-3">
                <HardDrive class="size-5 text-muted-foreground shrink-0" />
                <div class="min-w-0">
                  <div class="font-medium text-sm">This device only</div>
                  <div class="text-xs text-muted-foreground">
                    Your workspace stays on this device.
                  </div>
                </div>
              </div>
            </button>

            {#each bundleProviders as provider (provider.pluginId)}
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
                  {#if !isAuthenticated()}
                    <Lock class="size-4 text-muted-foreground shrink-0" />
                  {/if}
                </div>
              </button>
            {/each}
          </div>

          {#if settingUp}
            <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 class="size-4 animate-spin" />
              Setting up…
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
              onclick={() => navigateTo('sign-in')}
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
                Pick a workspace to restore on this device.
              </p>
            </div>

            <!-- Provider plugin install card -->
            {#if !providerPluginInstalled}
              <div class="rounded-lg border border-border bg-secondary/30 p-4 space-y-3 fade-in" style="animation-delay: 0.15s">
                <div class="flex items-start gap-3">
                  <Download class="size-5 text-primary shrink-0 mt-0.5" />
                  <div class="space-y-1">
                    <div class="font-medium text-sm">
                      {providerPlugin?.name ?? 'Sync'} plugin required
                    </div>
                    <p class="text-xs text-muted-foreground">
                      Install to restore and keep your workspaces in sync.
                    </p>
                  </div>
                </div>
                {#if providerPluginError}
                  <p class="text-xs text-destructive">{providerPluginError}</p>
                {/if}
                <Button
                  class="w-full"
                  size="sm"
                  disabled={!providerPlugin || installingProviderPlugin}
                  onclick={handleInstallProviderPlugin}
                >
                  {#if installingProviderPlugin}
                    <Loader2 class="size-4 animate-spin mr-2" />
                    Installing…
                  {:else if !providerPlugin}
                    <Loader2 class="size-4 animate-spin mr-2" />
                    Loading…
                  {:else}
                    <Download class="size-4 mr-2" />
                    Install {providerPlugin.name} Plugin
                  {/if}
                </Button>
              </div>
            {:else}
              <div class="rounded-lg border border-border bg-secondary/30 p-3 flex items-center gap-2 text-sm text-muted-foreground fade-in" style="animation-delay: 0.15s">
                <Check class="size-4 text-primary shrink-0" />
                {providerPlugin?.name ?? 'Sync'} plugin installed
              </div>
            {/if}

            <div class="space-y-2 fade-in" style="animation-delay: 0.2s">
              {#each workspaceNamespaces as ns (ns.id)}
                <button
                  type="button"
                  class="w-full text-left p-4 rounded-lg border border-border transition-colors disabled:opacity-50
                    {providerPluginInstalled ? 'hover:border-primary/50 hover:bg-secondary/50' : 'opacity-60 cursor-not-allowed'}"
                  disabled={!providerPluginInstalled || restoringNamespace !== null}
                  onclick={() => handleRestoreWorkspace(ns)}
                >
                  <div class="flex items-center gap-3">
                    {#if restoringNamespace === ns.id}
                      <Loader2 class="size-5 animate-spin text-primary shrink-0" />
                    {:else}
                      <Cloud class="size-5 text-muted-foreground shrink-0" />
                    {/if}
                    <div class="min-w-0">
                      <div class="font-medium text-sm truncate">
                        {workspaceName(ns)}
                      </div>
                      {#if ns.metadata?.provider}
                        <div class="text-xs text-muted-foreground">
                          via {ns.metadata.provider}
                        </div>
                      {/if}
                    </div>
                  </div>
                </button>
              {/each}
            </div>

            <div class="text-center fade-in" style="animation-delay: 0.3s">
              <button
                type="button"
                class="text-xs text-muted-foreground hover:text-foreground transition-colors"
                disabled={settingUp}
                onclick={handleCreateFirstWorkspace}
              >
                {settingUp ? 'Creating…' : 'Or create a new workspace'}
              </button>
            </div>
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
            onDeferredSelect={(info) => handleBundleSelected(info)}
            onSelect={(bundle) => handleGetStarted(bundle)}
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
