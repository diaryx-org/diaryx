<script lang="ts">
  import type { Api } from '$lib/backend/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Alert from '$lib/components/ui/alert';
  import { getTemplateContextStore } from '$lib/stores/templateContextStore.svelte';
  import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
  import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
  import { workspaceStore } from '@/models/stores';
  import ManageAudiencesModal from '$lib/components/ManageAudiencesModal.svelte';
  import {
    AlertCircle,
    CheckCircle2,
    ChevronRight,
    Eye,
    Globe,
    Loader2,
    Settings2,
    Tags,
    Trash2,
  } from '@lucide/svelte';
  import * as corePublish from '$lib/publish/corePublishService';
  import { describePublishError } from '$lib/publish/publishErrors';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import { getAuthState, getServerUrl } from '$lib/auth';
  import { createNamespace } from '$lib/namespace/namespaceService';
  import UpgradeBanner from '$lib/components/UpgradeBanner.svelte';
  import NamespaceSiteUrl from '$lib/namespace/NamespaceSiteUrl.svelte';
  import NamespaceSubdomainManager from '$lib/namespace/NamespaceSubdomainManager.svelte';
  import NamespaceAudienceManager from '$lib/namespace/NamespaceAudienceManager.svelte';
  import NamespacePublishButton from '$lib/namespace/NamespacePublishButton.svelte';

  interface Props {
    api: Api | null;
  }

  let { api }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();
  const configStore = getWorkspaceConfigStore();

  // ---- Publish command helper ----
  //
  // Routes the panel's command surface to `corePublishService` — i.e. directly
  // to `diaryx_core::publish` (WASM backend on web, native IPC on Tauri). No
  // Extism `diaryx.publish` plugin is involved.

  async function executePublishCommand<T = any>(
    command: string,
    params: Record<string, any> = {},
  ): Promise<T> {
    if (!api) throw new Error('Publish API unavailable');

    switch (command) {
      case 'GetPublishConfig':
        return (await corePublish.getPublishConfig(api)) as T;
      case 'SetPublishConfig':
        await corePublish.setPublishConfig(api, params as corePublish.PublishConfig);
        return { ok: true } as T;
      case 'GetAudiencePublishStates':
        return ((await corePublish.getPublishConfig(api)).audience_states ?? {}) as T;
      case 'SetAudiencePublishState':
        return (await corePublish.setAudiencePublishState(
          api,
          params.audience,
          params.config,
        )) as T;
      case 'PreviewPublish':
        return (await corePublish.previewPublish(
          params.server_url,
          params.namespace_id,
        )) as T;
      case 'PublishToNamespace':
        return (await corePublish.publishToNamespace(
          params.server_url,
          params.namespace_id,
        )) as T;
      default:
        throw new Error(`Unknown publish command: ${command}`);
    }
  }

  // ---- State ----

  let authState = $derived(getAuthState());
  let isAuthenticated = $derived(authState.isAuthenticated);
  let serverUrl = $derived(getServerUrl() ?? '');

  let rootPath = $derived(workspaceStore.tree?.path ?? null);
  let defaultAudience = $derived(configStore.config?.default_audience ?? null);
  let hasDefaultWorkspace = $derived(rootPath !== null);

  // Plugin-backed state
  let namespaceId = $state<string | null>(null);
  let subdomain = $state<string | null>(null);
  let error = $state<string | null>(null);
  let errorDetail = $state<string | null>(null);

  /**
   * Surface a publish/preview failure: translate it to a friendly message,
   * show it in the inline alert (with an expandable detail line) and as a
   * toast, and log the raw error for debugging.
   */
  function reportPublishError(e: unknown, fallback: string) {
    const friendly = describePublishError(e, fallback);
    error = friendly.title;
    errorDetail = friendly.detail ?? null;
    showError(friendly.title, 'Publishing');
  }

  function clearError() {
    error = null;
    errorDetail = null;
  }

  type AudienceConfig = { state: string; access_method?: string };
  let audienceStates = $state<Record<string, AudienceConfig>>({});
  let availableAudiences = $state<string[]>([]);
  let initializedRootPath = $state<string | null>(null);

  // Loading flags
  let isLoading = $state(false);
  let isPublishing = $state(false);
  let isCreatingNamespace = $state(false);
  let isPreviewing = $state(false);
  let isUnpublishing = $state(false);

  // Inline confirm for the destructive "remove published files" action.
  let showUnpublishConfirm = $state(false);

  // ---- Preview / progress / receipt ----

  type PreviewAudience = {
    name: string;
    publish: boolean;
    stale: boolean;
    upload_count: number;
    upload_bytes: number;
    unchanged: number;
    delete_count: number;
    deletes: string[];
  };
  type PreviewSummary = {
    audiences: PreviewAudience[];
    audiences_to_delete: string[];
    totals: { uploads: number; unchanged: number; deletes: number; bytes: number };
  };
  type PublishReceipt = {
    audiences_published: string[];
    audiences_deleted: string[];
    uploaded: number;
    skipped_unchanged: number;
    deleted: number;
    bytes_uploaded: number;
  };
  type PublishProgress = {
    phase: 'start' | 'uploading' | 'finalizing' | 'done';
    audience?: string;
    current: number;
    total: number;
  };

  let previewResult = $state<PreviewSummary | null>(null);
  let publishResult = $state<PublishReceipt | null>(null);
  let progress = $state<PublishProgress | null>(null);

  let progressPercent = $derived(
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.current / progress.total) * 100))
      : progress?.phase === 'finalizing' || progress?.phase === 'done'
        ? 100
        : 0,
  );

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  // Manage audiences modal
  let showManageAudiences = $state(false);

  // Set default audience inline
  let showDefaultAudienceInput = $state(false);
  let defaultAudienceInput = $state('');

  // Collapsible settings section
  let showSettings = $state(false);

  let isConfigured = $derived(namespaceId !== null);

  // File-declared audiences (`audiences:` in the root index frontmatter) are
  // the source of truth when present. They supersede the legacy
  // `audience_states` map — see NamespaceAudienceManager's `usingFile` logic.
  let declaredAudiences = $derived(configStore.config?.audiences ?? null);

  let allAudiences = $derived.by(() => {
    const set = new Set(availableAudiences);
    if (defaultAudience && !set.has(defaultAudience)) {
      set.add(defaultAudience);
    }
    for (const decl of declaredAudiences ?? []) {
      set.add(decl.name);
    }
    return [...set];
  });

  let hasAnyAudience = $derived(allAudiences.length > 0);

  // When audiences are declared in the workspace file, every declaration is a
  // published audience (its gates control access, not whether it publishes).
  // Otherwise fall back to the legacy per-audience `state` map.
  let publishedAudienceCount = $derived(
    declaredAudiences !== null
      ? declaredAudiences.length
      : Object.values(audienceStates).filter(c => c.state !== 'unpublished').length
  );

  let hasPublishingAccess = $derived(
    isConfigured || authState.publishedSiteLimit > 0
  );

  let canPublish = $derived(
    hasDefaultWorkspace
    && !isPublishing
    && !isLoading
    && !isCreatingNamespace
    && isAuthenticated
    && hasPublishingAccess
    && (publishedAudienceCount > 0 || !hasAnyAudience)
  );

  let firstPublishedAudience = $derived.by(() => {
    if (declaredAudiences && declaredAudiences.length > 0) {
      return declaredAudiences[0].name;
    }
    const entry = Object.entries(audienceStates).find(([, c]) => c.state !== 'unpublished');
    return entry?.[0];
  });

  // ---- Effects ----

  $effect(() => {
    if (!rootPath || initializedRootPath === rootPath) return;
    initializedRootPath = rootPath;
    loadPublishConfig();
  });

  $effect(() => {
    if (rootPath) configStore.load(rootPath);
  });

  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    templateContextStore.audiencesVersion;
    if (rootPath) loadAudiences();
  });

  // Subscribe to live publish progress. Events arrive on the shared
  // filesystem-event channel (uppercase `type` is forwarded on both backends).
  $effect(() => {
    const backend = workspaceStore.backend as
      | {
          onFileSystemEvent?: (cb: (e: any) => void) => number;
          offFileSystemEvent?: (id: number) => void;
        }
      | null
      | undefined;
    if (!backend?.onFileSystemEvent) return;
    const id = backend.onFileSystemEvent((event: any) => {
      if (event?.type === 'PublishProgress') {
        progress = {
          phase: event.phase,
          audience: event.audience,
          current: event.current ?? 0,
          total: event.total ?? 0,
        };
      }
    });
    return () => backend.offFileSystemEvent?.(id);
  });

  // ---- Data loading ----

  async function loadPublishConfig() {
    if (!api) return;
    isLoading = true;
    clearError();
    try {
      const config = await executePublishCommand<{
        namespace_id?: string | null;
        subdomain?: string | null;
        audience_states?: Record<string, AudienceConfig>;
      }>('GetPublishConfig', {});
      namespaceId = config.namespace_id ?? null;
      subdomain = config.subdomain ?? null;
      audienceStates = config.audience_states ?? {};
    } catch (e) {
      const friendly = describePublishError(e, 'Failed to load publish config');
      error = friendly.title;
      errorDetail = friendly.detail ?? null;
      console.error('[Publishing] load config failed', e);
      namespaceId = null;
      subdomain = null;
      audienceStates = {};
    } finally {
      isLoading = false;
    }
  }

  async function loadAudiences() {
    if (!api || !rootPath) return;
    try {
      availableAudiences = await api.getAvailableAudiences(rootPath);
      for (const name of availableAudiences) colorStore.assignColor(name);
    } catch {
      availableAudiences = [];
    }
  }

  // ---- Callbacks for child components ----

  async function handleAudienceStateChange(audience: string, config: AudienceConfig) {
    // Sync to plugin frontmatter
    try {
      await executePublishCommand('SetAudiencePublishState', {
        audience,
        server_url: serverUrl,
        config: {
          state: config.state,
          access_method: config.access_method,
        },
      });
    } catch {
      // Best effort — server sync already happened in the component
    }

    if (config.state === 'unpublished') {
      const { [audience]: _, ...rest } = audienceStates;
      audienceStates = rest;
    } else {
      audienceStates = { ...audienceStates, [audience]: config };
    }
  }

  function handleSubdomainChange(newSubdomain: string | null) {
    subdomain = newSubdomain;
  }

  // ---- Publish ----

  async function handleQuickPublish() {
    if (!api || !rootPath) return;
    clearError();

    try {
      // Set audience: ["public"] on the root index so children inherit
      const rootIndexPath = await api.resolveWorkspaceRootIndexPath(rootPath);
      if (!rootIndexPath) {
        reportPublishError(
          'Could not find the workspace root index',
          'Could not find the workspace root index',
        );
        return;
      }
      await api.setFrontmatterProperty(rootIndexPath, 'audience', ['public']);

      // Set the "public" audience to published state
      await executePublishCommand('SetAudiencePublishState', {
        audience: 'public',
        server_url: serverUrl,
        config: { state: 'public' },
      });

      // Update local state
      audienceStates = { ...audienceStates, public: { state: 'public' } };
      availableAudiences = [...new Set([...availableAudiences, 'public'])];
      colorStore.assignColor('public');
      templateContextStore.bumpAudiencesVersion();
    } catch (e) {
      reportPublishError(e, 'Failed to set up public audience');
      return;
    }

    // Now publish via the normal path
    await handlePublish();
  }

  async function handlePreview() {
    if (!api || !namespaceId) return;
    clearError();
    isPreviewing = true;
    previewResult = null;
    try {
      const result = await executePublishCommand<PreviewSummary>('PreviewPublish', {
        namespace_id: namespaceId,
        server_url: serverUrl,
      });
      previewResult = result;
    } catch (e) {
      reportPublishError(e, 'Preview failed');
    } finally {
      isPreviewing = false;
    }
  }

  async function handlePublish() {
    clearError();

    // Auto-create namespace if not configured
    if (!isConfigured) {
      isCreatingNamespace = true;
      try {
        const ns = await createNamespace();
        namespaceId = ns.id;
        // Save to plugin config
        await executePublishCommand('SetPublishConfig', {
          namespace_id: ns.id,
          subdomain,
          audience_states: audienceStates,
          public_audiences: Object.entries(audienceStates)
            .filter(([, c]) => c.state === 'public')
            .map(([name]) => name),
        });
      } catch (e) {
        reportPublishError(e, 'Failed to create namespace');
        isCreatingNamespace = false;
        return;
      } finally {
        isCreatingNamespace = false;
      }
    }

    isPublishing = true;
    progress = null;
    publishResult = null;
    try {
      const result = await executePublishCommand<PublishReceipt>('PublishToNamespace', {
        namespace_id: namespaceId,
        server_url: serverUrl,
      });
      publishResult = result;
      previewResult = null; // preview is now stale

      const parts = [`${result.uploaded} uploaded`];
      if (result.skipped_unchanged) parts.push(`${result.skipped_unchanged} unchanged`);
      if (result.deleted) parts.push(`${result.deleted} deleted`);
      showSuccess(`Published — ${parts.join(' · ')}`);
    } catch (e) {
      reportPublishError(e, 'Publish failed');
    } finally {
      isPublishing = false;
      progress = null;
    }
  }

  // ---- Unpublish (remove uploaded files) ----

  async function handleUnpublish() {
    if (!namespaceId) return;
    clearError();
    isUnpublishing = true;
    try {
      const { deleted } = await corePublish.unpublishNamespace(serverUrl, namespaceId);
      // Any cached preview/receipt is now stale.
      previewResult = null;
      publishResult = null;
      showSuccess(`Removed ${deleted} published file${deleted === 1 ? '' : 's'}`);
    } catch (e) {
      reportPublishError(e, 'Failed to remove published files');
    } finally {
      isUnpublishing = false;
      showUnpublishConfirm = false;
    }
  }

  // ---- Default audience ----

  async function handleSetDefaultAudience() {
    const value = defaultAudienceInput.trim();
    if (!value) return;
    await configStore.setField('default_audience', value);
    colorStore.assignColor(value);
    showDefaultAudienceInput = false;
    defaultAudienceInput = '';
    templateContextStore.bumpAudiencesVersion();
  }

  function handleOpenSyncSetup() {
    showInfo('Open account settings to configure publishing.');
  }
</script>

<div class="p-3 space-y-4">
  {#if error}
    <Alert.Root variant="destructive" class="py-2">
      <AlertCircle class="size-4" />
      <Alert.Description class="text-xs">
        <span class="font-medium">{error}</span>
        {#if errorDetail && errorDetail !== error}
          <details class="mt-1">
            <summary class="cursor-pointer text-[11px] opacity-80 hover:opacity-100">
              Details
            </summary>
            <pre class="mt-1 whitespace-pre-wrap break-words text-[11px] opacity-80">{errorDetail}</pre>
          </details>
        {/if}
      </Alert.Description>
    </Alert.Root>
  {/if}

  {#if !hasDefaultWorkspace}
    <div class="text-center space-y-1 py-8">
      <Globe class="size-8 mx-auto text-muted-foreground" />
      <h3 class="font-medium text-sm">Publishing Unavailable</h3>
      <p class="text-xs text-muted-foreground">
        Sign in and ensure your default workspace is available.
      </p>
    </div>
  {:else if isLoading && !isConfigured}
    <div class="flex items-center justify-center py-8">
      <Loader2 class="size-5 animate-spin text-muted-foreground" />
    </div>
  {:else if !isAuthenticated}
    <div class="text-center space-y-3 py-8">
      <Globe class="size-8 mx-auto text-muted-foreground" />
      <div class="space-y-1">
        <h3 class="font-medium text-sm">Sign in to publish</h3>
        <p class="text-xs text-muted-foreground">
          Publish your workspace as a site with audience-based access control.
        </p>
      </div>
      <Button variant="outline" size="sm" onclick={handleOpenSyncSetup}>
        Open Account Setup
      </Button>
    </div>
  {:else if authState.publishedSiteLimit === 0 && !isConfigured}
    <UpgradeBanner
      feature="Publishing"
      description="This account does not include website publishing."
      icon={Globe}
    />
  {:else}
    <!-- Site URL -->
    <NamespaceSiteUrl {namespaceId} {subdomain} {audienceStates} />

    <!-- Publish button -->
    <NamespacePublishButton
      {namespaceId}
      {canPublish}
      {publishedAudienceCount}
      {isPublishing}
      {isCreatingNamespace}
      onPublish={!hasAnyAudience ? handleQuickPublish : handlePublish}
    />

    {#if !hasAnyAudience}
      <p class="text-xs text-muted-foreground text-center">All entries will be published publicly</p>
    {/if}

    <!-- Live publish progress -->
    {#if isPublishing && progress}
      <div class="space-y-1">
        <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
          <div
            class="h-full rounded-full bg-primary transition-all duration-200"
            style="width: {progressPercent}%"
          ></div>
        </div>
        <p class="text-xs text-muted-foreground">
          {#if progress.phase === 'uploading'}
            Uploading {progress.current}/{progress.total}{progress.audience ? ` · ${progress.audience}` : ''}
          {:else if progress.phase === 'finalizing'}
            Finalizing…
          {:else if progress.phase === 'done'}
            Done
          {:else}
            Preparing…
          {/if}
        </p>
      </div>
    {/if}

    <!-- Preview changes -->
    {#if isConfigured}
      <Button
        variant="outline"
        size="sm"
        class="w-full text-xs"
        onclick={handlePreview}
        disabled={isPreviewing || isPublishing}
      >
        {#if isPreviewing}
          <Loader2 class="size-3.5 mr-1.5 animate-spin" />
          Checking…
        {:else}
          <Eye class="size-3.5 mr-1.5" />
          Preview changes
        {/if}
      </Button>
    {/if}

    <!-- Preview result -->
    {#if previewResult}
      <div class="rounded-md border border-border p-2.5 space-y-2 text-xs">
        <div class="flex items-center justify-between">
          <span class="font-medium">Pending changes</span>
          <button
            type="button"
            class="text-muted-foreground hover:text-foreground"
            onclick={() => { previewResult = null; }}
            aria-label="Dismiss preview"
          >×</button>
        </div>
        {#if previewResult.totals.uploads === 0 && previewResult.totals.deletes === 0 && previewResult.audiences_to_delete.length === 0}
          <p class="text-muted-foreground">Everything is up to date — nothing to publish.</p>
        {:else}
          <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-muted-foreground">
            <span>{previewResult.totals.uploads} to upload{previewResult.totals.bytes ? ` (${formatBytes(previewResult.totals.bytes)})` : ''}</span>
            <span>{previewResult.totals.unchanged} unchanged</span>
            {#if previewResult.totals.deletes > 0}
              <span class="text-destructive">{previewResult.totals.deletes} to delete</span>
            {/if}
          </div>
          {#if previewResult.audiences_to_delete.length > 0}
            <p class="text-destructive">
              Will remove audience(s): {previewResult.audiences_to_delete.join(', ')}
            </p>
          {/if}
          <div class="space-y-0.5">
            {#each previewResult.audiences.filter(a => a.upload_count > 0 || a.delete_count > 0) as aud (aud.name)}
              <div class="flex items-center justify-between">
                <span class="truncate">{aud.name}</span>
                <span class="text-muted-foreground shrink-0 tabular-nums">
                  {aud.upload_count > 0 ? `↑${aud.upload_count}` : ''}
                  {aud.delete_count > 0 ? ` ✕${aud.delete_count}` : ''}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Last publish receipt -->
    {#if publishResult && !isPublishing}
      <div class="rounded-md border border-border bg-muted/30 p-2.5 space-y-1 text-xs">
        <div class="flex items-center gap-1.5 font-medium">
          <CheckCircle2 class="size-3.5 text-primary" />
          Published
        </div>
        <div class="flex flex-wrap gap-x-3 text-muted-foreground">
          <span>{publishResult.uploaded} uploaded{publishResult.bytes_uploaded ? ` (${formatBytes(publishResult.bytes_uploaded)})` : ''}</span>
          <span>{publishResult.skipped_unchanged} unchanged</span>
          {#if publishResult.deleted > 0}
            <span>{publishResult.deleted} deleted</span>
          {/if}
        </div>
        {#if publishResult.audiences_deleted.length > 0}
          <p class="text-destructive">Removed audience(s): {publishResult.audiences_deleted.join(', ')}</p>
        {/if}
      </div>
    {/if}

    <!-- Collapsible settings -->
    <div>
      <button
        type="button"
        class="flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer hover:text-foreground w-full"
        onclick={() => { showSettings = !showSettings; }}
      >
        <ChevronRight class="size-4 md:size-3 transition-transform {showSettings ? 'rotate-90' : ''}" />
        <Settings2 class="size-4 md:size-3" />
        <span class="font-medium">Settings</span>
      </button>

      {#if showSettings}
        <div class="mt-3 space-y-4 pl-1">
          <!-- Subdomain -->
          {#if isConfigured && namespaceId}
            <NamespaceSubdomainManager
              {namespaceId}
              {subdomain}
              {firstPublishedAudience}
              onSubdomainChange={handleSubdomainChange}
            />
          {/if}

          <!-- Audiences -->
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-xs font-medium text-muted-foreground">Audiences</span>
              <Button
                variant="ghost"
                size="icon"
                class="size-6"
                onclick={() => { showManageAudiences = true; }}
                aria-label="Manage audiences"
              >
                <Tags class="size-3.5" />
              </Button>
            </div>

            {#if hasAnyAudience}
              <NamespaceAudienceManager
                namespaceId={namespaceId ?? ''}
                audiences={allAudiences}
                {audienceStates}
                {defaultAudience}
                onStateChange={handleAudienceStateChange}
              />
            {:else}
              <div class="space-y-2">
                <p class="text-xs text-muted-foreground">
                  No audiences configured. Add audience tags to entries or set a default audience.
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  class="text-xs"
                  onclick={() => { showDefaultAudienceInput = !showDefaultAudienceInput; }}
                >
                  <Globe class="size-3.5 mr-1.5" />
                  Set default audience
                </Button>
                {#if showDefaultAudienceInput}
                  <div class="flex gap-2">
                    <Input
                      type="text"
                      bind:value={defaultAudienceInput}
                      placeholder="e.g. public, family, friends"
                      class="h-8 text-xs flex-1"
                      onkeydown={(e) => { if (e.key === 'Enter') handleSetDefaultAudience(); }}
                    />
                    <Button
                      variant="default"
                      size="sm"
                      class="h-8 text-xs shrink-0"
                      onclick={handleSetDefaultAudience}
                      disabled={defaultAudienceInput.trim().length === 0}
                    >
                      Save
                    </Button>
                  </div>
                {/if}
              </div>
            {/if}
          </div>

          <!-- Danger zone: remove all uploaded files for this namespace. -->
          {#if isConfigured && namespaceId}
            <div class="space-y-2 border-t border-border pt-3">
              <span class="text-xs font-medium text-muted-foreground">Danger zone</span>
              {#if !showUnpublishConfirm}
                <Button
                  variant="outline"
                  size="sm"
                  class="w-full text-xs text-destructive hover:text-destructive"
                  onclick={() => { showUnpublishConfirm = true; }}
                  disabled={isUnpublishing || isPublishing}
                >
                  <Trash2 class="size-3.5 mr-1.5" />
                  Remove published files
                </Button>
              {:else}
                <div class="rounded-md border border-destructive/40 p-2.5 space-y-2">
                  <p class="text-xs text-muted-foreground">
                    Deletes every uploaded source and rendered page from your site.
                    Your audiences and site address are kept; publish again to restore.
                  </p>
                  <div class="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      class="flex-1 text-xs"
                      onclick={() => { showUnpublishConfirm = false; }}
                      disabled={isUnpublishing}
                    >
                      Cancel
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      class="flex-1 text-xs"
                      onclick={handleUnpublish}
                      disabled={isUnpublishing}
                    >
                      {#if isUnpublishing}
                        <Loader2 class="size-3.5 mr-1.5 animate-spin" />
                        Removing…
                      {:else}
                        <Trash2 class="size-3.5 mr-1.5" />
                        Delete everything
                      {/if}
                    </Button>
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- Manage Audiences Modal -->
{#if api && rootPath}
  <ManageAudiencesModal
    open={showManageAudiences}
    {api}
    {rootPath}
    onClose={() => { showManageAudiences = false; loadAudiences(); }}
  />
{/if}
