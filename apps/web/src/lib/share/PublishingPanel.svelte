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
    Globe,
    Loader2,
    Settings2,
    ShieldOff,
    Tags,
  } from '@lucide/svelte';
  import * as browserPlugins from '$lib/plugins/browserPluginManager.svelte';
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

  // ---- Plugin command helper ----

  function normalizeToObject(value: any): any {
    if (value instanceof Map) {
      const obj: Record<string, any> = {};
      for (const [k, v] of value.entries()) {
        obj[k] = normalizeToObject(v);
      }
      return obj;
    }
    if (Array.isArray(value)) {
      return value.map(normalizeToObject);
    }
    return value;
  }

  async function executePublishCommand<T = any>(
    command: string,
    params: Record<string, any> = {},
  ): Promise<T> {
    if (!api) throw new Error('Publish API unavailable');

    const browserPublish = browserPlugins.getPlugin('diaryx.publish');
    if (browserPublish) {
      const result = await browserPlugins.dispatchCommand('diaryx.publish', command, params);
      if (!result.success) {
        throw new Error(result.error ?? `Publish command failed: ${command}`);
      }
      return normalizeToObject(result.data) as T;
    }

    const data = await api.executePluginCommand('diaryx.publish', command, params as any);
    return normalizeToObject(data) as T;
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

  type AudienceConfig = { state: string; access_method?: string };
  let audienceStates = $state<Record<string, AudienceConfig>>({});
  let availableAudiences = $state<string[]>([]);
  let initializedRootPath = $state<string | null>(null);

  // Loading flags
  let isLoading = $state(false);
  let isPublishing = $state(false);
  let isCreatingNamespace = $state(false);

  // Manage audiences modal
  let showManageAudiences = $state(false);

  // Set default audience inline
  let showDefaultAudienceInput = $state(false);
  let defaultAudienceInput = $state('');

  let isConfigured = $derived(namespaceId !== null);

  let allAudiences = $derived.by(() => {
    const set = new Set(availableAudiences);
    if (defaultAudience && !set.has(defaultAudience)) {
      set.add(defaultAudience);
    }
    return [...set];
  });

  let hasAnyAudience = $derived(allAudiences.length > 0);

  let publishedAudienceCount = $derived(
    Object.values(audienceStates).filter(c => c.state !== 'unpublished').length
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
    && publishedAudienceCount > 0
  );

  let firstPublishedAudience = $derived.by(() => {
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

  // ---- Data loading ----

  async function loadPublishConfig() {
    if (!api) return;
    isLoading = true;
    error = null;
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
      error = e instanceof Error ? e.message : 'Failed to load publish config';
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

  async function handlePublish() {
    error = null;

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
        showError(e instanceof Error ? e.message : 'Failed to create namespace', 'Publishing');
        isCreatingNamespace = false;
        return;
      } finally {
        isCreatingNamespace = false;
      }
    }

    isPublishing = true;
    try {
      const result = await executePublishCommand<{
        audiences_published: string[];
        files_uploaded: number;
        files_deleted: number;
      }>('PublishToNamespace', {
        namespace_id: namespaceId,
        server_url: serverUrl,
      });
      showSuccess(`Published ${result.audiences_published.length} audience(s)`);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Publish failed', 'Publishing');
    } finally {
      isPublishing = false;
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
      <Alert.Description class="text-xs">{error}</Alert.Description>
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

    <!-- Subdomain -->
    {#if isConfigured && namespaceId}
      <NamespaceSubdomainManager
        {namespaceId}
        {subdomain}
        {firstPublishedAudience}
        onSubdomainChange={handleSubdomainChange}
      />
    {/if}

    <!-- Audience list or empty state -->
    {#if !hasAnyAudience}
      <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
        <div class="flex items-start gap-2.5">
          <ShieldOff class="size-5 text-muted-foreground shrink-0 mt-0.5" />
          <div class="space-y-1">
            <p class="text-sm font-medium">Your workspace is private</p>
            <p class="text-xs text-muted-foreground">
              To share your workspace, add audience tags to your entries or set a default audience for the entire workspace.
            </p>
          </div>
        </div>

        <div class="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            class="text-xs"
            onclick={() => { showManageAudiences = true; }}
          >
            <Tags class="size-3.5 mr-1.5" />
            Add audience tags
          </Button>
          <Button
            variant="outline"
            size="sm"
            class="text-xs"
            onclick={() => { showDefaultAudienceInput = !showDefaultAudienceInput; }}
          >
            <Globe class="size-3.5 mr-1.5" />
            Set default audience
          </Button>
        </div>

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
    {:else}
      <!-- Manage audiences button -->
      <div class="flex items-center justify-end">
        <Button
          variant="ghost"
          size="icon"
          class="size-6"
          onclick={() => { showManageAudiences = true; }}
          aria-label="Manage audiences"
        >
          <Settings2 class="size-3.5" />
        </Button>
      </div>

      <NamespaceAudienceManager
        namespaceId={namespaceId ?? ''}
        audiences={allAudiences}
        {audienceStates}
        {defaultAudience}
        onStateChange={handleAudienceStateChange}
        onSendEmail={async (audience) => {
          if (!namespaceId) return;
          try {
            await executePublishCommand('SendEmailToAudience', {
              namespace_id: namespaceId,
              audience,
            });
            showSuccess(`Email sent to "${audience}" subscribers`);
          } catch (e) {
            showError(e instanceof Error ? e.message : 'Failed to send email', 'Email');
          }
        }}
      />

      <NamespacePublishButton
        {namespaceId}
        {canPublish}
        {publishedAudienceCount}
        {isPublishing}
        {isCreatingNamespace}
        onPublish={handlePublish}
      />
    {/if}
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
