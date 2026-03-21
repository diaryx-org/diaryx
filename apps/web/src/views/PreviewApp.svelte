<script lang="ts">
  /**
   * PreviewApp — lightweight themed workspace mockup for onboarding carousel.
   *
   * Mounted instead of App when `?preview` is in the URL. Renders a static
   * workspace layout (sidebar + editor) using the bundle's theme colors.
   * No backend, no plugins, no WASM — just themed HTML.
   */
  import { onMount } from "svelte";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import type { ThemeColorPalette } from "$lib/stores/appearance.types";
  import { FONT_FAMILY_MAP, CONTENT_WIDTH_MAP } from "$lib/stores/appearance.types";

  interface Props {
    bundleId: string;
    darkMode?: boolean;
  }

  let { bundleId, darkMode = false }: Props = $props();

  let loaded = $state(false);

  onMount(async () => {
    if (darkMode) {
      document.documentElement.classList.add("dark");
    }

    try {
      const [bundleReg, themeReg] = await Promise.all([
        fetchBundleRegistry(),
        fetchThemeRegistry().catch(() => ({ themes: [] as ThemeRegistryEntry[] })),
      ]);

      const bundle = bundleReg.bundles.find((b: BundleRegistryEntry) => b.id === bundleId);
      if (!bundle) {
        loaded = true;
        return;
      }

      // Find the theme
      const themeEntry = themeReg.themes.find((t: ThemeRegistryEntry) => t.id === bundle.theme_id);
      if (themeEntry) {
        const palette: ThemeColorPalette = darkMode
          ? themeEntry.theme.colors.dark
          : themeEntry.theme.colors.light;

        // Apply theme CSS variables to :root
        const root = document.documentElement.style;
        for (const [key, value] of Object.entries(palette)) {
          root.setProperty(`--${key}`, value);
        }
      }

      // Apply typography if bundle specifies one
      const typoReg = await import("$lib/marketplace/typographyRegistry")
        .then((m) => m.fetchTypographyRegistry())
        .catch(() => null);

      if (typoReg && bundle.typography_id) {
        const typo = typoReg.typographies?.find(
          (t: any) => t.id === bundle.typography_id,
        );
        if (typo?.typography?.settings) {
          const s = { ...typo.typography.settings, ...(bundle.typography ?? {}) };
          const root = document.documentElement.style;
          if (s.fontFamily && FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]) {
            root.setProperty("--editor-font-family", FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]);
          }
          if (s.baseFontSize) {
            root.setProperty("--editor-font-size", `${s.baseFontSize}px`);
          }
          if (s.lineHeight) {
            root.setProperty("--editor-line-height", String(s.lineHeight));
          }
          if (s.contentWidth && CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]) {
            root.setProperty("--editor-content-max-width", CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]);
          }
        }
      }
    } catch (e) {
      console.warn("[PreviewApp] Failed to load bundle/theme:", e);
    }

    loaded = true;
  });

  // Sample sidebar entries
  const sidebarEntries = [
    { name: "Welcome", icon: "file", active: true, indent: 0 },
    { name: "Getting Started", icon: "file", active: false, indent: 1 },
    { name: "Quick Tips", icon: "file", active: false, indent: 1 },
    { name: "Journal", icon: "folder", active: false, indent: 0 },
    { name: "Today", icon: "file", active: false, indent: 1 },
    { name: "Ideas", icon: "folder", active: false, indent: 0 },
    { name: "Project Notes", icon: "file", active: false, indent: 1 },
    { name: "Reading List", icon: "file", active: false, indent: 1 },
  ];
</script>

<div
  class="preview-root h-full w-full flex bg-background text-foreground overflow-hidden transition-opacity duration-300"
  class:opacity-0={!loaded}
  class:opacity-100={loaded}
>
  <!-- Sidebar -->
  <div class="w-56 shrink-0 flex flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
    <!-- Sidebar header -->
    <div class="px-4 py-4 border-b border-sidebar-border bg-sidebar-accent">
      <div class="flex items-center gap-2">
        <div class="size-5 rounded bg-sidebar-primary opacity-80"></div>
        <span class="text-sm font-semibold text-sidebar-foreground truncate">My Workspace</span>
      </div>
    </div>

    <!-- Sidebar tree -->
    <div class="flex-1 overflow-hidden py-2">
      {#each sidebarEntries as entry}
        <div
          class="flex items-center gap-2 px-3 py-1.5 text-xs cursor-default transition-colors
            {entry.active ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-sidebar-foreground hover:bg-sidebar-accent/50'}"
          style="padding-left: {12 + entry.indent * 16}px"
        >
          {#if entry.icon === "folder"}
            <svg class="size-3.5 shrink-0 opacity-60" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
            </svg>
          {:else}
            <svg class="size-3.5 shrink-0 opacity-60" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
              <polyline points="14 2 14 8 20 8"/>
            </svg>
          {/if}
          <span class="truncate">{entry.name}</span>
        </div>
      {/each}
    </div>
  </div>

  <!-- Editor area -->
  <div class="flex-1 flex flex-col min-w-0 bg-background">
    <!-- Editor toolbar -->
    <div class="flex items-center gap-1 px-4 py-2 border-b border-border">
      {#each ["B", "I", "U", "H₁", "H₂", "⊞", "—"] as tool}
        <div class="px-2 py-1 text-xs text-muted-foreground rounded hover:bg-secondary cursor-default">
          {tool}
        </div>
      {/each}
    </div>

    <!-- Editor content -->
    <div class="flex-1 overflow-hidden">
      <div
        class="mx-auto px-8 py-8"
        style="
          max-width: var(--editor-content-max-width, 65ch);
          font-family: var(--editor-font-family);
          font-size: var(--editor-font-size, 16px);
          line-height: var(--editor-line-height, 1.6);
        "
      >
        <h1 class="text-2xl font-bold text-foreground mb-4">Welcome to Diaryx</h1>
        <p class="text-foreground/90 mb-4">
          Your personal knowledge workspace. Write, organize, and connect your thoughts
          in a single, beautiful interface.
        </p>
        <h2 class="text-lg font-semibold text-foreground mt-6 mb-3">Getting Started</h2>
        <p class="text-foreground/80 mb-3">
          Everything lives in a <span class="px-1.5 py-0.5 rounded text-xs font-medium bg-secondary text-secondary-foreground">tree</span> on
          the left. Create entries, nest them, and link them together.
        </p>
        <ul class="space-y-2 mb-4">
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Organize with nested entries and folders</span>
          </li>
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Rich text editing with markdown shortcuts</span>
          </li>
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Sync across all your devices</span>
          </li>
        </ul>
        <blockquote class="border-l-3 border-primary/40 pl-4 italic text-muted-foreground mb-4">
          "The best time to start writing is now."
        </blockquote>
        <p class="text-foreground/70 text-sm">
          Explore the sidebar to see how entries are organized, or dive in and start writing.
        </p>
      </div>
    </div>
  </div>

  <!-- Right sidebar hint -->
  <div class="w-10 shrink-0 border-l border-border bg-background flex flex-col items-center py-3 gap-3">
    {#each Array(4) as _}
      <div class="size-5 rounded bg-muted"></div>
    {/each}
  </div>
</div>

<style>
  .preview-root {
    /* Ensure theme variables are inherited */
    color: var(--foreground);
    background: var(--background);
  }
</style>
