<script lang="ts">
  /**
   * PluginSettingsTab - Renders declarative settings fields from a plugin manifest.
   *
   * Takes an array of SettingsField definitions and renders them as form controls
   * using shadcn-svelte primitives. Calls onConfigChange when any field value changes.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Button } from "$lib/components/ui/button";
  import type { Api } from "$lib/backend/api";
  import { Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { dispatchCommand, getPlugin } from "$lib/plugins/browserPluginManager.svelte";
  import type { BrowserPluginCallOptions } from "$lib/plugins/extismBrowserLoader";
  import { openOauthWindow } from "$lib/plugins/oauthWindow";
  import { readPluginLocalSelectionFile } from "$lib/plugins/pluginLocalSelections";
  import { getRuntimePluginCommandParams } from "$lib/plugins/pluginRuntimeConfig";
  import { getAuthState } from "$lib/auth";
  import {
    getCurrentWorkspaceId,
    setPluginMetadata,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import UpgradeBanner from "$lib/components/UpgradeBanner.svelte";
  import PublishingPanel from "$lib/share/PublishingPanel.svelte";
  import NamespaceGuardWidget from "$lib/namespace/NamespaceGuardWidget.svelte";
  import NamespaceSiteUrlWidget from "$lib/namespace/NamespaceSiteUrlWidget.svelte";
  import NamespaceSubdomainWidget from "$lib/namespace/NamespaceSubdomainWidget.svelte";
  import NamespaceAudienceWidget from "$lib/namespace/NamespaceAudienceWidget.svelte";
  import AudiencePickerWidget from "$lib/namespace/AudiencePickerWidget.svelte";
  import NamespacePublishWidget from "$lib/namespace/NamespacePublishWidget.svelte";
  import NamespaceCustomDomainManager from "$lib/namespace/NamespaceCustomDomainManager.svelte";
  import { getNamespaceContext } from "$lib/namespace/namespaceContext.svelte";
  import Self from "./PluginSettingsTab.svelte";
  import { evaluateFieldCondition } from "./pluginFieldConditions";
  import type { SettingsField, SelectOption } from "$lib/backend/generated";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

  interface Props {
    pluginId: string;
    fields: SettingsField[];
    config: Record<string, JsonValue>;
    onConfigChange: (key: string, value: JsonValue) => void | Promise<void>;
    api?: Api | null;
    onHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
  }

  let { pluginId, fields, config, onConfigChange, api = null, onHostAction }: Props = $props();

  let authState = $derived(getAuthState());

  // Button state: keyed by command name
  let buttonStates = $state<Record<string, { loading: boolean; result?: { success: boolean; message: string } }>>({});

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

  function isCancelledHostActionResult(result: unknown): boolean {
    return !!result && typeof result === "object" && !Array.isArray(result)
      && (result as { cancelled?: unknown }).cancelled === true;
  }

  function buildBrowserCallOptions(
    params: Record<string, JsonValue>,
  ): BrowserPluginCallOptions | undefined {
    const fileKey = typeof params.file_key === "string" ? params.file_key : null;
    if (!fileKey) {
      return undefined;
    }

    return {
      getFile: async (key: string) => {
        if (key !== fileKey) {
          return null;
        }
        return await readPluginLocalSelectionFile(fileKey);
      },
    };
  }

  async function buildNativeRequestFiles(
    params: Record<string, JsonValue>,
  ): Promise<Record<string, Uint8Array> | undefined> {
    const fileKey = typeof params.file_key === "string" ? params.file_key : null;
    if (!fileKey) {
      return undefined;
    }

    const bytes = await readPluginLocalSelectionFile(fileKey);
    if (!bytes) {
      return undefined;
    }

    return { [fileKey]: bytes };
  }

  function readResultMessage(result: PluginCommandResult): string {
    const message =
      result.data &&
      typeof result.data === "object" &&
      "message" in result.data &&
      typeof (result.data as { message?: unknown }).message === "string"
        ? (result.data as { message: string }).message
        : null;
    return message ?? result.error ?? (result.success ? "Success" : "Failed");
  }

  function readHostActionEnvelope(data: unknown): HostActionEnvelope | null {
    if (!data || typeof data !== "object") {
      return null;
    }
    if (!("host_action" in data)) {
      return null;
    }
    const envelope = data as HostActionEnvelope;
    if (!envelope.host_action?.type) {
      return null;
    }
    return envelope;
  }

  function readConfigPatch(data: unknown): Record<string, JsonValue> | null {
    if (!data || typeof data !== "object" || !("config_patch" in data)) {
      return null;
    }
    const patch = (data as { config_patch?: unknown }).config_patch;
    if (!patch || typeof patch !== "object" || Array.isArray(patch)) {
      return null;
    }
    return patch as Record<string, JsonValue>;
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

  async function applyConfigPatch(data: unknown): Promise<void> {
    const patch = readConfigPatch(data);
    if (!patch) {
      return;
    }

    for (const [key, value] of Object.entries(patch)) {
      await onConfigChange(key, value);
    }
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

  async function executePluginCommand(
    command: string,
    params: Record<string, JsonValue>,
  ): Promise<PluginCommandResult> {
    const browserPlugin = getPlugin(pluginId);
    if (browserPlugin) {
      return dispatchCommand(pluginId, command, params, buildBrowserCallOptions(params));
    }

    if (!api) {
      return {
        success: false,
        error: `Plugin command unavailable: ${pluginId}`,
      };
    }

    try {
      const requestFiles = await buildNativeRequestFiles(params);
      const data = await api.executePluginCommand(
        pluginId,
        command,
        params as JsonValue,
        requestFiles,
      );
      return { success: true, data };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  async function saveCurrentConfig(): Promise<void> {
    const browserPlugin = getPlugin(pluginId);
    if (browserPlugin) {
      await browserPlugin.setConfig(config as Record<string, unknown>);
      return;
    }
    if (api) {
      await api.setPluginConfig(pluginId, config as JsonValue);
    }
  }

  async function handleButtonClick(command: string) {
    buttonStates = { ...buttonStates, [command]: { loading: true } };
    try {
      // Save current config first
      console.log("[PluginSettingsTab] saving config before command", { pluginId, command });
      await saveCurrentConfig();
      console.log("[PluginSettingsTab] config saved");

      const runtimeParams = await getRuntimePluginCommandParams(pluginId, command, config);
      console.log("[PluginSettingsTab] dispatching command", {
        pluginId,
        command,
        runtimeParams,
      });
      let result = await executePluginCommand(command, runtimeParams);

      while (result.success) {
        const hostAction = readHostActionEnvelope(result.data);
        if (!hostAction) {
          break;
        }

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

        if (isCancelledHostActionResult(hostResult)) {
          result = { success: true, data: { message: "Cancelled" } };
          break;
        }

        if (!hostAction.follow_up?.command) {
          result = { success: true, data: { message: "Completed" } };
          break;
        }

        const hostResultPatch =
          hostResult && typeof hostResult === "object" && !Array.isArray(hostResult)
            ? (hostResult as Record<string, JsonValue>)
            : {};
        result = await executePluginCommand(
          hostAction.follow_up.command,
          {
            ...((hostAction.follow_up.params as Record<string, JsonValue> | undefined) ?? {}),
            ...hostResultPatch,
          },
        );
      }

      if (result.success) {
        await applyConfigPatch(result.data);
        await applyWorkspaceMetadataPatch(result.data);
      }

      console.log("[PluginSettingsTab] command result", { pluginId, command, result });
      buttonStates = {
        ...buttonStates,
        [command]: {
          loading: false,
          result: {
            success: result.success,
            message: readResultMessage(result),
          },
        },
      };
      if (!result.success) {
        return;
      }
    } catch (e) {
      console.error("[PluginSettingsTab] command failed", { pluginId, command, error: e });
      buttonStates = { ...buttonStates, [command]: { loading: false, result: { success: false, message: String(e) } } };
    }
  }

  async function handleHostActionClick(actionType: string) {
    const stateKey = `host:${actionType}`;
    buttonStates = { ...buttonStates, [stateKey]: { loading: true } };
    try {
      if (!onHostAction) {
        throw new Error(`Host action unavailable: ${actionType}`);
      }
      await onHostAction({ type: actionType });
      buttonStates = {
        ...buttonStates,
        [stateKey]: { loading: false, result: { success: true, message: "Opened" } },
      };
    } catch (error) {
      buttonStates = {
        ...buttonStates,
        [stateKey]: {
          loading: false,
          result: {
            success: false,
            message: error instanceof Error ? error.message : String(error),
          },
        },
      };
    }
  }

  function handleAddWorkspaceRequest() {
    return onHostAction?.({ type: "open-add-workspace" });
  }
</script>

<div class="space-y-4">
  {#each fields as field}
    {#if field.type === "Section"}
      <div class="pt-2">
        <h4 class="font-medium text-sm">{field.label}</h4>
        {#if field.description}
          <p class="text-xs text-muted-foreground">{field.description}</p>
        {/if}
      </div>
    {:else if field.type === "Toggle"}
      <div class="flex items-center justify-between gap-4 px-1">
        <Label for={field.key} class="text-sm cursor-pointer flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Switch
          id={field.key}
          checked={config[field.key] === true}
          onCheckedChange={(checked) => onConfigChange(field.key, checked)}
        />
      </div>
    {:else if field.type === "Text"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Input
          id={field.key}
          type="text"
          placeholder={field.placeholder ?? undefined}
          value={(config[field.key] as string) ?? ""}
          oninput={(e) => onConfigChange(field.key, (e.target as HTMLInputElement).value)}
        />
      </div>
    {:else if field.type === "Password"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Input
          id={field.key}
          type="password"
          placeholder={field.placeholder ?? undefined}
          value={(config[field.key] as string) ?? ""}
          oninput={(e) => onConfigChange(field.key, (e.target as HTMLInputElement).value)}
        />
      </div>
    {:else if field.type === "Number"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm">{field.label}</Label>
        <Input
          id={field.key}
          type="number"
          min={field.min ?? undefined}
          max={field.max ?? undefined}
          value={(config[field.key] as number) ?? 0}
          oninput={(e) => onConfigChange(field.key, Number((e.target as HTMLInputElement).value))}
        />
      </div>
    {:else if field.type === "Select"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <select
          id={field.key}
          class="w-full px-2 py-1 text-sm border rounded bg-background"
          value={(config[field.key] as string) ?? ""}
          onchange={(e) => onConfigChange(field.key, (e.target as HTMLSelectElement).value)}
        >
          {#each (field.options as SelectOption[]) as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
      </div>
    {:else if field.type === "Button"}
      {@const state = buttonStates[field.command]}
      <div class="flex items-center gap-3 px-1 pt-1">
        <Button
          variant={field.variant === "destructive" ? "destructive" : field.variant === "outline" ? "outline" : "default"}
          disabled={state?.loading}
          onclick={() => handleButtonClick(field.command)}
        >
          {#if state?.loading}
            <Loader2 class="size-4 mr-1.5 animate-spin" />
          {/if}
          {field.label}
        </Button>
        {#if state?.result}
          <span class="flex items-center gap-1 text-sm" class:text-green-600={state.result.success} class:text-destructive={!state.result.success}>
            {#if state.result.success}
              <Check class="size-4" />
            {:else}
              <AlertCircle class="size-4" />
            {/if}
            {state.result.message}
          </span>
        {/if}
      </div>
    {:else if field.type === "HostActionButton"}
      {@const state = buttonStates[`host:${field.action_type}`]}
      <div class="flex items-center gap-3 px-1 pt-1">
        <Button
          variant={field.variant === "destructive" ? "destructive" : field.variant === "outline" ? "outline" : "default"}
          disabled={state?.loading}
          onclick={() => handleHostActionClick(field.action_type)}
        >
          {#if state?.loading}
            <Loader2 class="size-4 mr-1.5 animate-spin" />
          {/if}
          {field.label}
        </Button>
        {#if state?.result}
          <span class="flex items-center gap-1 text-sm" class:text-green-600={state.result.success} class:text-destructive={!state.result.success}>
            {#if state.result.success}
              <Check class="size-4" />
            {:else}
              <AlertCircle class="size-4" />
            {/if}
            {state.result.message}
          </span>
        {/if}
      </div>
    {:else if field.type === "AuthStatus"}
      <div class="space-y-1.5 px-1">
        <h4 class="font-medium text-sm">{field.label}</h4>
        {#if field.description && !authState.isAuthenticated}
          <p class="text-xs text-muted-foreground">{field.description}</p>
        {/if}
        {#if authState.isAuthenticated}
          <p class="text-sm text-muted-foreground">
            Signed in as <span class="font-medium text-foreground">{authState.user?.email ?? "unknown"}</span>
          </p>
        {:else}
          <SignInForm compact />
        {/if}
      </div>
    {:else if field.type === "UpgradeBanner"}
      {#if authState.tier !== "plus"}
        <div class="px-1">
          <UpgradeBanner
            feature={field.feature}
            description={field.description ?? `Upgrade to use ${field.feature}.`}
          />
        </div>
      {/if}
    {:else if field.type === "Conditional"}
      {#if evaluateFieldCondition(field.condition, authState, config)}
        <Self {pluginId} fields={field.fields} {config} {onConfigChange} {api} {onHostAction} />
      {/if}
    {:else if field.type === "HostWidget"}
      {#if field.widget_id === "publish.site-panel"}
        <div class="px-1">
          <PublishingPanel {api} onAddWorkspace={handleAddWorkspaceRequest} />
        </div>
      {:else if field.widget_id === "namespace.guard"}
        <div class="px-1">
          <NamespaceGuardWidget signInAction={field.sign_in_action} />
        </div>
      {:else if field.widget_id === "namespace.site-url"}
        <div class="px-1">
          <NamespaceSiteUrlWidget />
        </div>
      {:else if field.widget_id === "namespace.subdomain"}
        <div class="px-1">
          <NamespaceSubdomainWidget />
        </div>
      {:else if field.widget_id === "namespace.audiences"}
        <div class="px-1">
          <NamespaceAudienceWidget {api} />
        </div>
      {:else if field.widget_id === "audience.picker"}
        <AudiencePickerWidget {api} />
      {:else if field.widget_id === "namespace.publish-button"}
        <div class="px-1">
          <NamespacePublishWidget />
        </div>
      {:else if field.widget_id === "namespace.custom-domains"}
        {@const nsCtx = getNamespaceContext()}
        {#if nsCtx.isReady && nsCtx.isConfigured && nsCtx.namespaceId && nsCtx.customDomainsAvailable}
          <div class="px-1">
            <NamespaceCustomDomainManager namespaceId={nsCtx.namespaceId} />
          </div>
        {/if}
      {:else}
        <div class="px-1 text-xs text-muted-foreground">
          Unsupported host widget: {field.widget_id}
        </div>
      {/if}
    {/if}
  {/each}
</div>
