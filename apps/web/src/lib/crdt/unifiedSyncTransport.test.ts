import { describe, expect, it, vi } from 'vitest';
import { UnifiedSyncTransport } from './unifiedSyncTransport';

describe('UnifiedSyncTransport sync completion gating', () => {
  it('marks workspace synced immediately on statusChanged synced', () => {
    const onWorkspaceSynced = vi.fn();

    const transport = new UnifiedSyncTransport({
      serverUrl: 'ws://localhost:3030/sync2',
      workspaceId: 'workspace-id',
      backend: {} as any,
      writeToDisk: true,
      onWorkspaceSynced,
    });

    (transport as any).handleSyncEvent(
      JSON.stringify({ type: 'statusChanged', status: { state: 'synced' } })
    );

    expect(onWorkspaceSynced).toHaveBeenCalledTimes(1);
    expect(transport.isWorkspaceSynced).toBe(true);
  });

  it('forwards syncComplete without changing workspaceSynced', () => {
    const onSyncComplete = vi.fn();
    const onWorkspaceSynced = vi.fn();

    const transport = new UnifiedSyncTransport({
      serverUrl: 'ws://localhost:3030/sync2',
      workspaceId: 'workspace-id',
      backend: {} as any,
      writeToDisk: true,
      onWorkspaceSynced,
      onSyncComplete,
    });

    (transport as any).handleSyncEvent(
      JSON.stringify({ type: 'syncComplete', filesSynced: 2 })
    );

    expect(onSyncComplete).toHaveBeenCalledWith(2);
    expect(onWorkspaceSynced).not.toHaveBeenCalled();
    expect(transport.isWorkspaceSynced).toBe(false);
  });
});
