import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { refreshTree } from './workspaceController';

const setTree = vi.fn();
let currentTree: any = null;

vi.mock('../models/stores', () => ({
  workspaceStore: {
    setTree: (tree: any) => {
      currentTree = tree;
      setTree(tree);
    },
    get tree() {
      return currentTree;
    },
  },
}));

describe('workspaceController.refreshTree', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    currentTree = null;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('retries transient root-index failures and then loads workspace tree', async () => {
    vi.useFakeTimers();
    const workspaceTree = { path: 'README.md', name: 'My Journal', children: [] };

    const api = {
      findRootIndex: vi
        .fn()
        .mockRejectedValueOnce(new Error('Workspace not found at \'.\''))
        .mockResolvedValue('README.md'),
      getWorkspaceTree: vi.fn().mockResolvedValue(workspaceTree),
      getFilesystemTree: vi.fn(),
    };
    const backend = { getWorkspacePath: vi.fn().mockReturnValue('.') };

    const promise = refreshTree(api as any, backend as any, false, false);
    await vi.runAllTimersAsync();
    await promise;

    expect(api.findRootIndex).toHaveBeenCalledTimes(2);
    expect(api.getWorkspaceTree).toHaveBeenCalledWith('README.md', 2);
    expect(setTree).toHaveBeenCalledWith(workspaceTree);
  });

  it('keeps existing tree when fallback filesystem tree is transiently empty', async () => {
    currentTree = { path: 'README.md', name: 'My Journal', children: [] };

    const api = {
      findRootIndex: vi.fn().mockRejectedValue(new Error('Workspace not found at \'.\'')),
      getWorkspaceTree: vi.fn(),
      getFilesystemTree: vi.fn().mockResolvedValue({ path: '.', name: '.', children: [] }),
    };
    const backend = { getWorkspacePath: vi.fn().mockReturnValue('.') };

    await refreshTree(api as any, backend as any, false, false);

    expect(setTree).not.toHaveBeenCalled();
    expect(currentTree.path).toBe('README.md');
  });

  it('uses fallback filesystem tree when no prior tree exists', async () => {
    const fallbackTree = { path: '.', name: '.', children: [] };
    const api = {
      findRootIndex: vi.fn().mockRejectedValue(new Error('Workspace not found at \'.\'')),
      getWorkspaceTree: vi.fn(),
      getFilesystemTree: vi.fn().mockResolvedValue(fallbackTree),
    };
    const backend = { getWorkspacePath: vi.fn().mockReturnValue('.') };

    await refreshTree(api as any, backend as any, false, false);

    expect(setTree).toHaveBeenCalledWith(fallbackTree);
  });
});
