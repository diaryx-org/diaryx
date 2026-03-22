import "./app.css";
import { mount } from "svelte";

if (import.meta.env.DEV && typeof window !== "undefined") {
  const { protocol, hostname, port, pathname, search, hash } = window.location;
  const isLocalHttp = protocol === "http:" || protocol === "https:";
  if (isLocalHttp && (hostname === "127.0.0.1" || hostname === "[::1]")) {
    const target = `${protocol}//localhost${port ? `:${port}` : ""}${pathname}${search}${hash}`;
    window.location.replace(target);
  }
}

const target = document.getElementById("app");

if (target) {
  target.innerHTML = "";

  const params = new URLSearchParams(window.location.search);
  if (params.has("preview")) {
    // Preview mode — real app with a mock backend (no WASM/Workers).
    // Used by the onboarding carousel via iframe.
    setupPreview(target, params);
  } else {
    import("./App.svelte").then(({ default: App }) => {
      mount(App, { target });
    });
  }
}

async function setupPreview(target: HTMLElement, params: URLSearchParams) {
  const _g = globalThis as any;
  _g.__diaryx_preview = true;

  // Create and register mock backend as the singleton
  const { getMockBackend } = await import("./lib/backend/mockBackend");
  const backend = await getMockBackend();
  _g.__diaryx_backendInstance = backend as import("./lib/backend/interface").Backend;

  // Apply bundle theme before mounting
  const bundleId = params.get("bundle") ?? "bundle.default";
  const darkMode = params.get("dark") === "1";
  if (darkMode) {
    document.documentElement.classList.add("dark");
  }
  const bundle = await applyPreviewTheme(bundleId, darkMode);

  // Load starter workspace content into the mock backend
  if (bundle?.starter_workspace_id) {
    await loadStarterWorkspace(backend, bundle.starter_workspace_id);
  }

  // Mount the real app
  const { default: App } = await import("./App.svelte");
  mount(App, { target });
}

async function applyPreviewTheme(bundleId: string, darkMode: boolean): Promise<any | null> {
  try {
    const [{ fetchBundleRegistry }, { fetchThemeRegistry }] = await Promise.all([
      import("./lib/marketplace/bundleRegistry"),
      import("./lib/marketplace/themeRegistry"),
    ]);

    const [bundleReg, themeReg] = await Promise.all([
      fetchBundleRegistry(),
      fetchThemeRegistry().catch(() => ({ themes: [] as any[] })),
    ]);

    const bundle = bundleReg.bundles.find((b: any) => b.id === bundleId);
    if (!bundle) return null;

    const themeEntry = themeReg.themes.find((t: any) => t.id === bundle.theme_id);
    if (themeEntry) {
      const palette = darkMode ? themeEntry.theme.colors.dark : themeEntry.theme.colors.light;
      const root = document.documentElement.style;
      for (const [key, value] of Object.entries(palette)) {
        root.setProperty(`--${key}`, value as string);
      }
    }

    // Apply typography
    const { fetchTypographyRegistry } = await import("./lib/marketplace/typographyRegistry");
    const { FONT_FAMILY_MAP, CONTENT_WIDTH_MAP } = await import("./lib/stores/appearance.types");
    const typoReg = await fetchTypographyRegistry().catch(() => null);

    if (typoReg && bundle.typography_id) {
      const typo = (typoReg as any).typographies?.find((t: any) => t.id === bundle.typography_id);
      if (typo?.typography?.settings) {
        const s = { ...typo.typography.settings, ...(bundle.typography ?? {}) };
        const root = document.documentElement.style;
        if (s.fontFamily && FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]) {
          root.setProperty("--editor-font-family", FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]);
        }
        if (s.baseFontSize) root.setProperty("--editor-font-size", `${s.baseFontSize}px`);
        if (s.lineHeight) root.setProperty("--editor-line-height", String(s.lineHeight));
        if (s.contentWidth && CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]) {
          root.setProperty("--editor-content-max-width", CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]);
        }
      }
    }
    return bundle;
  } catch (e) {
    console.warn("[Preview] Failed to apply theme:", e);
    return null;
  }
}

async function loadStarterWorkspace(backend: import("./lib/backend/mockBackend").MockBackend, starterWorkspaceId: string) {
  try {
    const { fetchStarterWorkspaceRegistry, } = await import("./lib/marketplace/starterWorkspaceRegistry");
    const { fetchStarterWorkspaceZip } = await import("./lib/marketplace/starterWorkspaceApply");
    const JSZip = (await import("jszip")).default;

    const registry = await fetchStarterWorkspaceRegistry();
    const starter = registry.starters.find((s: any) => s.id === starterWorkspaceId);
    if (!starter?.artifact) return;

    const blob = await fetchStarterWorkspaceZip(starter);
    const zip = await JSZip.loadAsync(blob);
    const files = new Map<string, string>();

    for (const [zipPath, zipEntry] of Object.entries(zip.files)) {
      if (zipEntry.dir) continue;
      if (!zipPath.endsWith(".md")) continue;
      const content = await zipEntry.async("string");
      // Normalize path: strip leading slashes, prefix with "workspace/"
      const normalizedPath = "workspace/" + zipPath.replace(/^\/+/, "");
      files.set(normalizedPath, content);
    }

    if (files.size > 0) {
      backend.loadFiles(files);
    }
  } catch (e) {
    console.warn("[Preview] Failed to load starter workspace:", e);
  }
}
