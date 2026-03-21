<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience.
   *
   * Two primary paths:
   * - "Sign in to get your workspace" → sign in → pick existing workspace or create first
   * - "Continue without an account" → create a local-only workspace
   *
   * Views:
   * - main: Two-button welcome
   * - sign-in: Embedded SignInForm
   * - workspace-picker: List of user's synced workspaces after auth
   * - bundles: Full-screen bundle picker (via "More options")
   */
  import { Button } from "$lib/components/ui/button";
  import { ArrowLeft, Package, Minus, LogIn, Loader2, Ellipsis, Cloud, FolderOpen } from "@lucide/svelte";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import type { NamespaceEntry } from "$lib/auth/authService";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import { listUserWorkspaceNamespaces } from "$lib/auth/authStore.svelte";

  interface Props {
    /** Called to create a local workspace (no account) */
    onGetStarted: (selectedBundle: BundleRegistryEntry | null) => void | Promise<void>;
    /** Called after sign-in when user has no existing workspaces — create first synced workspace */
    onSignInCreateNew: () => void | Promise<void>;
    /** Called when user picks an existing workspace to restore */
    onRestoreWorkspace: (namespace: NamespaceEntry) => void | Promise<void>;
    /** When set, user navigated here from an existing workspace — show a "Return" button */
    returnWorkspaceName?: string | null;
    onReturn?: () => void;
    /** Called when a returning user picks a bundle to create a new workspace */
    onCreateNewWithBundle?: (selectedBundle: BundleRegistryEntry | null) => void | Promise<void>;
  }

  let {
    onGetStarted,
    onSignInCreateNew,
    onRestoreWorkspace,
    returnWorkspaceName = null,
    onReturn,
    onCreateNewWithBundle,
  }: Props = $props();

  // View state machine
  type WelcomeView = 'main' | 'sign-in' | 'workspace-picker' | 'bundles';
  let currentView = $state<WelcomeView>('main');
  let transitionDirection = $state<'forward' | 'back'>('forward');

  // Data
  let bundles = $state<BundleRegistryEntry[]>([]);
  let themes = $state<ThemeRegistryEntry[]>([]);
  let loading = $state(true);
  let selectedBundleId = $state<string>("bundle.default");
  let settingUp = $state(false);

  // Workspace picker state
  let workspaceNamespaces = $state<NamespaceEntry[]>([]);
  let loadingWorkspaces = $state(false);
  let restoringNamespace = $state<string | null>(null);

  async function handleGetStarted(bundle: BundleRegistryEntry | null) {
    settingUp = true;
    try {
      await onGetStarted(bundle);
    } catch {
      settingUp = false;
    }
  }

  async function handleCreateNewWithBundle(bundle: BundleRegistryEntry | null) {
    if (!onCreateNewWithBundle) return;
    settingUp = true;
    try {
      await onCreateNewWithBundle(bundle);
    } catch {
      settingUp = false;
    }
  }

  async function handleSignInComplete() {
    // After auth, fetch the user's workspace namespaces
    loadingWorkspaces = true;
    navigateTo('workspace-picker');
    try {
      workspaceNamespaces = await listUserWorkspaceNamespaces();
    } catch {
      workspaceNamespaces = [];
    } finally {
      loadingWorkspaces = false;
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

  let defaultBundle = $derived(
    bundles.find((b) => b.id === "bundle.default") ?? null,
  );

  let selectedBundle = $derived(
    bundles.find((b) => b.id === selectedBundleId) ?? null,
  );

  let themeMap = $derived(
    new Map(themes.map((t) => [t.id, t])),
  );

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
    transitionDirection = view === 'main' ? 'back' : 'forward';
    currentView = view;
  }

  function getThemeColors(themeId: string): string[] | null {
    if (themeId === 'default') return null;
    const theme = themeMap.get(themeId);
    if (!theme) return null;
    const c = theme.theme.colors.light;
    return [c.primary, c.background, c.accent, c.muted];
  }

  function workspaceName(ns: NamespaceEntry): string {
    return ns.metadata?.name ?? ns.id;
  }
</script>

<div class="flex items-center justify-center min-h-full welcome-bg px-4 overflow-hidden select-none">
  {#key currentView}
    <div class="w-full view-content {transitionDirection === 'forward' ? 'slide-in-right' : 'slide-in-left'}">

      {#if currentView === 'main'}
        <!-- ============ MAIN VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div class="text-center space-y-4">
            <img src="/icon.png" alt="Diaryx" class="size-16 mx-auto fade-in" style="animation-delay: 0s" />
            <h1 class="text-3xl font-bold tracking-tight text-foreground fade-in" style="animation-delay: 0.2s">
              Welcome to Diaryx
            </h1>
            <p class="text-muted-foreground text-sm fade-in" style="animation-delay: 0.4s">
              Your personal knowledge workspace.
            </p>
          </div>

          <div class="space-y-3 fade-in" style="animation-delay: 0.5s">
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
                disabled={loading || settingUp}
                onclick={() => handleGetStarted(defaultBundle)}
              >
                {#if loading || settingUp}
                  <Loader2 class="size-4 animate-spin mr-2" />
                {/if}
                {settingUp ? 'Setting up…' : 'Continue without an account'}
              </Button>
            {/if}
          </div>

          <div class="text-center fade-in" style="animation-delay: 0.7s">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
              onclick={() => navigateTo('bundles')}
            >
              <Ellipsis class="size-3" />
              More options
            </button>
          </div>
        </div>

      {:else if currentView === 'sign-in'}
        <!-- ============ SIGN-IN VIEW ============ -->
        <div class="w-full max-w-sm mx-auto space-y-6">
          <div>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors fade-in"
              onclick={() => navigateTo('main')}
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

            <div class="space-y-2 fade-in" style="animation-delay: 0.2s">
              {#each workspaceNamespaces as ns (ns.id)}
                <button
                  type="button"
                  class="w-full text-left p-4 rounded-lg border border-border hover:border-primary/50 hover:bg-secondary/50 transition-colors disabled:opacity-50"
                  disabled={restoringNamespace !== null}
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
          {:else}
            <!-- No existing workspaces — create first one -->
            <div class="text-center space-y-4 fade-in" style="animation-delay: 0.1s">
              <div class="space-y-2">
                <h1 class="text-2xl font-bold tracking-tight text-foreground">
                  Welcome!
                </h1>
                <p class="text-muted-foreground text-sm">
                  You're signed in. Let's create your first workspace.
                </p>
              </div>

              <Button
                class="w-full get-started-btn"
                disabled={settingUp}
                onclick={handleCreateFirstWorkspace}
              >
                {#if settingUp}
                  <Loader2 class="size-4 animate-spin mr-2" />
                  Creating…
                {:else}
                  <FolderOpen class="size-4 mr-2" />
                  Create workspace
                {/if}
              </Button>
            </div>
          {/if}
        </div>

      {:else if currentView === 'bundles'}
        <!-- ============ BUNDLE PICKER VIEW ============ -->
        <div class="w-full max-w-2xl mx-auto space-y-6">
          <div>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors fade-in"
              onclick={() => navigateTo('main')}
            >
              <ArrowLeft class="size-4" />
              Back
            </button>
          </div>

          <div class="text-center space-y-2 fade-in" style="animation-delay: 0.1s">
            <h1 class="text-2xl font-bold tracking-tight text-foreground">
              Choose your setup
            </h1>
            <p class="text-muted-foreground text-sm">
              Pick a bundle to configure your workspace with themes, plugins, and starter content.
            </p>
          </div>

          {#if loading}
            <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground py-8 fade-in" style="animation-delay: 0.2s">
              <Loader2 class="size-4 animate-spin" />
              Loading bundles...
            </div>
          {:else}
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 fade-in" style="animation-delay: 0.2s">
              {#each bundles as bundle (bundle.id)}
                {@const colors = getThemeColors(bundle.theme_id)}
                <button
                  type="button"
                  class="text-left p-4 rounded-lg border-2 transition-colors {selectedBundleId === bundle.id ? 'border-primary bg-secondary' : 'border-border hover:border-muted-foreground/50'}"
                  onclick={() => { selectedBundleId = bundle.id; }}
                >
                  <div class="space-y-2">
                    <!-- Theme color swatches -->
                    {#if colors}
                      <div class="flex gap-1.5">
                        {#each colors as color}
                          <span
                            class="size-4 rounded-full border border-border/50"
                            style="background-color: {color}"
                          ></span>
                        {/each}
                      </div>
                    {:else}
                      <div class="flex gap-1.5">
                        <span class="size-4 rounded-full border border-border/50 bg-foreground/10"></span>
                        <span class="size-4 rounded-full border border-border/50 bg-primary/30"></span>
                        <span class="size-4 rounded-full border border-border/50 bg-muted"></span>
                      </div>
                    {/if}

                    <div>
                      <div class="font-medium text-sm flex items-center gap-2">
                        {#if bundle.id === 'bundle.minimal'}
                          <Minus class="size-4 shrink-0 {selectedBundleId === bundle.id ? 'text-primary' : 'text-muted-foreground'}" />
                        {:else}
                          <Package class="size-4 shrink-0 {selectedBundleId === bundle.id ? 'text-primary' : 'text-muted-foreground'}" />
                        {/if}
                        {bundle.name}
                        {#if bundle.id === 'bundle.default'}
                          <span class="text-xs text-primary">Recommended</span>
                        {/if}
                      </div>
                      <p class="text-xs text-muted-foreground mt-1">{bundle.summary}</p>
                    </div>

                    <div class="text-xs text-muted-foreground/60">
                      {#if bundle.plugins.length > 0}
                        {bundle.plugins.length} plugin{bundle.plugins.length === 1 ? '' : 's'}
                      {:else}
                        No plugins
                      {/if}
                    </div>
                  </div>
                </button>
              {/each}
            </div>

            <div class="fade-in" style="animation-delay: 0.3s">
              <Button
                class="w-full get-started-btn"
                disabled={settingUp}
                onclick={() => {
                  if (returnWorkspaceName && onCreateNewWithBundle) {
                    handleCreateNewWithBundle(selectedBundle);
                  } else {
                    handleGetStarted(selectedBundle);
                  }
                }}
              >
                {#if settingUp}
                  <Loader2 class="size-4 animate-spin mr-2" />
                  Setting up…
                {:else if returnWorkspaceName}
                  {#if selectedBundle}
                    New Workspace with {selectedBundle.name}
                  {:else}
                    New Workspace
                  {/if}
                {:else}
                  {#if selectedBundle}
                    Get Started with {selectedBundle.name}
                  {:else}
                    Get Started
                  {/if}
                {/if}
              </Button>
            </div>
          {/if}
        </div>
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
</style>
