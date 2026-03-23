/**
 * Namespace context store — shared reactive state for namespace host widgets.
 *
 * Multiple HostWidget components in the same sidebar panel share this context
 * so they stay in sync (e.g. audience manager + publish button + site URL).
 *
 * Uses Svelte's getContext/setContext for component-tree scoping.
 */

import { getContext, setContext } from 'svelte';
import type { Api } from '$lib/backend/api';
import type { HostAction } from '$lib/backend/generated';
import * as browserPlugins from '$lib/plugins/browserPluginManager.svelte';
import { getAuthState, getServerUrl } from '$lib/auth';
import { getTemplateContextStore } from '$lib/stores/templateContextStore.svelte';
import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
import { workspaceStore } from '@/models/stores';
import { showError, showSuccess, showInfo } from '@/models/services/toastService';
import { createNamespace } from './namespaceService';

const CONTEXT_KEY = Symbol('namespace-context');

export type AudienceConfig = {
  state: string;
  access_method?: string;
  email_on_publish?: boolean;
  email_subject?: string;
  email_cover?: string;
};

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

export class NamespaceContext {
  // --- Injected deps (reactive so effects re-trigger on init) ---
  api = $state<Api | null>(null);
  #onHostAction: ((action: { type: string; payload?: unknown }) => void) | undefined;
  signInAction = $state<HostAction | null>(null);

  // --- Reactive state ---
  namespaceId = $state<string | null>(null);
  subdomain = $state<string | null>(null);
  error = $state<string | null>(null);
  audienceStates = $state<Record<string, AudienceConfig>>({});
  availableAudiences = $state<string[]>([]);
  isLoading = $state(false);
  isPublishing = $state(false);
  isCreatingNamespace = $state(false);

  // Manage audiences modal
  showManageAudiences = $state(false);
  // Default audience input
  showDefaultAudienceInput = $state(false);
  defaultAudienceInput = $state('');

  // Server capabilities (fetched once)
  siteBaseUrl = $state<string | null>(null);
  siteDomain = $state<string | null>(null);
  subdomainsAvailable = $state(false);
  customDomainsAvailable = $state(false);

  #initializedRootPath: string | null = null;

  // --- Derived ---
  get authState() { return getAuthState(); }
  get isAuthenticated() { return this.authState.isAuthenticated; }
  get serverUrl() { return getServerUrl() ?? ''; }
  get rootPath() { return workspaceStore.tree?.path ?? null; }
  get configStore() { return getWorkspaceConfigStore(); }
  get colorStore() { return getAudienceColorStore(); }
  get templateContextStore() { return getTemplateContextStore(); }
  get defaultAudience() { return this.configStore.config?.default_audience ?? null; }
  get hasDefaultWorkspace() { return this.rootPath !== null; }
  get isConfigured() { return this.namespaceId !== null; }

  /** True once initial loading is complete and auth/workspace guards pass. */
  get isReady() {
    return (
      !this.isLoading
      && this.hasDefaultWorkspace
      && this.isAuthenticated
      && this.hasPublishingAccess
    );
  }

  get allAudiences(): string[] {
    const set = new Set(this.availableAudiences);
    if (this.defaultAudience && !set.has(this.defaultAudience)) {
      set.add(this.defaultAudience);
    }
    return [...set];
  }

  get hasAnyAudience() { return this.allAudiences.length > 0; }

  get publishedAudienceCount() {
    return Object.values(this.audienceStates).filter(c => c.state !== 'unpublished').length;
  }

  /** Expose the host action callback for widgets that need to open app-level UI. */
  get hostAction() { return this.#onHostAction; }

  get hasPublishingAccess() {
    return this.isConfigured || this.authState.publishedSiteLimit > 0;
  }

  get canPublish() {
    return (
      this.hasDefaultWorkspace
      && !this.isPublishing
      && !this.isLoading
      && !this.isCreatingNamespace
      && this.isAuthenticated
      && this.hasPublishingAccess
      && this.publishedAudienceCount > 0
    );
  }

  get firstPublishedAudience(): string | undefined {
    const entry = Object.entries(this.audienceStates).find(([, c]) => c.state !== 'unpublished');
    return entry?.[0];
  }

  // --- Setup ---

  init(newApi: Api | null, onHostAction?: (action: { type: string; payload?: unknown }) => void) {
    this.api = newApi;
    this.#onHostAction = onHostAction;
  }

  /** Called by the first widget that mounts. Loads config if rootPath changed. */
  tryLoad() {
    const rp = this.rootPath;
    // Wait until api is injected via init() before attempting to load.
    if (!rp || !this.api) return;
    if (this.#initializedRootPath === rp) return;
    this.#initializedRootPath = rp;
    this.loadPublishConfig();
    this.configStore.load(rp);
    this.loadCapabilities();
  }

  /** Fetch server capabilities (site URL, subdomain availability). */
  private async loadCapabilities() {
    try {
      const serverUrl = this.serverUrl;
      if (!serverUrl) return;
      const resp = await fetch(`${serverUrl}/capabilities`);
      if (resp.ok) {
        const caps = await resp.json();
        this.siteBaseUrl = caps.site_base_url ?? null;
        this.siteDomain = caps.site_domain ?? null;
        this.subdomainsAvailable = caps.subdomains_available ?? false;
        this.customDomainsAvailable = caps.custom_domains_available ?? false;
      }
    } catch {
      // Best effort — capabilities are optional enhancements
    }
  }

  loadAudiences() {
    const rp = this.rootPath;
    if (!this.api || !rp) return;
    this.api.getAvailableAudiences(rp).then(audiences => {
      this.availableAudiences = audiences;
      for (const name of audiences) this.colorStore.assignColor(name);
    }).catch(() => {
      this.availableAudiences = [];
    });
  }

  // --- Plugin command helper ---

  async executePublishCommand<T = any>(
    command: string,
    params: Record<string, any> = {},
  ): Promise<T> {
    if (!this.api) throw new Error('Publish API unavailable');

    const browserPublish = browserPlugins.getPlugin('diaryx.publish');
    if (browserPublish) {
      const result = await browserPlugins.dispatchCommand('diaryx.publish', command, params);
      if (!result.success) {
        throw new Error(result.error ?? `Publish command failed: ${command}`);
      }
      return normalizeToObject(result.data) as T;
    }

    const data = await this.api.executePluginCommand('diaryx.publish', command, params as any);
    return normalizeToObject(data) as T;
  }

  // --- Data loading ---

  async loadPublishConfig() {
    if (!this.api) return;
    this.isLoading = true;
    this.error = null;
    try {
      const config = await this.executePublishCommand<{
        namespace_id?: string | null;
        subdomain?: string | null;
        audience_states?: Record<string, AudienceConfig>;
      }>('GetPublishConfig', {});
      this.namespaceId = config.namespace_id ?? null;
      this.subdomain = config.subdomain ?? null;
      this.audienceStates = config.audience_states ?? {};
    } catch (e) {
      this.error = e instanceof Error ? e.message : 'Failed to load publish config';
      this.namespaceId = null;
      this.subdomain = null;
      this.audienceStates = {};
    } finally {
      this.isLoading = false;
    }
  }

  // --- Actions ---

  async handleAudienceStateChange(audience: string, config: AudienceConfig) {
    try {
      await this.executePublishCommand('SetAudiencePublishState', {
        audience,
        server_url: this.serverUrl,
        config: {
          state: config.state,
          access_method: config.access_method,
          email_on_publish: config.email_on_publish,
          email_subject: config.email_subject,
          email_cover: config.email_cover,
        },
      });
    } catch {
      // Best effort — server sync already happened in the component
    }

    if (config.state === 'unpublished') {
      const { [audience]: _, ...rest } = this.audienceStates;
      this.audienceStates = rest;
    } else {
      this.audienceStates = { ...this.audienceStates, [audience]: config };
    }
  }

  async handleSendEmail(audience: string) {
    if (!this.namespaceId) return;
    try {
      await this.executePublishCommand('SendEmailToAudience', {
        namespace_id: this.namespaceId,
        audience,
      });
    } catch (e) {
      throw e;
    }
  }

  handleSubdomainChange(newSubdomain: string | null) {
    this.subdomain = newSubdomain;
  }

  async handlePublish() {
    this.error = null;

    if (!this.isConfigured) {
      this.isCreatingNamespace = true;
      try {
        const ns = await createNamespace();
        this.namespaceId = ns.id;
        await this.executePublishCommand('SetPublishConfig', {
          namespace_id: ns.id,
          subdomain: this.subdomain,
          audience_states: this.audienceStates,
          public_audiences: Object.entries(this.audienceStates)
            .filter(([, c]) => c.state === 'public')
            .map(([name]) => name),
        });
      } catch (e) {
        showError(e instanceof Error ? e.message : 'Failed to create namespace', 'Publishing');
        this.isCreatingNamespace = false;
        return;
      } finally {
        this.isCreatingNamespace = false;
      }
    }

    this.isPublishing = true;
    try {
      const result = await this.executePublishCommand<{
        audiences_published: string[];
        files_uploaded: number;
        files_deleted: number;
      }>('PublishToNamespace', {
        namespace_id: this.namespaceId,
        server_url: this.serverUrl,
      });
      showSuccess(`Published ${result.audiences_published.length} audience(s)`);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Publish failed', 'Publishing');
    } finally {
      this.isPublishing = false;
    }
  }

  async handleSetDefaultAudience() {
    const value = this.defaultAudienceInput.trim();
    if (!value) return;
    await this.configStore.setField('default_audience', value);
    this.colorStore.assignColor(value);
    this.showDefaultAudienceInput = false;
    this.defaultAudienceInput = '';
    this.templateContextStore.bumpAudiencesVersion();
  }

  handleOpenSyncSetup() {
    if (this.signInAction && this.#onHostAction) {
      this.#onHostAction({
        type: this.signInAction.action_type,
        payload: this.signInAction.payload ?? undefined,
      });
      return;
    }
    if (this.#onHostAction) {
      this.#onHostAction({ type: 'open-add-workspace' });
      return;
    }
    showInfo('Open account or sync settings to enable faster server-side publishing.');
  }
}

/** Create and set a new NamespaceContext in the component tree. */
export function createNamespaceContext(): NamespaceContext {
  const ctx = new NamespaceContext();
  setContext(CONTEXT_KEY, ctx);
  return ctx;
}

/** Get the existing NamespaceContext from a parent component. */
export function getNamespaceContext(): NamespaceContext {
  return getContext<NamespaceContext>(CONTEXT_KEY);
}
