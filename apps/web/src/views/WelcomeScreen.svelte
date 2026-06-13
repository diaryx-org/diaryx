<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience.
   *
   * Views:
   * - main: Folder-first workspace setup
   * - sign-in: Embedded SignInForm
   * - bundles: Full-screen bundle picker
   */
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Progress } from "$lib/components/ui/progress";
  import { ArrowLeft, Loader2, FileText, FolderOpen, Plus } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import AnimatedLogo from "./AnimatedLogo.svelte";
  import BundleCarousel, { type BundleSelectInfo, type PluginOverride } from "./BundleCarousel.svelte";
  import { isAuthenticated } from "$lib/auth/authStore.svelte";

  interface Props {
    /** Called to choose a folder, opening an existing workspace or initializing a new one. Returns false when cancelled. */
    onChooseFolderWorkspace: (
      selectedBundle: BundleRegistryEntry | null,
      pluginOverrides?: PluginOverride[],
      onProgress?: (progress: { percent: number; message: string; detail?: string }) => void,
    ) => boolean | void | Promise<boolean | void>;
    /** Called to open one Markdown root file when folder access is unavailable. Returns false when cancelled. */
    onOpenFileNavigation?: (
      onProgress?: (progress: { percent: number; message: string; detail?: string }) => void,
    ) => boolean | void | Promise<boolean | void>;
    /**
     * Whether the runtime can pick a real folder (native app, or a browser with
     * File System Access). When false (e.g. Safari/Firefox), the primary action
     * creates a private in-browser workspace instead of picking a folder.
     */
    canPickFolder?: boolean;
    /** Called to show the launch zoom overlay — App.svelte owns rendering */
    onLaunch?: (info: BundleSelectInfo) => void;
    /** When set, user navigated here from an existing workspace — show a "Return" button */
    returnWorkspaceName?: string | null;
    onReturn?: () => void;
    /** When set, jump directly to a specific view on mount. */
    initialView?: WelcomeView | null;
  }

  let {
    onChooseFolderWorkspace,
    onOpenFileNavigation,
    onLaunch,
    returnWorkspaceName = null,
    onReturn,
    initialView = null,
    canPickFolder = true,
  }: Props = $props();

  // View state machine
  type WelcomeView = 'main' | 'sign-in' | 'bundles';
  let currentView = $state<WelcomeView>('main');
  let transitionDirection = $state<'forward' | 'back'>('forward');

  // Data
  let bundles = $state<BundleRegistryEntry[]>([]);
  let themes = $state<ThemeRegistryEntry[]>([]);
  let loading = $state(true);
  let settingUp = $state(false);
  let setupProgress = $state<{ percent: number; message: string; detail?: string | null } | null>(null);
  let setupError = $state<string | null>(null);

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

  async function playZoomThen<T>(callback: () => T | Promise<T>): Promise<T> {
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
      return await callback();
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

  async function handleChooseFolderWorkspace(bundle: BundleRegistryEntry | null, overrides?: PluginOverride[]) {
    beginSetup(canPickFolder ? "Choose a folder..." : "Creating workspace...");
    try {
      const run = () => onChooseFolderWorkspace(bundle, overrides, updateSetupProgress);
      const completed = launchInfo ? await playZoomThen(run) : await run();
      if (completed === false) {
        settingUp = false;
        setupProgress = null;
        fadingOut = false;
        launchInfo = null;
      }
    } catch (e) {
      failSetup(e, "Failed to set up workspace");
    }
  }

  async function handleOpenFileNavigation() {
    if (!onOpenFileNavigation) return;

    beginSetup("Choose a file...");
    try {
      const completed = await onOpenFileNavigation(updateSetupProgress);
      if (completed === false) {
        settingUp = false;
        setupProgress = null;
        fadingOut = false;
      }
    } catch (e) {
      failSetup(e, "Failed to open file");
    }
  }

  async function handleBundleSelected(info: BundleSelectInfo, overrides?: PluginOverride[]) {
    launchInfo = info;
    await handleChooseFolderWorkspace(info.bundle, overrides);
  }

  export async function handleSignInComplete() {
    navigateTo('main');
  }

  $effect(() => {
    loadData();
  });

  // Run once on mount — honour initialView if provided, otherwise auto-navigate
  onMount(() => {
    if (initialView && initialView !== 'main') {
      navigateTo(initialView);
    }
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
            <Button
              class="w-full get-started-btn"
              disabled={!animationDone || settingUp}
              onclick={() => handleChooseFolderWorkspace(null)}
            >
              {#if canPickFolder}
                <FolderOpen class="size-4 mr-2" />
                Choose workspace folder
              {:else}
                <Plus class="size-4 mr-2" />
                Create workspace
              {/if}
            </Button>

            {#if onOpenFileNavigation}
              <Button
                class="w-full"
                variant="outline"
                disabled={!animationDone || settingUp}
                onclick={handleOpenFileNavigation}
              >
                <FileText class="size-4 mr-2" />
                Open single file
              </Button>
            {/if}

            <button
              type="button"
              class="w-full text-xs text-muted-foreground/70 hover:text-foreground transition-colors disabled:opacity-50"
              disabled={!animationDone || settingUp}
              onclick={() => navigateTo('bundles')}
            >
              Choose a starter workspace
            </button>

            {#if !isAuthenticated()}
              <button
                type="button"
                class="w-full text-xs text-muted-foreground/70 hover:text-foreground transition-colors disabled:opacity-50"
                disabled={!animationDone || settingUp}
                onclick={() => navigateTo('sign-in')}
              >
                Already have an account? Sign in
              </button>
            {/if}

            {#if returnWorkspaceName && onReturn}
              <div class="flex items-center justify-center gap-2 w-full">
                <Button
                  variant="ghost"
                  class="text-muted-foreground"
                  disabled={!animationDone || settingUp}
                  onclick={onReturn}
                >
                  <ArrowLeft class="size-4 mr-2" />
                  Return to {returnWorkspaceName}
                </Button>
              </div>
            {/if}

            {#if settingUp}
              <div class="space-y-2 pt-2">
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
              Manage your Diaryx account.
            </p>
          </div>

          <div class="fade-in" style="animation-delay: 0.2s">
            <SignInForm compact={true} onAuthenticated={() => handleSignInComplete()} />
          </div>
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
            onSelect={(bundle, overrides) => handleChooseFolderWorkspace(bundle, overrides)}
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
