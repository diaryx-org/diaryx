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
  import {
    dispatchCommand,
    getPlugin as getBrowserPlugin,
  } from "$lib/plugins/browserPluginManager.svelte";
  import { openOauthWindow } from "$lib/plugins/oauthWindow";
  import {
    getCurrentWorkspaceId,
    setPluginMetadata,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { getThemeStore } from "@/models/stores";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import type { EntryData } from "$lib/backend/interface";
  import type { Api } from "$lib/backend/api";
  import { getAuthState, getToken } from "$lib/auth";

  interface Props {
    pluginId: string;
    componentId: string;
    entry?: EntryData | null;
    api?: Api | null;
    onHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
  }

  let {
    pluginId,
    componentId,
    entry = null,
    api = null,
    onHostAction,
  }: Props = $props();

  let iframeEl: HTMLIFrameElement | undefined = $state();
  let blobUrl: string | null = $state(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let iframeReady = $state(false);
  const COMPONENT_LOAD_TIMEOUT_MS = 20000;
  type PluginCommandResult = {
    success: boolean;
    data?: unknown;
    error?: string;
  };
  type HostActionEnvelope = {
    host_action: {
      type: string;
      payload?: unknown;
    };
    follow_up?: {
      command: string;
      params?: Record<string, unknown>;
    };
  };
  type WorkspaceMetadataPatchEnvelope = {
    workspace_metadata_patch: {
      plugin_id?: string;
      data: Record<string, unknown> | null;
    };
  };

  const themeStore = getThemeStore();
  const appearanceStore = getAppearanceStore();

  function cloneForPostMessage<T>(value: T): T | null {
    try {
      return structuredClone(value);
    } catch {
      try {
        return JSON.parse(JSON.stringify(value)) as T;
      } catch {
        return null;
      }
    }
  }

  function postToIframe(message: unknown) {
    const win = iframeEl?.contentWindow;
    if (!win) return;
    win.postMessage(cloneForPostMessage(message), "*");
  }

  function withManagedContext(command: string, params: unknown): unknown {
    const baseParams =
      params && typeof params === "object" && !Array.isArray(params)
        ? { ...(params as Record<string, unknown>) }
        : {};

    if (
      pluginId !== "diaryx.ai" ||
      (command !== "chat" && command !== "chat_continue")
    ) {
      return baseParams;
    }

    const authState = getAuthState();
    const token = getToken();
    if (!authState.serverUrl || !token) {
      return baseParams;
    }

    return {
      ...baseParams,
      managed: {
        server_url: authState.serverUrl,
        auth_token: token,
        tier: authState.tier,
      },
    };
  }

  async function executePluginCommand(
    command: string,
    params: unknown,
  ): Promise<PluginCommandResult> {
    console.debug("[PluginIframe] dispatch start", {
      pluginId,
      componentId,
      command,
      hasApi: !!api,
      runtime: getBrowserPlugin(pluginId) ? "browser-plugin" : "backend-api",
    });
    const commandParams = withManagedContext(command, params);
    const browserPlugin = getBrowserPlugin(pluginId);
    if (browserPlugin) {
      try {
        const result = await dispatchCommand(pluginId, command, commandParams);
        console.debug("[PluginIframe] dispatch done (browser-plugin)", {
          pluginId,
          componentId,
          command,
          success: result.success,
          hasData: result.data != null,
          error: result.error ?? null,
        });
        return result;
      } catch (e) {
        console.error("[PluginIframe] dispatch threw (browser-plugin)", {
          pluginId,
          componentId,
          command,
          error: e instanceof Error ? e.message : String(e),
        });
        if (!api) {
          return {
            success: false,
            error: e instanceof Error ? e.message : String(e),
          };
        }
      }
    }

    if (!api) {
      return {
        success: false,
        error: `Plugin runtime unavailable: ${pluginId}`,
      };
    }

    try {
      const data = await api.executePluginCommand(
        pluginId,
        command,
        commandParams as any,
      );
      console.debug("[PluginIframe] dispatch done (backend-api)", {
        pluginId,
        componentId,
        command,
        success: true,
        hasData: data != null,
      });
      return { success: true, data };
    } catch (e) {
      console.error("[PluginIframe] dispatch failed (backend-api)", {
        pluginId,
        componentId,
        command,
        error: e instanceof Error ? e.message : String(e),
      });
      return {
        success: false,
        error: e instanceof Error ? e.message : String(e),
      };
    }
  }

  function readHostActionEnvelope(data: unknown): HostActionEnvelope | null {
    if (!data || typeof data !== "object" || !("host_action" in data)) {
      return null;
    }
    const envelope = data as HostActionEnvelope;
    if (!envelope.host_action?.type) {
      return null;
    }
    return envelope;
  }

  function readWorkspaceMetadataPatch(
    data: unknown,
  ): WorkspaceMetadataPatchEnvelope["workspace_metadata_patch"] | null {
    if (!data || typeof data !== "object" || !("workspace_metadata_patch" in data)) {
      return null;
    }
    const patch = (data as { workspace_metadata_patch?: unknown }).workspace_metadata_patch;
    if (!patch || typeof patch !== "object" || Array.isArray(patch)) {
      return null;
    }
    const rawData = (patch as { data?: unknown }).data;
    if (
      rawData !== null &&
      (rawData === undefined || typeof rawData !== "object" || Array.isArray(rawData))
    ) {
      return null;
    }
    return patch as WorkspaceMetadataPatchEnvelope["workspace_metadata_patch"];
  }

  async function applyWorkspaceMetadataPatch(data: unknown): Promise<void> {
    const patch = readWorkspaceMetadataPatch(data);
    if (!patch) {
      return;
    }

    const localId = getCurrentWorkspaceId();
    if (!localId) {
      return;
    }

    const effectivePluginId =
      typeof patch.plugin_id === "string" && patch.plugin_id.trim().length > 0
        ? patch.plugin_id
        : pluginId;
    setPluginMetadata(localId, effectivePluginId, patch.data ?? null);
  }

  async function executePluginCommandWithHostEffects(
    command: string,
    params: unknown,
  ): Promise<PluginCommandResult> {
    let result = await executePluginCommand(command, params);
    const hostAction = readHostActionEnvelope(result.data);
    if (result.success && hostAction) {
      const hostResult = await Promise.resolve().then(() => {
        if (onHostAction) {
          return onHostAction(hostAction.host_action);
        }
        if (hostAction.host_action.type !== "open-oauth") {
          throw new Error(`Unsupported host action: ${hostAction.host_action.type}`);
        }
        const payload =
          hostAction.host_action.payload &&
          typeof hostAction.host_action.payload === "object"
            ? (hostAction.host_action.payload as {
                url?: string;
                redirect_uri_prefix?: string;
              })
            : {};
        return openOauthWindow({
          url: payload.url ?? "",
          redirect_uri_prefix: payload.redirect_uri_prefix,
        });
      });

      if (hostAction.follow_up?.command) {
        const hostResultPatch =
          hostResult && typeof hostResult === "object" && !Array.isArray(hostResult)
            ? (hostResult as Record<string, unknown>)
            : {};
        result = await executePluginCommandWithHostEffects(hostAction.follow_up.command, {
          ...(hostAction.follow_up.params ?? {}),
          ...hostResultPatch,
        });
      }
    }

    if (result.success) {
      await applyWorkspaceMetadataPatch(result.data);
    }
    return result;
  }

  function extractComponentHtml(value: unknown): string | null {
    if (typeof value === "string") return value;
    if (!value || typeof value !== "object") return null;

    const obj = value as Record<string, unknown>;

    if (typeof obj.response === "string") return obj.response;
    if (typeof obj.html === "string") return obj.html;
    if (typeof obj.data === "string") return obj.data;
    if (obj.type === "PluginResult" && typeof obj.data === "string") {
      return obj.data;
    }
    if (obj.success === true) {
      return extractComponentHtml(obj.data);
    }

    return null;
  }

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
      const startedAt = performance.now();
      console.debug("[PluginIframe] load start", { pluginId, componentId });
      const result: PluginCommandResult = await Promise.race([
        executePluginCommand("get_component_html", {
          component_id: componentId,
        }),
        new Promise<PluginCommandResult>((resolve) =>
          setTimeout(() => {
            resolve({
              success: false,
              error: `Timed out loading plugin component after ${COMPONENT_LOAD_TIMEOUT_MS}ms`,
            });
          }, COMPONENT_LOAD_TIMEOUT_MS),
        ),
      ]);
      console.debug("[PluginIframe] load response", {
        pluginId,
        componentId,
        elapsedMs: Math.round(performance.now() - startedAt),
        success: result.success,
        hasData: result.data != null,
        error: result.error ?? null,
      });
      const html = extractComponentHtml(result.data);
      if (!result.success || !html) {
        console.error("[PluginIframe] get_component_html failed: success=%s, error=%s, data type=%s", result.success, result.error, typeof result.data, {
          pluginId,
          componentId,
          result,
        });
        error = result.error ?? "Failed to load component HTML";
        loading = false;
        return;
      }
      const blob = new Blob([html], { type: "text/html" });
      blobUrl = URL.createObjectURL(blob);
      console.debug("[PluginIframe] iframe blob ready", {
        pluginId,
        componentId,
        htmlBytes: html.length,
      });
      loading = false;
    } catch (e) {
      console.error("[PluginIframe] get_component_html threw:", {
        pluginId,
        componentId,
        error: e instanceof Error ? e.message : String(e),
      });
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
    console.debug("[PluginIframe] iframe loaded", { pluginId, componentId });
    iframeReady = true;
    postToIframe({
      type: "init",
      theme: themeStore.isDark ? "dark" : "light",
      cssVars: collectCssVars(),
      entry: entry ? { path: entry.path, title: entry.title, content: entry.content } : null,
    });
  }

  // Re-send theme data when light/dark mode or appearance preset changes
  $effect(() => {
    // Track reactive dependencies
    const isDark = themeStore.isDark;
    void appearanceStore.appearance;

    if (!iframeReady || !iframeEl?.contentWindow) return;
    postToIframe({
      type: "theme-update",
      theme: isDark ? "dark" : "light",
      cssVars: collectCssVars(),
    });
  });

  // Send entry-changed event when the current entry changes
  $effect(() => {
    const e = entry;
    if (!iframeReady || !iframeEl?.contentWindow) return;
    postToIframe({
      type: "plugin-event",
      event: "entry-changed",
      data: e ? { path: e.path, title: e.title, content: e.content } : null,
    });
  });

  // Listen for messages from the iframe
  function handleMessage(event: MessageEvent) {
    // Only handle messages from our iframe
    if (!iframeEl || event.source !== iframeEl.contentWindow) return;

    const data = event.data;
    if (!data || typeof data !== "object") return;

    if (data.type === "plugin-command") {
      const { command, params, requestId } = data;
      console.debug("[PluginIframe] iframe -> host command", {
        pluginId,
        componentId,
        command,
        requestId,
      });
      executePluginCommandWithHostEffects(command, params).then((result) => {
        console.debug("[PluginIframe] host -> iframe response", {
          pluginId,
          componentId,
          command,
          requestId,
          success: result.success,
          error: result.error ?? null,
        });
        postToIframe({
          type: "plugin-response",
          requestId,
          result,
        });
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
          postToIframe({
            type: "host-action-response",
            requestId,
            success: true,
            data: result ?? null,
          });
        })
        .catch((e) => {
          postToIframe({
            type: "host-action-response",
            requestId,
            success: false,
            error: e instanceof Error ? e.message : String(e),
          });
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
    postToIframe({ type: "plugin-event", event, data });
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
