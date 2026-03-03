<script lang="ts">
  /**
   * PluginIframe — renders plugin-provided HTML in a sandboxed iframe.
   *
   * On mount, calls the plugin's `get_component_html` command to get
   * the HTML content, then creates a blob URL and renders it in an iframe.
   * A postMessage bridge allows the iframe to dispatch plugin commands
   * and receive responses/events from the host.
   */
  import { onMount, onDestroy } from "svelte";
  import { dispatchCommand } from "$lib/plugins/browserPluginManager.svelte";
  import { getThemeStore } from "@/models/stores";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import type { EntryData } from "$lib/backend/interface";

  interface Props {
    pluginId: string;
    componentId: string;
    entry?: EntryData | null;
    onHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
  }

  let { pluginId, componentId, entry = null, onHostAction }: Props = $props();

  let iframeEl: HTMLIFrameElement | undefined = $state();
  let blobUrl: string | null = $state(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let iframeReady = $state(false);

  const themeStore = getThemeStore();
  const appearanceStore = getAppearanceStore();

  /** CSS variable names forwarded to plugin iframes for theming. */
  const CSS_VAR_NAMES = [
    "--background", "--foreground",
    "--card", "--card-foreground",
    "--popover", "--popover-foreground",
    "--primary", "--primary-foreground",
    "--secondary", "--secondary-foreground",
    "--muted", "--muted-foreground",
    "--accent", "--accent-foreground",
    "--destructive",
    "--border", "--input", "--ring", "--radius",
    "--sidebar", "--sidebar-foreground",
    "--sidebar-primary", "--sidebar-primary-foreground",
    "--sidebar-accent", "--sidebar-accent-foreground",
    "--sidebar-border", "--sidebar-ring",
    "--editor-font-family", "--editor-font-size",
    "--editor-line-height", "--editor-content-max-width",
  ];

  /** Read current CSS variable values from the document. */
  function collectCssVars(): Record<string, string> {
    const computed = getComputedStyle(document.documentElement);
    const vars: Record<string, string> = {};
    for (const name of CSS_VAR_NAMES) {
      const value = computed.getPropertyValue(name).trim();
      if (value) vars[name] = value;
    }
    return vars;
  }

  onMount(async () => {
    try {
      const result = await dispatchCommand(pluginId, "get_component_html", {
        component_id: componentId,
      });
      // data may be a string directly, or nested if the response shape differs
      const html = typeof result.data === "string"
        ? result.data
        : typeof (result as any).data?.response === "string"
          ? (result as any).data.response
          : null;
      if (!result.success || !html) {
        console.error("[PluginIframe] get_component_html failed: success=%s, error=%s, data type=%s", result.success, result.error, typeof result.data);
        error = result.error ?? "Failed to load component HTML";
        loading = false;
        return;
      }
      const blob = new Blob([html], { type: "text/html" });
      blobUrl = URL.createObjectURL(blob);
      loading = false;
    } catch (e) {
      console.error("[PluginIframe] get_component_html threw:", e);
      error = e instanceof Error ? e.message : String(e);
      loading = false;
    }
  });

  onDestroy(() => {
    if (blobUrl) {
      URL.revokeObjectURL(blobUrl);
    }
  });

  // Send initial theme/context to iframe once loaded
  function handleIframeLoad() {
    if (!iframeEl?.contentWindow) return;
    iframeReady = true;
    iframeEl.contentWindow.postMessage(
      {
        type: "init",
        theme: themeStore.isDark ? "dark" : "light",
        cssVars: collectCssVars(),
        entry: entry ? { path: entry.path, title: entry.title, content: entry.content } : null,
      },
      "*"
    );
  }

  // Re-send theme data when light/dark mode or appearance preset changes
  $effect(() => {
    // Track reactive dependencies
    const isDark = themeStore.isDark;
    void appearanceStore.appearance;

    if (!iframeReady || !iframeEl?.contentWindow) return;
    iframeEl.contentWindow.postMessage(
      {
        type: "theme-update",
        theme: isDark ? "dark" : "light",
        cssVars: collectCssVars(),
      },
      "*"
    );
  });

  // Send entry-changed event when the current entry changes
  $effect(() => {
    const e = entry;
    if (!iframeReady || !iframeEl?.contentWindow) return;
    iframeEl.contentWindow.postMessage(
      {
        type: "plugin-event",
        event: "entry-changed",
        data: e ? { path: e.path, title: e.title, content: e.content } : null,
      },
      "*"
    );
  });

  // Listen for messages from the iframe
  function handleMessage(event: MessageEvent) {
    // Only handle messages from our iframe
    if (!iframeEl || event.source !== iframeEl.contentWindow) return;

    const data = event.data;
    if (!data || typeof data !== "object") return;

    if (data.type === "plugin-command") {
      const { command, params, requestId } = data;
      dispatchCommand(pluginId, command, params).then((result) => {
        iframeEl?.contentWindow?.postMessage(
          {
            type: "plugin-response",
            requestId,
            result,
          },
          "*"
        );
      });
      return;
    }

    if (data.type === "host-action") {
      const { action, requestId } = data;
      Promise.resolve()
        .then(() => {
          if (!onHostAction) {
            throw new Error("Host actions are not available in this context");
          }
          return onHostAction(action ?? {});
        })
        .then((result) => {
          iframeEl?.contentWindow?.postMessage(
            {
              type: "host-action-response",
              requestId,
              success: true,
              data: result ?? null,
            },
            "*"
          );
        })
        .catch((e) => {
          iframeEl?.contentWindow?.postMessage(
            {
              type: "host-action-response",
              requestId,
              success: false,
              error: e instanceof Error ? e.message : String(e),
            },
            "*"
          );
        });
    }
  }

  $effect(() => {
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  });

  /**
   * Send a plugin event to the iframe.
   */
  export function sendEvent(event: string, data: unknown) {
    iframeEl?.contentWindow?.postMessage(
      { type: "plugin-event", event, data },
      "*"
    );
  }
</script>

<div class="h-full w-full flex flex-col">
  {#if loading}
    <div class="flex-1 flex items-center justify-center text-sm text-muted-foreground">
      Loading...
    </div>
  {:else if error}
    <div class="flex-1 flex items-center justify-center text-sm text-destructive p-4 text-center">
      {error}
    </div>
  {:else if blobUrl}
    <iframe
      bind:this={iframeEl}
      src={blobUrl}
      sandbox="allow-scripts"
      class="flex-1 w-full border-0"
      title="Plugin: {componentId}"
      onload={handleIframeLoad}
    ></iframe>
  {/if}
</div>
