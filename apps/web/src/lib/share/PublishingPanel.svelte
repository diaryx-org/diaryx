<script lang="ts">
  import type { Api } from '$lib/backend/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Alert from '$lib/components/ui/alert';
  import * as Dialog from '$lib/components/ui/dialog';
  import NativeSelect from '$lib/components/ui/native-select/native-select.svelte';
  import { getTemplateContextStore } from '$lib/stores/templateContextStore.svelte';
  import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
  import { getAudienceColor } from '$lib/utils/audienceDotColor';
  import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
  import { workspaceStore } from '@/models/stores';
  import ManageAudiencesModal from '$lib/components/ManageAudiencesModal.svelte';
  import {
    AlertCircle,
    Check,
    Copy,
    Globe,
    KeyRound,
    Loader2,
    Lock,
    Settings2,
    ShieldOff,
    Tags,
    Upload,
  } from '@lucide/svelte';
  import * as browserPlugins from '$lib/plugins/browserPluginManager.svelte';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import { getAuthState, getServerUrl } from '$lib/auth';
  import UpgradeBanner from '$lib/components/UpgradeBanner.svelte';

  interface Props {
    onAddWorkspace?: () => void;
    api: Api | null;
  }

  let { onAddWorkspace, api }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();
  const configStore = getWorkspaceConfigStore();

  // ---- Plugin command helper (same pattern as ExportDialog) ----

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

  // Workspace root path from tree (for getAvailableAudiences)
  let rootPath = $derived(workspaceStore.tree?.path ?? null);
  let defaultAudience = $derived(configStore.config?.default_audience ?? null);
  let hasDefaultWorkspace = $derived(rootPath !== null);

  // Plugin-backed state
  let namespaceId = $state<string | null>(null);
  let subdomain = $state<string | null>(null);
  let error = $state<string | null>(null);

  // Loading flags
  let isLoading = $state(false);
  let isPublishing = $state(false);
  let isCreatingNamespace = $state(false);
  let isClaimingSubdomain = $state(false);
  let isCreatingToken = $state(false);
  let lastCreatedAccessUrl = $state<string | null>(null);

  // Subdomain input
  let showSubdomainInput = $state(false);
  let subdomainInput = $state('');

  // Audience states: audience name -> { state, access_method }
  type AudienceState = 'unpublished' | 'public' | 'access-control';
  interface AudienceConfig { state: AudienceState; access_method?: string }
  let audienceStates = $state<Record<string, AudienceConfig>>({});
  let availableAudiences = $state<string[]>([]);
  let initializedRootPath = $state<string | null>(null);

  // Manage audiences modal
  let showManageAudiences = $state(false);

  // Set default audience inline
  let showDefaultAudienceInput = $state(false);
  let defaultAudienceInput = $state('');

  // Access control dialog state
  let accessDialogOpen = $state(false);
  let accessDialogAudience = $state<string | null>(null);
  let accessDialogState = $state<AudienceState>('unpublished');
  let accessDialogMethod = $state<string>('access-key');
  let copiedAccessUrl = $state(false);
  let copiedSiteUrl = $state(false);

  let isConfigured = $derived(namespaceId !== null);

  let siteUrl = $derived.by(() => {
    if (!namespaceId) return null;
    if (subdomain) return `https://${subdomain}.diaryx.org`;
    const base = `https://diaryx.org/ns/${namespaceId}`;
    // Show URL for first published audience
    const firstPublished = Object.entries(audienceStates).find(([, c]) => c.state !== 'unpublished');
    if (firstPublished) return `${base}/${firstPublished[0]}/index.html`;
    return base;
  });

  // Combine explicit audience tags + default audience into a unified list
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

  // ---- Effects ----

  $effect(() => {
    if (!rootPath || initializedRootPath === rootPath) return;
    initializedRootPath = rootPath;
    loadPublishConfig();
  });

  // Load workspace config when rootPath becomes available
  $effect(() => {
    if (rootPath) configStore.load(rootPath);
  });

  // Reload audiences when rootPath or version bumps
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
      console.debug('[PublishingPanel] GetPublishConfig response:', config);
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

  function getAudienceState(audience: string): AudienceConfig {
    return audienceStates[audience] ?? { state: 'unpublished' };
  }

  function isDefaultOnly(audience: string): boolean {
    return audience === defaultAudience && !availableAudiences.includes(audience);
  }

  // ---- Access dialog ----

  function openAccessDialog(audience: string) {
    const config = getAudienceState(audience);
    accessDialogAudience = audience;
    accessDialogState = config.state;
    accessDialogMethod = config.access_method ?? 'access-key';
    accessDialogOpen = true;
    lastCreatedAccessUrl = null;
  }

  async function handleSaveAccessDialog() {
    if (!accessDialogAudience) return;
    const config: AudienceConfig = {
      state: accessDialogState,
      access_method: accessDialogState === 'access-control' ? accessDialogMethod : undefined,
    };
    try {
      await executePublishCommand('SetAudiencePublishState', {
        audience: accessDialogAudience,
        server_url: serverUrl,
        config: {
          state: config.state,
          access_method: config.access_method,
        },
      });
      if (config.state === 'unpublished') {
        const { [accessDialogAudience]: _, ...rest } = audienceStates;
        audienceStates = rest;
      } else {
        audienceStates = { ...audienceStates, [accessDialogAudience]: config };
      }
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to save audience state', 'Publishing');
    }
    accessDialogOpen = false;
  }

  // ---- Publish ----

  async function handlePublish() {
    error = null;

    // Auto-create namespace if not configured
    if (!isConfigured) {
      isCreatingNamespace = true;
      try {
        const result = await executePublishCommand<{ namespace_id: string }>(
          'CreateNamespace',
          { server_url: serverUrl },
        );
        namespaceId = result.namespace_id;
      } catch (e) {
        showError(e instanceof Error ? e.message : 'Failed to create namespace', 'Publishing');
        isCreatingNamespace = false;
        return;
      } finally {
        isCreatingNamespace = false;
      }
    }

    // Publish via plugin — single call, plugin loops audiences
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

  // ---- Access token ----

  async function handleCreateToken() {
    if (!accessDialogAudience || !isConfigured) return;
    isCreatingToken = true;
    try {
      const result = await executePublishCommand<{ token: string; access_url: string }>(
        'CreateAccessToken',
        { server_url: serverUrl, audience: accessDialogAudience },
      );
      lastCreatedAccessUrl = result.access_url;
      showSuccess('Access link generated');
      showInfo('Copy the access URL now. It is only shown once.');
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to create token', 'Publishing');
    } finally {
      isCreatingToken = false;
    }
  }

  // ---- Subdomain ----

  async function handleClaimSubdomain() {
    const value = subdomainInput.trim().toLowerCase();
    if (!value || !isConfigured) return;
    isClaimingSubdomain = true;
    try {
      // Use first published audience as default
      const firstPublished = Object.entries(audienceStates).find(([, c]) => c.state !== 'unpublished');
      await executePublishCommand('ClaimSubdomain', {
        server_url: serverUrl,
        subdomain: value,
        default_audience: firstPublished?.[0],
      });
      subdomain = value;
      showSubdomainInput = false;
      subdomainInput = '';
      showSuccess(`Subdomain claimed: ${value}.diaryx.org`);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to claim subdomain', 'Publishing');
    } finally {
      isClaimingSubdomain = false;
    }
  }

  async function handleReleaseSubdomain() {
    if (!isConfigured || !subdomain) return;
    try {
      await executePublishCommand('ReleaseSubdomain', { server_url: serverUrl });
      subdomain = null;
      showSuccess('Subdomain released');
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to release subdomain', 'Publishing');
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

  // ---- Clipboard ----

  async function copyToClipboard(value: string, flag: 'access-url' | 'site-url') {
    try {
      await navigator.clipboard.writeText(value);
      if (flag === 'access-url') {
        copiedAccessUrl = true;
        setTimeout(() => { copiedAccessUrl = false; }, 1800);
      } else {
        copiedSiteUrl = true;
        setTimeout(() => { copiedSiteUrl = false; }, 1800);
      }
    } catch {
      showError('Copy failed. Check browser clipboard permissions.', 'Publishing');
    }
  }

  function handleOpenSyncSetup() {
    if (onAddWorkspace) {
      onAddWorkspace();
      return;
    }
    showInfo('Open account or sync settings to enable faster server-side publishing.');
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
    <!-- Header: Publish as a site -->
    <div class="space-y-1">
      <h3 class="font-medium text-sm">Publish as a site</h3>
      {#if siteUrl}
        <div class="flex items-center gap-1.5">
          <p class="text-xs text-muted-foreground font-mono truncate flex-1">{siteUrl}</p>
          <Button
            variant="ghost"
            size="icon"
            class="size-6 shrink-0"
            onclick={() => copyToClipboard(siteUrl!, 'site-url')}
            aria-label="Copy site URL"
          >
            {#if copiedSiteUrl}
              <Check class="size-3" />
            {:else}
              <Copy class="size-3" />
            {/if}
          </Button>
        </div>
      {/if}
    </div>

    <!-- Subdomain -->
    {#if isConfigured}
      {#if subdomain}
        <div class="flex items-center gap-2 text-xs">
          <span class="text-muted-foreground">Subdomain:</span>
          <span class="font-mono font-medium">{subdomain}.diaryx.org</span>
          <Button
            variant="ghost"
            size="sm"
            class="h-6 text-xs text-muted-foreground ml-auto px-2"
            onclick={handleReleaseSubdomain}
          >
            Release
          </Button>
        </div>
      {:else if showSubdomainInput}
        <div class="space-y-1.5">
          <p class="text-xs text-muted-foreground">Choose a subdomain for your site</p>
          <div class="flex gap-2 items-center">
            <Input
              type="text"
              bind:value={subdomainInput}
              placeholder="my-site"
              class="h-8 text-xs flex-1 font-mono"
              onkeydown={(e) => { if (e.key === 'Enter') handleClaimSubdomain(); }}
            />
            <span class="text-xs text-muted-foreground shrink-0">.diaryx.org</span>
          </div>
          <div class="flex gap-2">
            <Button
              variant="default"
              size="sm"
              class="h-7 text-xs"
              onclick={handleClaimSubdomain}
              disabled={subdomainInput.trim().length < 3 || isClaimingSubdomain}
            >
              {#if isClaimingSubdomain}
                <Loader2 class="size-3 mr-1 animate-spin" />
              {/if}
              Claim
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 text-xs"
              onclick={() => { showSubdomainInput = false; }}
            >
              Cancel
            </Button>
          </div>
        </div>
      {:else}
        <Button
          variant="outline"
          size="sm"
          class="w-full text-xs"
          onclick={() => { showSubdomainInput = true; }}
        >
          <Globe class="size-3.5 mr-1.5" />
          Set custom subdomain
        </Button>
      {/if}
    {/if}

    <!-- Audience list or empty state -->
    {#if !hasAnyAudience}
      <!-- Private workspace: no audiences, no default audience -->
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
      <!-- Audience tags list -->
      <div class="space-y-1.5">
        <div class="flex items-center justify-between">
          <p class="text-xs font-medium text-muted-foreground">Audience tags</p>
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

        <div class="space-y-1">
          {#each allAudiences as audience}
            {@const config = getAudienceState(audience)}
            {@const dotColor = getAudienceColor(audience, colorStore.audienceColors)}
            {@const isDefault = isDefaultOnly(audience)}
            <button
              class="w-full flex items-center gap-2 px-2.5 py-2 rounded-md border border-border bg-background hover:bg-secondary transition-colors text-left"
              onclick={() => openAccessDialog(audience)}
            >
              <span class="size-2.5 rounded-full shrink-0 {dotColor}"></span>
              <span class="text-sm font-medium flex-1 truncate">
                {audience}
                {#if isDefault}
                  <span class="text-xs font-normal text-muted-foreground">(default)</span>
                {/if}
              </span>
              <span class="text-xs text-muted-foreground flex items-center gap-1">
                {#if config.state === 'public'}
                  <Globe class="size-3" />
                  Public
                {:else if config.state === 'access-control'}
                  <Lock class="size-3" />
                  Access Key
                {:else}
                  <span class="text-muted-foreground/60">Unpublished</span>
                {/if}
              </span>
              <Settings2 class="size-3.5 text-muted-foreground/50" />
            </button>
          {/each}
        </div>
      </div>

      <!-- Publish button -->
      <Button
        class="w-full"
        variant="default"
        onclick={handlePublish}
        disabled={!canPublish}
      >
        {#if isPublishing}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Publishing...
        {:else if isCreatingNamespace}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Setting up namespace...
        {:else}
          <Upload class="size-4 mr-2" />
          Publish
          {#if publishedAudienceCount > 0}
            ({publishedAudienceCount} {publishedAudienceCount === 1 ? 'audience' : 'audiences'})
          {/if}
        {/if}
      </Button>
    {/if}
  {/if}
</div>

<!-- Access control dialog -->
<Dialog.Root bind:open={accessDialogOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-base">
        {#if accessDialogAudience}
          {@const dotColor = getAudienceColor(accessDialogAudience, colorStore.audienceColors)}
          <span class="size-2.5 rounded-full {dotColor}"></span>
        {/if}
        {accessDialogAudience}
      </Dialog.Title>
      <Dialog.Description class="text-xs text-muted-foreground">
        Configure how this audience tag is published.
      </Dialog.Description>
    </Dialog.Header>

    <div class="space-y-3 py-2">
      <!-- State selector -->
      <div class="space-y-2">
        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'unpublished' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'unpublished'; }}
        >
          <div class="flex-1">
            <p class="text-sm font-medium">Unpublished</p>
            <p class="text-xs text-muted-foreground">This audience is not included when publishing.</p>
          </div>
        </button>

        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'public' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'public'; }}
        >
          <Globe class="size-4 text-muted-foreground shrink-0" />
          <div class="flex-1">
            <p class="text-sm font-medium">Public</p>
            <p class="text-xs text-muted-foreground">Anyone with the link can view.</p>
          </div>
        </button>

        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'access-control' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'access-control'; }}
        >
          <Lock class="size-4 text-muted-foreground shrink-0" />
          <div class="flex-1">
            <p class="text-sm font-medium">Access Control</p>
            <p class="text-xs text-muted-foreground">Restrict access with a key link.</p>
          </div>
        </button>
      </div>

      <!-- Access control options (shown when access-control selected) -->
      {#if accessDialogState === 'access-control'}
        <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
          <div class="space-y-1.5">
            <label for="access-method" class="text-xs font-medium text-muted-foreground">Method</label>
            <NativeSelect id="access-method" bind:value={accessDialogMethod} class="w-full h-8 text-xs">
              <option value="access-key">Access Key Link</option>
            </NativeSelect>
          </div>

          {#if accessDialogMethod === 'access-key' && isConfigured}
            <div class="space-y-2">
              <Button
                variant="secondary"
                size="sm"
                class="w-full h-8 text-xs"
                onclick={handleCreateToken}
                disabled={isCreatingToken}
              >
                {#if isCreatingToken}
                  <Loader2 class="size-3.5 mr-1 animate-spin" />
                {:else}
                  <KeyRound class="size-3.5 mr-1" />
                {/if}
                Generate Access Link
              </Button>

              {#if lastCreatedAccessUrl}
                <Alert.Root class="py-2 border border-primary/30 bg-secondary">
                  <Alert.Description class="text-xs space-y-2">
                    <p class="font-medium text-foreground">Access URL (shown once)</p>
                    <code class="block text-[11px] break-all bg-background rounded p-2 border border-border">{lastCreatedAccessUrl}</code>
                    <div class="flex gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        class="h-7 text-xs"
                        onclick={() => copyToClipboard(lastCreatedAccessUrl!, 'access-url')}
                      >
                        {#if copiedAccessUrl}
                          <Check class="size-3.5 mr-1" /> Copied
                        {:else}
                          <Copy class="size-3.5 mr-1" /> Copy URL
                        {/if}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-7 text-xs"
                        onclick={() => { lastCreatedAccessUrl = null; }}
                      >
                        Dismiss
                      </Button>
                    </div>
                  </Alert.Description>
                </Alert.Root>
              {/if}
            </div>
          {:else if accessDialogMethod === 'access-key' && !isConfigured}
            <p class="text-xs text-muted-foreground">Publish the site first to generate access links.</p>
          {/if}
        </div>
      {/if}
    </div>

    <Dialog.Footer>
      <Button variant="outline" size="sm" onclick={() => { accessDialogOpen = false; }}>
        Cancel
      </Button>
      <Button size="sm" onclick={handleSaveAccessDialog}>
        Save
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Manage Audiences Modal -->
{#if api && rootPath}
  <ManageAudiencesModal
    open={showManageAudiences}
    {api}
    {rootPath}
    onClose={() => { showManageAudiences = false; loadAudiences(); }}
  />
{/if}
