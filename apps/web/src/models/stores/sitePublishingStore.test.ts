import { beforeEach, describe, expect, it, vi } from 'vitest';

let serverWorkspaceId: string | null = 'remote-1';
let syncEnabled = false;

vi.mock('$lib/auth', () => ({
  createServerWorkspace: vi.fn(),
  getAuthState: vi.fn(),
}));

vi.mock('$lib/publish/workspaceSnapshot', () => ({
  createLocalWorkspaceSnapshot: vi.fn(),
}));

vi.mock('$lib/storage/localWorkspaceRegistry.svelte', () => ({
  getCurrentWorkspaceId: vi.fn(),
  getLocalWorkspace: vi.fn(),
  getServerWorkspaceId: vi.fn(() => serverWorkspaceId),
  isWorkspaceSyncEnabled: vi.fn(() => syncEnabled),
  setPluginMetadata: vi.fn((_id: string, _pluginId: string, metadata: { serverId?: string; syncEnabled?: boolean } | null) => {
    serverWorkspaceId = metadata?.serverId ?? null;
    syncEnabled = metadata?.syncEnabled !== false && !!metadata?.serverId;
  }),
}));

vi.mock('./collaborationStore.svelte', () => ({
  collaborationStore: {
    collaborationEnabled: false,
    effectiveSyncStatus: 'idle',
  },
}));

vi.mock('../services/sitePublishingService', () => ({
  getSite: vi.fn(),
  createSite: vi.fn(),
  deleteSite: vi.fn(),
  publishSite: vi.fn(),
  createToken: vi.fn(),
  listTokens: vi.fn(),
  revokeToken: vi.fn(),
  setCustomDomain: vi.fn(),
  removeCustomDomain: vi.fn(),
}));

import { createServerWorkspace, getAuthState } from '$lib/auth';
import { createLocalWorkspaceSnapshot } from '$lib/publish/workspaceSnapshot';
import {
  getCurrentWorkspaceId,
  getLocalWorkspace,
  setPluginMetadata,
} from '$lib/storage/localWorkspaceRegistry.svelte';
import { collaborationStore } from './collaborationStore.svelte';
import {
  createSite,
  createToken,
  getSite,
  listTokens,
  publishSite,
  revokeToken,
} from '../services/sitePublishingService';
import { getSitePublishingStore } from './sitePublishingStore.svelte';

describe('sitePublishingStore', () => {
  const store = getSitePublishingStore();

  beforeEach(() => {
    vi.clearAllMocks();
    store.reset();
    serverWorkspaceId = 'remote-1';
    syncEnabled = false;
    collaborationStore.collaborationEnabled = false;
    collaborationStore.effectiveSyncStatus = 'idle';

    vi.mocked(getAuthState).mockReturnValue({
      activeWorkspaceId: 'local-1',
      isAuthenticated: true,
      tier: 'plus',
    } as never);
    vi.mocked(getCurrentWorkspaceId).mockReturnValue('local-1');
    vi.mocked(getLocalWorkspace).mockImplementation((id: string) => (
      id === 'local-1'
        ? { id: 'local-1', name: 'Journal', pluginMetadata: {} }
        : null
    ) as never);
    vi.mocked(createLocalWorkspaceSnapshot).mockResolvedValue(new Blob(['snapshot']));
    vi.mocked(createServerWorkspace).mockResolvedValue({ id: 'remote-new', name: 'Journal' } as never);
    vi.mocked(listTokens).mockResolvedValue([]);
  });

  it('disables publishing actions when no local workspace is available', async () => {
    vi.mocked(getAuthState).mockReturnValue({
      activeWorkspaceId: null,
      isAuthenticated: true,
      tier: 'plus',
    } as never);
    vi.mocked(getCurrentWorkspaceId).mockReturnValue(null);

    await store.load();

    expect(store.hasDefaultWorkspace).toBe(false);
    expect(store.canPublish).toBe(false);
    expect(store.error).toBe('No local workspace is available for publishing.');
  });

  it('loads site state through the linked server workspace id', async () => {
    vi.mocked(getSite).mockResolvedValue({
      site: {
        id: 'site-1',
        workspace_id: 'remote-1',
        slug: 'my-site',
        custom_domain: null,
        enabled: true,
        auto_publish: true,
        last_published_at: 1730000000,
        created_at: 1730000000,
        updated_at: 1730000000,
      },
      audiences: [{ name: 'public', file_count: 12, built_at: 1730000000 }],
    });
    vi.mocked(listTokens).mockResolvedValue([
      {
        id: 'token-1',
        audience: 'public',
        label: null,
        expires_at: null,
        revoked: false,
        created_at: 1730000001,
      },
    ]);

    await store.load();

    expect(getSite).toHaveBeenCalledWith('remote-1');
    expect(listTokens).toHaveBeenCalledWith('remote-1');
    expect(store.site?.slug).toBe('my-site');
    expect(store.tokens).toHaveLength(1);
  });

  it('clears publishing state when the local workspace is not cloud-linked yet', async () => {
    serverWorkspaceId = null;
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    store.tokens = [{
      id: 'token-1',
      audience: 'public',
      label: null,
      expires_at: null,
      revoked: false,
      created_at: 1730000001,
    }];

    await store.load();

    expect(getSite).not.toHaveBeenCalled();
    expect(store.site).toBeNull();
    expect(store.tokens).toEqual([]);
    expect(store.error).toBeNull();
  });

  it('creates and stores a server workspace link before creating a site', async () => {
    serverWorkspaceId = null;
    vi.mocked(createSite).mockResolvedValue({
      id: 'site-1',
      workspace_id: 'remote-new',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    });

    const site = await store.create({ slug: 'my-site', enabled: true, auto_publish: true });

    expect(createServerWorkspace).toHaveBeenCalledWith('Journal');
    expect(setPluginMetadata).toHaveBeenCalledWith('local-1', 'sync', {
      serverId: 'remote-new',
      syncEnabled: false,
    });
    expect(createSite).toHaveBeenCalledWith('remote-new', expect.any(Object));
    expect(site?.workspace_id).toBe('remote-new');
  });

  it('sends a local snapshot to the publish-with-fallback endpoint when sync is not active', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    vi.mocked(publishSite).mockResolvedValue({
      slug: 'my-site',
      audiences: [{ name: 'public', file_count: 15 }],
      published_at: 1730001234,
    });

    const result = await store.publishNow();

    expect(result).toBe(true);
    expect(createLocalWorkspaceSnapshot).toHaveBeenCalledWith('local-1');
    expect(publishSite).toHaveBeenCalledWith('remote-1', {
      audience: undefined,
      snapshot: expect.any(Blob),
    });
    expect(store.lastPublishedAt).toBe(1730001234);
  });

  it('uses the sync fast path when live sync is active and up to date', async () => {
    syncEnabled = true;
    collaborationStore.collaborationEnabled = true;
    collaborationStore.effectiveSyncStatus = 'synced';
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    vi.mocked(publishSite).mockResolvedValue({
      slug: 'my-site',
      audiences: [{ name: 'public', file_count: 15 }],
      published_at: 1730001234,
    });

    const result = await store.publishNow();

    expect(result).toBe(true);
    expect(createLocalWorkspaceSnapshot).not.toHaveBeenCalled();
    expect(publishSite).toHaveBeenCalledTimes(1);
    expect(publishSite).toHaveBeenCalledWith('remote-1', { audience: undefined });
  });

  it('retries the publish-with-fallback endpoint with a snapshot when fast path needs one', async () => {
    syncEnabled = true;
    collaborationStore.collaborationEnabled = true;
    collaborationStore.effectiveSyncStatus = 'synced';
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    vi.mocked(publishSite)
      .mockRejectedValueOnce(Object.assign(new Error('The server does not have a current snapshot for this workspace yet.'), {
        status: 412,
        code: 'snapshot_required',
      }))
      .mockResolvedValueOnce({
        slug: 'my-site',
        audiences: [{ name: 'public', file_count: 15 }],
        published_at: 1730001234,
      });

    const result = await store.publishNow();

    expect(result).toBe(true);
    expect(createLocalWorkspaceSnapshot).toHaveBeenCalledWith('local-1');
    expect(publishSite).toHaveBeenCalledTimes(2);
    expect(publishSite).toHaveBeenNthCalledWith(1, 'remote-1', { audience: undefined });
    expect(publishSite).toHaveBeenNthCalledWith(2, 'remote-1', {
      audience: undefined,
      snapshot: expect.any(Blob),
    });
  });

  it('stores one-time access URL and prepends created token', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };

    vi.mocked(createToken).mockResolvedValue({
      token: {
        id: 'token-2',
        audience: 'family',
        label: 'Cousins',
        expires_at: 1730000900,
        revoked: false,
        created_at: 1730000002,
      },
      accessUrl: 'https://sites.example.com/my-site?access=abc',
    });

    await store.createToken({ audience: 'family', label: 'Cousins', expires_in: '7d' });

    expect(createToken).toHaveBeenCalledWith('remote-1', expect.any(Object));
    expect(store.tokens[0].id).toBe('token-2');
    expect(store.lastCreatedAccessUrl).toBe('https://sites.example.com/my-site?access=abc');
  });

  it('removes token from local state after revoke', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'remote-1',
      slug: 'my-site',
      custom_domain: null,
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    store.tokens = [
      {
        id: 'token-1',
        audience: 'public',
        label: null,
        expires_at: null,
        revoked: false,
        created_at: 1730000001,
      },
      {
        id: 'token-2',
        audience: 'family',
        label: 'Cousins',
        expires_at: null,
        revoked: false,
        created_at: 1730000002,
      },
    ];
    vi.mocked(revokeToken).mockResolvedValue(undefined);

    const result = await store.revokeToken('token-1');

    expect(result).toBe(true);
    expect(revokeToken).toHaveBeenCalledWith('remote-1', 'token-1');
    expect(store.tokens.map((token) => token.id)).toEqual(['token-2']);
  });
});
