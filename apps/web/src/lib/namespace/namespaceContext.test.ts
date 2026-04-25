import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------

const mockGetAuthState = vi.hoisted(() =>
  vi.fn().mockReturnValue({
    isAuthenticated: true,
    publishedSiteLimit: 5,
  }),
);

const mockGetServerUrl = vi.hoisted(() =>
  vi.fn().mockReturnValue('https://server.example.com'),
);

const mockGetPlugin = vi.hoisted(() => vi.fn().mockReturnValue(null));
const mockDispatchCommand = vi.hoisted(() => vi.fn());

const mockWorkspaceStore = vi.hoisted(() => ({
  workspaceStore: {
    tree: { path: '/workspace/root' },
  },
}));

const mockSetContext = vi.hoisted(() => vi.fn());
const mockGetContext = vi.hoisted(() => vi.fn());

const mockConfigStore = vi.hoisted(() => ({
  config: { default_audience: 'public' } as { default_audience: string } | null,
  load: vi.fn(),
  setField: vi.fn().mockResolvedValue(undefined),
}));

const mockColorStore = vi.hoisted(() => ({
  assignColor: vi.fn(),
}));

const mockTemplateContextStore = vi.hoisted(() => ({
  bumpAudiencesVersion: vi.fn(),
}));

const mockShowError = vi.hoisted(() => vi.fn());
const mockShowSuccess = vi.hoisted(() => vi.fn());
const mockShowInfo = vi.hoisted(() => vi.fn());

const mockProxyFetch = vi.hoisted(() => vi.fn());

const mockCreateNamespace = vi.hoisted(() =>
  vi.fn().mockResolvedValue({ id: 'ns-123', owner_user_id: 'user-1', created_at: 1000 }),
);

// ---------------------------------------------------------------------------
// vi.mock calls
// ---------------------------------------------------------------------------

vi.mock('svelte', () => ({
  getContext: mockGetContext,
  setContext: mockSetContext,
}));

vi.mock('$lib/auth', () => ({
  getAuthState: mockGetAuthState,
  getServerUrl: mockGetServerUrl,
}));

vi.mock('$lib/plugins/browserPluginManager.svelte', () => ({
  getPlugin: mockGetPlugin,
  dispatchCommand: mockDispatchCommand,
}));

vi.mock('@/models/stores', () => mockWorkspaceStore);

vi.mock('@/models/services/toastService', () => ({
  showError: mockShowError,
  showSuccess: mockShowSuccess,
  showInfo: mockShowInfo,
}));

vi.mock('$lib/backend/proxyFetch', () => ({
  proxyFetch: mockProxyFetch,
}));

vi.mock('./namespaceService', () => ({
  createNamespace: mockCreateNamespace,
}));

vi.mock('$lib/stores/workspaceConfigStore.svelte', () => ({
  getWorkspaceConfigStore: () => mockConfigStore,
}));

vi.mock('$lib/stores/audienceColorStore.svelte', () => ({
  getAudienceColorStore: () => mockColorStore,
}));

vi.mock('$lib/stores/templateContextStore.svelte', () => ({
  getTemplateContextStore: () => mockTemplateContextStore,
}));

// ---------------------------------------------------------------------------
// Import SUT
// ---------------------------------------------------------------------------

import {
  NamespaceContext,
  createNamespaceContext,
  getNamespaceContext,
} from './namespaceContext.svelte';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createCtx() {
  return new NamespaceContext();
}

function createMockApi(overrides: Record<string, any> = {}) {
  return {
    getAvailableAudiences: vi.fn().mockResolvedValue(['public', 'members']),
    executePluginCommand: vi.fn().mockResolvedValue({}),
    ...overrides,
  } as any;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('NamespaceContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetAuthState.mockReturnValue({
      isAuthenticated: true,
      publishedSiteLimit: 5,
    });
    mockGetServerUrl.mockReturnValue('https://server.example.com');
    mockWorkspaceStore.workspaceStore.tree = { path: '/workspace/root' };
    mockGetPlugin.mockReturnValue(null);
    mockConfigStore.config = { default_audience: 'public' };
  });

  // -----------------------------------------------------------------------
  // Initial state
  // -----------------------------------------------------------------------

  describe('initial state', () => {
    it('has null/empty default values', () => {
      const ctx = createCtx();
      expect(ctx.api).toBeNull();
      expect(ctx.namespaceId).toBeNull();
      expect(ctx.subdomain).toBeNull();
      expect(ctx.error).toBeNull();
      expect(ctx.audienceStates).toEqual({});
      expect(ctx.availableAudiences).toEqual([]);
      expect(ctx.isLoading).toBe(false);
      expect(ctx.isPublishing).toBe(false);
      expect(ctx.isCreatingNamespace).toBe(false);
      expect(ctx.showManageAudiences).toBe(false);
      expect(ctx.showDefaultAudienceInput).toBe(false);
      expect(ctx.defaultAudienceInput).toBe('');
      expect(ctx.siteBaseUrl).toBeNull();
      expect(ctx.siteDomain).toBeNull();
      expect(ctx.subdomainsAvailable).toBe(false);
      expect(ctx.customDomainsAvailable).toBe(false);
      expect(ctx.signInAction).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // Derived getters
  // -----------------------------------------------------------------------

  describe('derived getters', () => {
    it('authState returns getAuthState()', () => {
      const ctx = createCtx();
      expect(ctx.authState).toEqual({ isAuthenticated: true, publishedSiteLimit: 5 });
    });

    it('isAuthenticated delegates to authState', () => {
      const ctx = createCtx();
      expect(ctx.isAuthenticated).toBe(true);

      mockGetAuthState.mockReturnValue({ isAuthenticated: false, publishedSiteLimit: 0 });
      expect(ctx.isAuthenticated).toBe(false);
    });

    it('serverUrl returns getServerUrl or empty string', () => {
      const ctx = createCtx();
      expect(ctx.serverUrl).toBe('https://server.example.com');

      mockGetServerUrl.mockReturnValue(null);
      expect(ctx.serverUrl).toBe('');
    });

    it('rootPath returns workspace tree path', () => {
      const ctx = createCtx();
      expect(ctx.rootPath).toBe('/workspace/root');
    });

    it('rootPath returns null when tree is null', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      expect(ctx.rootPath).toBeNull();
    });

    it('defaultAudience returns config value', () => {
      const ctx = createCtx();
      expect(ctx.defaultAudience).toBe('public');
    });

    it('defaultAudience returns null when config is missing', () => {
      mockConfigStore.config = null;
      const ctx = createCtx();
      expect(ctx.defaultAudience).toBeNull();
    });

    it('hasDefaultWorkspace is true when rootPath exists', () => {
      const ctx = createCtx();
      expect(ctx.hasDefaultWorkspace).toBe(true);
    });

    it('hasDefaultWorkspace is false when rootPath is null', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      expect(ctx.hasDefaultWorkspace).toBe(false);
    });

    it('isConfigured is true when namespaceId is set', () => {
      const ctx = createCtx();
      expect(ctx.isConfigured).toBe(false);
      ctx.namespaceId = 'ns-1';
      expect(ctx.isConfigured).toBe(true);
    });
  });

  // -----------------------------------------------------------------------
  // isReady
  // -----------------------------------------------------------------------

  describe('isReady', () => {
    it('returns true when all conditions met', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1'; // hasPublishingAccess
      ctx.isLoading = false;
      expect(ctx.isReady).toBe(true);
    });

    it('returns false when isLoading', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.isLoading = true;
      expect(ctx.isReady).toBe(false);
    });

    it('returns false when not authenticated', () => {
      mockGetAuthState.mockReturnValue({ isAuthenticated: false, publishedSiteLimit: 0 });
      const ctx = createCtx();
      expect(ctx.isReady).toBe(false);
    });

    it('returns false when no workspace', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      expect(ctx.isReady).toBe(false);
    });

    it('returns false when no publishing access', () => {
      mockGetAuthState.mockReturnValue({ isAuthenticated: true, publishedSiteLimit: 0 });
      const ctx = createCtx();
      // namespaceId is null and publishedSiteLimit is 0
      expect(ctx.isReady).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // allAudiences
  // -----------------------------------------------------------------------

  describe('allAudiences', () => {
    it('merges availableAudiences and defaultAudience', () => {
      const ctx = createCtx();
      ctx.availableAudiences = ['members'];
      // defaultAudience is 'public' from config
      expect(ctx.allAudiences).toEqual(['members', 'public']);
    });

    it('does not duplicate if defaultAudience already in availableAudiences', () => {
      const ctx = createCtx();
      ctx.availableAudiences = ['public', 'members'];
      expect(ctx.allAudiences).toEqual(['public', 'members']);
    });

    it('returns only availableAudiences when no defaultAudience', () => {
      mockConfigStore.config = null;
      const ctx = createCtx();
      ctx.availableAudiences = ['alpha'];
      expect(ctx.allAudiences).toEqual(['alpha']);
    });
  });

  // -----------------------------------------------------------------------
  // hasAnyAudience
  // -----------------------------------------------------------------------

  describe('hasAnyAudience', () => {
    it('returns false when no audiences', () => {
      mockConfigStore.config = null;
      const ctx = createCtx();
      expect(ctx.hasAnyAudience).toBe(false);
    });

    it('returns true when audiences exist', () => {
      const ctx = createCtx();
      ctx.availableAudiences = ['a'];
      expect(ctx.hasAnyAudience).toBe(true);
    });
  });

  // -----------------------------------------------------------------------
  // publishedAudienceCount
  // -----------------------------------------------------------------------

  describe('publishedAudienceCount', () => {
    it('counts non-unpublished audience states', () => {
      const ctx = createCtx();
      ctx.audienceStates = {
        public: { state: 'public' },
        members: { state: 'unpublished' },
        vip: { state: 'private' },
      };
      expect(ctx.publishedAudienceCount).toBe(2);
    });

    it('returns 0 when all unpublished', () => {
      const ctx = createCtx();
      ctx.audienceStates = {
        public: { state: 'unpublished' },
      };
      expect(ctx.publishedAudienceCount).toBe(0);
    });
  });

  // -----------------------------------------------------------------------
  // hasPublishingAccess
  // -----------------------------------------------------------------------

  describe('hasPublishingAccess', () => {
    it('returns true when namespace is configured', () => {
      mockGetAuthState.mockReturnValue({ isAuthenticated: true, publishedSiteLimit: 0 });
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      expect(ctx.hasPublishingAccess).toBe(true);
    });

    it('returns true when publishedSiteLimit > 0', () => {
      const ctx = createCtx();
      expect(ctx.hasPublishingAccess).toBe(true);
    });

    it('returns false when neither configured nor has limit', () => {
      mockGetAuthState.mockReturnValue({ isAuthenticated: true, publishedSiteLimit: 0 });
      const ctx = createCtx();
      expect(ctx.hasPublishingAccess).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // canPublish
  // -----------------------------------------------------------------------

  describe('canPublish', () => {
    it('returns true when all conditions met', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      expect(ctx.canPublish).toBe(true);
    });

    it('returns false when no published audiences', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      expect(ctx.canPublish).toBe(false);
    });

    it('returns false when isPublishing', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      ctx.isPublishing = true;
      expect(ctx.canPublish).toBe(false);
    });

    it('returns false when isLoading', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      ctx.isLoading = true;
      expect(ctx.canPublish).toBe(false);
    });

    it('returns false when isCreatingNamespace', () => {
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      ctx.isCreatingNamespace = true;
      expect(ctx.canPublish).toBe(false);
    });

    it('returns false when not authenticated', () => {
      mockGetAuthState.mockReturnValue({ isAuthenticated: false, publishedSiteLimit: 0 });
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      expect(ctx.canPublish).toBe(false);
    });

    it('returns false when no workspace', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      ctx.namespaceId = 'ns-1';
      ctx.audienceStates = { public: { state: 'public' } };
      expect(ctx.canPublish).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // firstPublishedAudience
  // -----------------------------------------------------------------------

  describe('firstPublishedAudience', () => {
    it('returns first non-unpublished audience', () => {
      const ctx = createCtx();
      ctx.audienceStates = {
        alpha: { state: 'unpublished' },
        beta: { state: 'public' },
        gamma: { state: 'private' },
      };
      expect(ctx.firstPublishedAudience).toBe('beta');
    });

    it('returns undefined when all unpublished', () => {
      const ctx = createCtx();
      ctx.audienceStates = { alpha: { state: 'unpublished' } };
      expect(ctx.firstPublishedAudience).toBeUndefined();
    });

    it('returns undefined when empty', () => {
      const ctx = createCtx();
      expect(ctx.firstPublishedAudience).toBeUndefined();
    });
  });

  // -----------------------------------------------------------------------
  // init
  // -----------------------------------------------------------------------

  describe('init', () => {
    it('sets api and hostAction callback', () => {
      const ctx = createCtx();
      const api = createMockApi();
      const cb = vi.fn();
      ctx.init(api, cb);
      expect(ctx.api).toStrictEqual(api);
      expect(ctx.hostAction).toBe(cb);
    });

    it('works without hostAction callback', () => {
      const ctx = createCtx();
      const api = createMockApi();
      ctx.init(api);
      expect(ctx.api).toStrictEqual(api);
      expect(ctx.hostAction).toBeUndefined();
    });
  });

  // -----------------------------------------------------------------------
  // tryLoad
  // -----------------------------------------------------------------------

  describe('tryLoad', () => {
    it('does nothing when rootPath is null', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      const api = createMockApi();
      ctx.init(api);
      ctx.tryLoad();
      expect(api.executePluginCommand).not.toHaveBeenCalled();
    });

    it('does nothing when api is null', () => {
      const ctx = createCtx();
      ctx.tryLoad();
      // no error thrown
    });

    it('loads config on first call', () => {
      const ctx = createCtx();
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({
          namespace_id: 'ns-1',
          subdomain: 'test',
          audience_states: {},
        }),
      });
      ctx.init(api);

      mockProxyFetch.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({}),
      });

      ctx.tryLoad();

      // Should have called executePluginCommand (via loadPublishConfig)
      expect(api.executePluginCommand).toHaveBeenCalled();
      expect(mockConfigStore.load).toHaveBeenCalledWith('/workspace/root');
    });

    it('skips if same rootPath already initialized', () => {
      const ctx = createCtx();
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({}),
      });
      ctx.init(api);

      mockProxyFetch.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({}),
      });

      ctx.tryLoad();
      api.executePluginCommand.mockClear();
      mockConfigStore.load.mockClear();

      ctx.tryLoad();
      expect(api.executePluginCommand).not.toHaveBeenCalled();
      expect(mockConfigStore.load).not.toHaveBeenCalled();
    });
  });

  // -----------------------------------------------------------------------
  // loadAudiences
  // -----------------------------------------------------------------------

  describe('loadAudiences', () => {
    it('fetches and sets availableAudiences', async () => {
      const ctx = createCtx();
      const api = createMockApi();
      ctx.init(api);

      ctx.loadAudiences();
      await vi.waitFor(() => {
        expect(ctx.availableAudiences).toEqual(['public', 'members']);
      });
      expect(mockColorStore.assignColor).toHaveBeenCalledWith('public');
      expect(mockColorStore.assignColor).toHaveBeenCalledWith('members');
    });

    it('sets empty array on error', async () => {
      const ctx = createCtx();
      const api = createMockApi({
        getAvailableAudiences: vi.fn().mockRejectedValue(new Error('fail')),
      });
      ctx.init(api);
      ctx.availableAudiences = ['stale'];

      ctx.loadAudiences();
      await vi.waitFor(() => {
        expect(ctx.availableAudiences).toEqual([]);
      });
    });

    it('does nothing without api', () => {
      const ctx = createCtx();
      ctx.loadAudiences();
      // no error
    });

    it('does nothing without rootPath', () => {
      mockWorkspaceStore.workspaceStore.tree = null as any;
      const ctx = createCtx();
      ctx.init(createMockApi());
      ctx.loadAudiences();
    });
  });

  // -----------------------------------------------------------------------
  // executePublishCommand
  // -----------------------------------------------------------------------

  describe('executePublishCommand', () => {
    it('throws when api is null', async () => {
      const ctx = createCtx();
      await expect(ctx.executePublishCommand('Test')).rejects.toThrow('Publish API unavailable');
    });

    it('uses browser plugin when available', async () => {
      const ctx = createCtx();
      ctx.init(createMockApi());

      mockGetPlugin.mockReturnValue({ id: 'diaryx.publish' });
      mockDispatchCommand.mockResolvedValue({ success: true, data: { key: 'val' } });

      const result = await ctx.executePublishCommand('TestCmd', { a: 1 });
      expect(mockDispatchCommand).toHaveBeenCalledWith('diaryx.publish', 'TestCmd', { a: 1 });
      expect(result).toEqual({ key: 'val' });
    });

    it('throws on browser plugin failure', async () => {
      const ctx = createCtx();
      ctx.init(createMockApi());

      mockGetPlugin.mockReturnValue({ id: 'diaryx.publish' });
      mockDispatchCommand.mockResolvedValue({ success: false, error: 'bad command' });

      await expect(ctx.executePublishCommand('Fail')).rejects.toThrow('bad command');
    });

    it('falls back to api.executePluginCommand when no browser plugin', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({ result: true }),
      });
      const ctx = createCtx();
      ctx.init(api);
      mockGetPlugin.mockReturnValue(null);

      const result = await ctx.executePublishCommand('SomeCmd', { x: 1 });
      expect(api.executePluginCommand).toHaveBeenCalledWith('diaryx.publish', 'SomeCmd', { x: 1 });
      expect(result).toEqual({ result: true });
    });

    it('normalizes Map values to plain objects', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue(
          new Map([['nested', new Map([['a', 1]])]]),
        ),
      });
      const ctx = createCtx();
      ctx.init(api);

      const result = await ctx.executePublishCommand('MapCmd');
      expect(result).toEqual({ nested: { a: 1 } });
    });
  });

  // -----------------------------------------------------------------------
  // loadPublishConfig
  // -----------------------------------------------------------------------

  describe('loadPublishConfig', () => {
    it('loads config and sets state', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({
          namespace_id: 'ns-42',
          subdomain: 'mysite',
          audience_states: { public: { state: 'public' } },
        }),
      });
      const ctx = createCtx();
      ctx.init(api);

      // Mock proxyFetch for verifyNamespace
      mockProxyFetch.mockResolvedValue({ status: 200 });

      await ctx.loadPublishConfig();
      expect(ctx.namespaceId).toBe('ns-42');
      expect(ctx.subdomain).toBe('mysite');
      expect(ctx.audienceStates).toEqual({ public: { state: 'public' } });
      expect(ctx.isLoading).toBe(false);
      expect(ctx.error).toBeNull();
    });

    it('sets error on failure', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockRejectedValue(new Error('load fail')),
      });
      const ctx = createCtx();
      ctx.init(api);

      await ctx.loadPublishConfig();
      expect(ctx.error).toBe('load fail');
      expect(ctx.namespaceId).toBeNull();
      expect(ctx.subdomain).toBeNull();
      expect(ctx.audienceStates).toEqual({});
      expect(ctx.isLoading).toBe(false);
    });

    it('does nothing without api', async () => {
      const ctx = createCtx();
      await ctx.loadPublishConfig();
      expect(ctx.isLoading).toBe(false);
    });

    it('clears stale namespace when server returns 404', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn()
          .mockResolvedValueOnce({
            namespace_id: 'stale-ns',
            subdomain: 'old',
            audience_states: { public: { state: 'public' } },
          })
          // SetPublishConfig call from verifyNamespace cleanup
          .mockResolvedValueOnce(undefined),
      });
      const ctx = createCtx();
      ctx.init(api);

      mockProxyFetch.mockResolvedValue({ status: 404 });

      await ctx.loadPublishConfig();
      expect(ctx.namespaceId).toBeNull();
      expect(ctx.subdomain).toBeNull();
    });

    it('handles null fields gracefully', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({}),
      });
      const ctx = createCtx();
      ctx.init(api);

      await ctx.loadPublishConfig();
      expect(ctx.namespaceId).toBeNull();
      expect(ctx.subdomain).toBeNull();
      expect(ctx.audienceStates).toEqual({});
    });
  });

  // -----------------------------------------------------------------------
  // handleAudienceStateChange
  // -----------------------------------------------------------------------

  describe('handleAudienceStateChange', () => {
    it('adds audience config when state is not unpublished', async () => {
      const api = createMockApi({ executePluginCommand: vi.fn().mockResolvedValue(undefined) });
      const ctx = createCtx();
      ctx.init(api);

      await ctx.handleAudienceStateChange('members', { state: 'private' });
      expect(ctx.audienceStates).toEqual({ members: { state: 'private' } });
    });

    it('removes audience config when state is unpublished', async () => {
      const api = createMockApi({ executePluginCommand: vi.fn().mockResolvedValue(undefined) });
      const ctx = createCtx();
      ctx.init(api);
      ctx.audienceStates = { public: { state: 'public' }, members: { state: 'private' } };

      await ctx.handleAudienceStateChange('members', { state: 'unpublished' });
      expect(ctx.audienceStates).toEqual({ public: { state: 'public' } });
    });

    it('executes SetAudiencePublishState command', async () => {
      const api = createMockApi({ executePluginCommand: vi.fn().mockResolvedValue(undefined) });
      const ctx = createCtx();
      ctx.init(api);

      await ctx.handleAudienceStateChange('vip', {
        state: 'private',
        access_method: 'token',
      });

      expect(api.executePluginCommand).toHaveBeenCalledWith(
        'diaryx.publish',
        'SetAudiencePublishState',
        {
          audience: 'vip',
          server_url: 'https://server.example.com',
          config: {
            state: 'private',
            access_method: 'token',
          },
        },
      );
    });

    it('still updates local state if command fails', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockRejectedValue(new Error('fail')),
      });
      const ctx = createCtx();
      ctx.init(api);

      await ctx.handleAudienceStateChange('pub', { state: 'public' });
      expect(ctx.audienceStates).toEqual({ pub: { state: 'public' } });
    });
  });

  // -----------------------------------------------------------------------
  // (handleSendEmail removed — server-side email broadcasts are gone; the
  // writer composes audience emails in their own mail client via the
  // mailto-based share-action UI in NamespaceAudienceManager.)
  // -----------------------------------------------------------------------
  // handleSubdomainChange
  // -----------------------------------------------------------------------

  describe('handleSubdomainChange', () => {
    it('updates subdomain', () => {
      const ctx = createCtx();
      ctx.handleSubdomainChange('my-site');
      expect(ctx.subdomain).toBe('my-site');
    });

    it('accepts null', () => {
      const ctx = createCtx();
      ctx.subdomain = 'old';
      ctx.handleSubdomainChange(null);
      expect(ctx.subdomain).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // handlePublish
  // -----------------------------------------------------------------------

  describe('handlePublish', () => {
    it('creates namespace when not configured then publishes', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn()
          // SetPublishConfig
          .mockResolvedValueOnce(undefined)
          // PublishToNamespace
          .mockResolvedValueOnce({
            audiences_published: ['public'],
            files_uploaded: 5,
            files_deleted: 0,
          }),
      });
      const ctx = createCtx();
      ctx.init(api);
      ctx.audienceStates = { public: { state: 'public' } };

      await ctx.handlePublish();
      expect(mockCreateNamespace).toHaveBeenCalled();
      expect(ctx.namespaceId).toBe('ns-123');
      expect(mockShowSuccess).toHaveBeenCalledWith('Published 1 audience(s)');
      expect(ctx.isPublishing).toBe(false);
      expect(ctx.isCreatingNamespace).toBe(false);
    });

    it('skips namespace creation when already configured', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({
          audiences_published: ['public', 'members'],
          files_uploaded: 10,
          files_deleted: 2,
        }),
      });
      const ctx = createCtx();
      ctx.init(api);
      ctx.namespaceId = 'existing-ns';
      ctx.audienceStates = { public: { state: 'public' } };

      await ctx.handlePublish();
      expect(mockCreateNamespace).not.toHaveBeenCalled();
      expect(mockShowSuccess).toHaveBeenCalledWith('Published 2 audience(s)');
    });

    it('shows error when namespace creation fails', async () => {
      mockCreateNamespace.mockRejectedValueOnce(new Error('create fail'));
      const ctx = createCtx();
      ctx.init(createMockApi());

      await ctx.handlePublish();
      expect(mockShowError).toHaveBeenCalledWith('create fail', 'Publishing');
      expect(ctx.isCreatingNamespace).toBe(false);
      expect(ctx.isPublishing).toBe(false);
    });

    it('shows error when publish fails', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockRejectedValue(new Error('publish fail')),
      });
      const ctx = createCtx();
      ctx.init(api);
      ctx.namespaceId = 'ns-1';

      await ctx.handlePublish();
      expect(mockShowError).toHaveBeenCalledWith('publish fail', 'Publishing');
      expect(ctx.isPublishing).toBe(false);
    });

    it('resets isPublishing on success', async () => {
      const api = createMockApi({
        executePluginCommand: vi.fn().mockResolvedValue({
          audiences_published: [],
          files_uploaded: 0,
          files_deleted: 0,
        }),
      });
      const ctx = createCtx();
      ctx.init(api);
      ctx.namespaceId = 'ns-1';

      await ctx.handlePublish();
      expect(ctx.isPublishing).toBe(false);
    });
  });

  // -----------------------------------------------------------------------
  // handleSetDefaultAudience
  // -----------------------------------------------------------------------

  describe('handleSetDefaultAudience', () => {
    it('sets default audience and resets input', async () => {
      const ctx = createCtx();
      ctx.defaultAudienceInput = '  friends  ';
      ctx.showDefaultAudienceInput = true;

      await ctx.handleSetDefaultAudience();
      expect(mockConfigStore.setField).toHaveBeenCalledWith('default_audience', 'friends');
      expect(mockColorStore.assignColor).toHaveBeenCalledWith('friends');
      expect(ctx.showDefaultAudienceInput).toBe(false);
      expect(ctx.defaultAudienceInput).toBe('');
      expect(mockTemplateContextStore.bumpAudiencesVersion).toHaveBeenCalled();
    });

    it('does nothing when input is blank', async () => {
      const ctx = createCtx();
      ctx.defaultAudienceInput = '   ';
      await ctx.handleSetDefaultAudience();
      expect(mockConfigStore.setField).not.toHaveBeenCalled();
    });
  });

  // -----------------------------------------------------------------------
  // handleOpenSyncSetup
  // -----------------------------------------------------------------------

  describe('handleOpenSyncSetup', () => {
    it('invokes onHostAction when signInAction is set', () => {
      const cb = vi.fn();
      const ctx = createCtx();
      ctx.init(null, cb);
      ctx.signInAction = {
        action_type: 'open_sync',
        payload: { url: 'https://example.com' },
      } as any;

      ctx.handleOpenSyncSetup();
      expect(cb).toHaveBeenCalledWith({
        type: 'open_sync',
        payload: { url: 'https://example.com' },
      });
    });

    it('invokes onHostAction with undefined payload when payload is null', () => {
      const cb = vi.fn();
      const ctx = createCtx();
      ctx.init(null, cb);
      ctx.signInAction = { action_type: 'open_sync', payload: null } as any;

      ctx.handleOpenSyncSetup();
      expect(cb).toHaveBeenCalledWith({
        type: 'open_sync',
        payload: undefined,
      });
    });

    it('shows info toast when no signInAction', () => {
      const ctx = createCtx();
      ctx.handleOpenSyncSetup();
      expect(mockShowInfo).toHaveBeenCalledWith('Open account settings to configure publishing.');
    });

    it('shows info toast when no onHostAction callback', () => {
      const ctx = createCtx();
      ctx.signInAction = { action_type: 'open_sync' } as any;
      // no callback set
      ctx.handleOpenSyncSetup();
      expect(mockShowInfo).toHaveBeenCalled();
    });
  });

  // -----------------------------------------------------------------------
  // createNamespaceContext / getNamespaceContext
  // -----------------------------------------------------------------------

  describe('createNamespaceContext', () => {
    it('creates context and calls setContext', () => {
      const ctx = createNamespaceContext();
      expect(ctx).toBeInstanceOf(NamespaceContext);
      expect(mockSetContext).toHaveBeenCalledWith(expect.any(Symbol), ctx);
    });
  });

  describe('getNamespaceContext', () => {
    it('calls getContext with the context key', () => {
      const mockCtx = new NamespaceContext();
      mockGetContext.mockReturnValue(mockCtx);
      const ctx = getNamespaceContext();
      expect(mockGetContext).toHaveBeenCalledWith(expect.any(Symbol));
      expect(ctx).toBe(mockCtx);
    });
  });
});
