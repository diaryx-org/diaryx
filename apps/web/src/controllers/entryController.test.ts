import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks — referenced inside vi.mock factories
// ---------------------------------------------------------------------------

const {
  markClean,
  setSaving,
  setLoading,
  setCurrentEntry,
  setDisplayContent,
  setTitleError,
  setError,
  clearError,
  closeNewEntryModal,
  revokeBlobUrlsMock,
  reverseBlobUrlsToAttachmentPathsMock,
  dispatchFileOpenedEventMock,
  getBackendMock,
  clearCollaborationSession,
  setCollaborationPath,
} = vi.hoisted(() => ({
  markClean: vi.fn(),
  setSaving: vi.fn(),
  setLoading: vi.fn(),
  setCurrentEntry: vi.fn(),
  setDisplayContent: vi.fn(),
  setTitleError: vi.fn(),
  setError: vi.fn(),
  clearError: vi.fn(),
  closeNewEntryModal: vi.fn(),
  revokeBlobUrlsMock: vi.fn(),
  reverseBlobUrlsToAttachmentPathsMock: vi.fn((md: string) => md),
  dispatchFileOpenedEventMock: vi.fn(),
  getBackendMock: vi.fn(),
  clearCollaborationSession: vi.fn(),
  setCollaborationPath: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Module mocks
// ---------------------------------------------------------------------------

// Mutable flags so individual tests can flip them
let entryStoreSaving = false;
let entryStoreCurrentEntry: any = null;
let collaborationPath: string | null = null;

vi.mock('../models/stores', () => ({
  entryStore: {
    get isSaving() { return entryStoreSaving; },
    get currentEntry() { return entryStoreCurrentEntry; },
    setSaving,
    markClean,
    setLoading,
    setCurrentEntry: (entry: any) => {
      entryStoreCurrentEntry = entry;
      setCurrentEntry(entry);
    },
    setDisplayContent,
    setTitleError,
  },
  uiStore: {
    setError,
    clearError,
    closeNewEntryModal,
  },
  collaborationStore: {
    get currentCollaborationPath() { return collaborationPath; },
    clearCollaborationSession: () => {
      collaborationPath = null;
      clearCollaborationSession();
    },
    setCollaborationPath: (p: string) => {
      collaborationPath = p;
      setCollaborationPath(p);
    },
  },
}));

vi.mock('../models/services', () => ({
  reverseBlobUrlsToAttachmentPaths: reverseBlobUrlsToAttachmentPathsMock,
  transformAttachmentPaths: vi.fn(),
  revokeBlobUrls: revokeBlobUrlsMock,
}));

vi.mock('../lib/plugins/browserPluginManager.svelte', () => ({
  dispatchFileOpenedEvent: dispatchFileOpenedEventMock,
}));

vi.mock('../lib/backend', () => ({
  getBackend: getBackendMock,
}));

// Mock svelte tick
vi.mock('svelte', () => ({
  tick: vi.fn().mockResolvedValue(undefined),
}));

// ---------------------------------------------------------------------------
// Imports (after mocks)
// ---------------------------------------------------------------------------

import {
  getEditorBodyMarkdown,
  openEntry,
  saveEntry,
  createChildEntry,
  createEntry,
  deleteEntry,
  moveEntry,
  handlePropertyChange,
  removeProperty,
  addProperty,
  renameEntry,
  duplicateEntry,
  deleteEntryWithSync,
  createChildEntryWithSync,
  createEntryWithSync,
  saveEntryWithSync,
} from './entryController';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeApi(overrides: Record<string, any> = {}): any {
  return {
    getEntry: vi.fn(),
    saveEntry: vi.fn(),
    createChildEntry: vi.fn(),
    createEntry: vi.fn(),
    deleteEntry: vi.fn(),
    attachEntryToParent: vi.fn(),
    setFrontmatterProperty: vi.fn(),
    removeFrontmatterProperty: vi.fn(),
    renameEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    ...overrides,
  };
}

function makeEntry(overrides: Partial<{ path: string; title: string; content: string; frontmatter: any }> = {}): any {
  return {
    path: overrides.path ?? 'journal/entry.md',
    title: overrides.title ?? 'My Entry',
    content: overrides.content ?? '# Hello',
    frontmatter: overrides.frontmatter ?? { title: 'My Entry' },
  };
}

// ---------------------------------------------------------------------------
// Setup / Teardown
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  entryStoreSaving = false;
  entryStoreCurrentEntry = null;
  collaborationPath = null;
  reverseBlobUrlsToAttachmentPathsMock.mockImplementation((md: string) => md);
  getBackendMock.mockResolvedValue({ getWorkspacePath: () => 'workspace/README.md' });
});

afterEach(() => {
  vi.useRealTimers();
});

// ===========================================================================
// getEditorBodyMarkdown
// ===========================================================================

describe('getEditorBodyMarkdown', () => {
  it('returns reversed markdown from editor ref', () => {
    reverseBlobUrlsToAttachmentPathsMock.mockReturnValue('# normalized');
    const result = getEditorBodyMarkdown({ getMarkdown: () => '# raw' });
    expect(result).toBe('# normalized');
    expect(reverseBlobUrlsToAttachmentPathsMock).toHaveBeenCalledWith('# raw');
  });

  it('returns empty string when editorRef is null', () => {
    const result = getEditorBodyMarkdown(null);
    expect(result).toBe('');
    expect(reverseBlobUrlsToAttachmentPathsMock).toHaveBeenCalledWith('');
  });

  it('returns empty string when editorRef is undefined', () => {
    const result = getEditorBodyMarkdown(undefined);
    expect(result).toBe('');
  });

  it('returns empty string when getMarkdown is undefined', () => {
    const result = getEditorBodyMarkdown({});
    expect(result).toBe('');
  });

  it('returns empty string when getMarkdown returns undefined', () => {
    const result = getEditorBodyMarkdown({ getMarkdown: () => undefined });
    expect(result).toBe('');
    expect(reverseBlobUrlsToAttachmentPathsMock).toHaveBeenCalledWith('');
  });
});

// ===========================================================================
// openEntry
// ===========================================================================

describe('openEntry', () => {
  it('loads entry, sets store state, and dispatches file opened event', async () => {
    const entry = makeEntry();
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(entry) });
    const tree = { path: 'workspace/README.md', children: [] };

    await openEntry(api, 'journal/entry.md', tree as any, false);

    expect(setLoading).toHaveBeenCalledWith(true);
    expect(revokeBlobUrlsMock).toHaveBeenCalled();
    expect(api.getEntry).toHaveBeenCalledWith('journal/entry.md');
    expect(setCurrentEntry).toHaveBeenCalledWith(entry);
    expect(setDisplayContent).toHaveBeenCalledWith('# Hello');
    expect(markClean).toHaveBeenCalled();
    expect(clearError).toHaveBeenCalled();
    expect(setLoading).toHaveBeenCalledWith(false);
  });

  it('calls onBeforeOpen before loading', async () => {
    const order: string[] = [];
    const onBeforeOpen = vi.fn(async () => { order.push('before'); });
    const api = makeApi({
      getEntry: vi.fn().mockImplementation(async () => {
        order.push('getEntry');
        return makeEntry();
      }),
    });

    await openEntry(api, 'entry.md', null, false, { onBeforeOpen });

    expect(order).toEqual(['before', 'getEntry']);
  });

  it('aborts if isCurrentRequest returns false after onBeforeOpen', async () => {
    let calls = 0;
    const entry = makeEntry();
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(entry) });
    await openEntry(api, 'entry.md', null, false, {
      onBeforeOpen: async () => {},
      isCurrentRequest: () => {
        calls++;
        // First check (after onBeforeOpen) = true, second check (after getEntry) = false
        return calls <= 1;
      },
    });

    // getEntry is called between the two isCurrentRequest checks
    expect(api.getEntry).toHaveBeenCalledTimes(1);
    // But setCurrentEntry should NOT be called since the second check returns false
    expect(setCurrentEntry).not.toHaveBeenCalled();
  });

  it('sets error on store when getEntry fails', async () => {
    const api = makeApi({ getEntry: vi.fn().mockRejectedValue(new Error('Not found')) });

    await openEntry(api, 'missing.md', null, false);

    expect(setError).toHaveBeenCalledWith('Not found');
    expect(setLoading).toHaveBeenCalledWith(false);
  });

  it('normalizes Map frontmatter to plain object', async () => {
    const fm = new Map<string, unknown>([['title', 'Map Title'], ['tags', ['a', 'b']]]);
    const entry = makeEntry({ frontmatter: fm });
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(entry) });

    await openEntry(api, 'entry.md', null, false);

    // The entry passed to setCurrentEntry should have Object frontmatter
    const storedEntry = setCurrentEntry.mock.calls[0][0];
    expect(storedEntry.frontmatter).toEqual({ title: 'Map Title', tags: ['a', 'b'] });
    expect(storedEntry.frontmatter instanceof Map).toBe(false);
  });

  it('sets collaboration path when collaborationEnabled is true', async () => {
    const entry = makeEntry({ path: 'workspace/journal/entry.md' });
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(entry) });
    const tree = { path: 'workspace/README.md', children: [] };

    // Make entryStoreCurrentEntry match what openEntry sets
    setCurrentEntry.mockImplementation((e: any) => {
      entryStoreCurrentEntry = e;
    });

    await openEntry(api, 'workspace/journal/entry.md', tree as any, true);

    expect(setCollaborationPath).toHaveBeenCalledWith('journal/entry.md');
  });

  it('clears collaboration session when entry is null', async () => {
    entryStoreCurrentEntry = null;
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(null) });

    // When entry is null, setCurrentEntry(null) leaves entryStoreCurrentEntry as null
    setCurrentEntry.mockImplementation((e: any) => {
      entryStoreCurrentEntry = e;
    });

    await openEntry(api, 'entry.md', null, true);

    expect(clearCollaborationSession).toHaveBeenCalled();
  });

  it('sets error when getEntry returns null (entry not found)', async () => {
    const api = makeApi({ getEntry: vi.fn().mockResolvedValue(null) });

    await openEntry(api, 'entry.md', null, false);

    // null entry causes frontmatter normalization to throw, caught by error handler
    expect(setError).toHaveBeenCalled();
  });
});

// ===========================================================================
// saveEntry
// ===========================================================================

describe('saveEntry', () => {
  it('saves markdown via api and marks clean', async () => {
    const api = makeApi({ saveEntry: vi.fn().mockResolvedValue(null) });
    const entry = makeEntry();
    const editorRef = { getMarkdown: () => '# body' };

    await saveEntry(api, entry, editorRef);

    expect(setSaving).toHaveBeenCalledWith(true);
    expect(api.saveEntry).toHaveBeenCalledWith('journal/entry.md', '# body', undefined, undefined);
    expect(markClean).toHaveBeenCalled();
    expect(setSaving).toHaveBeenCalledWith(false);
  });

  it('does nothing when currentEntry is null', async () => {
    const api = makeApi();
    await saveEntry(api, null, { getMarkdown: () => 'x' });
    expect(api.saveEntry).not.toHaveBeenCalled();
  });

  it('does nothing when editorRef is null', async () => {
    const api = makeApi();
    await saveEntry(api, makeEntry(), null);
    expect(api.saveEntry).not.toHaveBeenCalled();
  });

  it('prevents concurrent saves', async () => {
    entryStoreSaving = true;
    const api = makeApi();
    await saveEntry(api, makeEntry(), { getMarkdown: () => 'x' });
    expect(api.saveEntry).not.toHaveBeenCalled();
  });

  it('sets error on failure', async () => {
    const api = makeApi({ saveEntry: vi.fn().mockRejectedValue(new Error('Disk full')) });
    await saveEntry(api, makeEntry(), { getMarkdown: () => 'x' });
    expect(setError).toHaveBeenCalledWith('Disk full');
    expect(markClean).not.toHaveBeenCalled();
  });
});

// ===========================================================================
// saveEntryWithSync
// ===========================================================================

describe('saveEntryWithSync', () => {
  it('retries transient save failures and eventually saves', async () => {
    vi.useFakeTimers();
    reverseBlobUrlsToAttachmentPathsMock.mockReturnValue('# normalized');

    const api = makeApi({
      saveEntry: vi.fn()
        .mockRejectedValueOnce(new Error('NotFoundError: temporary window'))
        .mockResolvedValue(undefined),
    });
    const entry = makeEntry({ path: 'README.md' });
    const editorRef = { getMarkdown: () => '# updated' };

    const promise = saveEntryWithSync(api, entry, editorRef);
    await vi.runAllTimersAsync();
    await promise;

    expect(api.saveEntry).toHaveBeenCalledTimes(2);
    expect(markClean).toHaveBeenCalledTimes(1);
    expect(setError).not.toHaveBeenCalled();
  });

  it('surfaces non-transient save errors without retry', async () => {
    const api = makeApi({
      saveEntry: vi.fn().mockRejectedValue(new Error('Permission denied')),
    });
    const entry = makeEntry({ path: 'README.md' });
    const editorRef = { getMarkdown: () => '# updated' };

    await saveEntryWithSync(api, entry, editorRef);

    expect(api.saveEntry).toHaveBeenCalledTimes(1);
    expect(markClean).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledTimes(1);
  });

  it('returns newPath when save causes rename', async () => {
    const api = makeApi({
      saveEntry: vi.fn().mockResolvedValue('journal/renamed.md'),
    });
    const entry = makeEntry({ path: 'journal/entry.md' });
    const editorRef = { getMarkdown: () => '# body' };

    const result = await saveEntryWithSync(api, entry, editorRef);

    expect(result).toEqual({ newPath: 'journal/renamed.md' });
  });

  it('returns newPath even when path is unchanged (title changed)', async () => {
    const api = makeApi({
      saveEntry: vi.fn().mockResolvedValue('journal/entry.md'),
    });
    const entry = makeEntry({ path: 'journal/entry.md' });
    const editorRef = { getMarkdown: () => '# body' };

    const result = await saveEntryWithSync(api, entry, editorRef);

    expect(result).toEqual({ newPath: 'journal/entry.md' });
  });

  it('does nothing when currentEntry is null', async () => {
    const api = makeApi();
    const result = await saveEntryWithSync(api, null, { getMarkdown: () => 'x' });
    expect(result).toBeUndefined();
    expect(api.saveEntry).not.toHaveBeenCalled();
  });

  it('prevents concurrent saves', async () => {
    entryStoreSaving = true;
    const api = makeApi();
    const result = await saveEntryWithSync(api, makeEntry(), { getMarkdown: () => 'x' });
    expect(result).toBeUndefined();
    expect(api.saveEntry).not.toHaveBeenCalled();
  });

  it('passes rootIndexPath and detectH1Title to save', async () => {
    const api = makeApi({ saveEntry: vi.fn().mockResolvedValue(null) });
    const entry = makeEntry();
    const editorRef = { getMarkdown: () => '# body' };

    await saveEntryWithSync(api, entry, editorRef, 'workspace/README.md', true);

    expect(api.saveEntry).toHaveBeenCalledWith(
      'journal/entry.md', '# body', 'workspace/README.md', true
    );
  });
});

// ===========================================================================
// createChildEntry
// ===========================================================================

describe('createChildEntry', () => {
  it('creates child and calls onSuccess', async () => {
    const result = { child_path: 'parent/child.md', parent_path: 'parent/index.md', parent_converted: false };
    const api = makeApi({ createChildEntry: vi.fn().mockResolvedValue(result) });
    const onSuccess = vi.fn();

    const ret = await createChildEntry(api, 'parent/index.md', onSuccess);

    expect(ret).toEqual(result);
    expect(api.createChildEntry).toHaveBeenCalledWith('parent/index.md');
    expect(onSuccess).toHaveBeenCalled();
  });

  it('returns null and sets error on failure', async () => {
    const api = makeApi({ createChildEntry: vi.fn().mockRejectedValue(new Error('fail')) });

    const ret = await createChildEntry(api, 'parent.md');

    expect(ret).toBeNull();
    expect(setError).toHaveBeenCalledWith('fail');
  });
});

// ===========================================================================
// createEntry
// ===========================================================================

describe('createEntry', () => {
  it('creates entry, calls onSuccess, and closes modal', async () => {
    const api = makeApi({ createEntry: vi.fn().mockResolvedValue('new/entry.md') });
    const onSuccess = vi.fn();

    const ret = await createEntry(api, 'new', { title: 'New Entry' }, onSuccess);

    expect(ret).toBe('new/entry.md');
    expect(api.createEntry).toHaveBeenCalledWith('new', { title: 'New Entry', rootIndexPath: undefined });
    expect(onSuccess).toHaveBeenCalled();
    expect(closeNewEntryModal).toHaveBeenCalled();
  });

  it('passes rootIndexPath option', async () => {
    const api = makeApi({ createEntry: vi.fn().mockResolvedValue('new/entry.md') });

    await createEntry(api, 'new', { title: 'New', rootIndexPath: 'README.md' });

    expect(api.createEntry).toHaveBeenCalledWith('new', { title: 'New', rootIndexPath: 'README.md' });
  });

  it('returns null and sets error on failure, still closes modal', async () => {
    const api = makeApi({ createEntry: vi.fn().mockRejectedValue(new Error('exists')) });

    const ret = await createEntry(api, 'new', { title: 'Dup' });

    expect(ret).toBeNull();
    expect(setError).toHaveBeenCalledWith('exists');
    expect(closeNewEntryModal).toHaveBeenCalled();
  });
});

// ===========================================================================
// deleteEntry
// ===========================================================================

describe('deleteEntry', () => {
  it('deletes and returns true', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });
    const onSuccess = vi.fn();

    const ret = await deleteEntry(api, 'entry.md', 'other.md', onSuccess);

    expect(ret).toBe(true);
    expect(api.deleteEntry).toHaveBeenCalledWith('entry.md');
    expect(onSuccess).toHaveBeenCalled();
  });

  it('clears current entry when deleting the open entry', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });

    await deleteEntry(api, 'entry.md', 'entry.md');

    expect(setCurrentEntry).toHaveBeenCalledWith(null);
    expect(markClean).toHaveBeenCalled();
  });

  it('does not clear current entry when deleting a different entry', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });

    await deleteEntry(api, 'other.md', 'entry.md');

    expect(setCurrentEntry).not.toHaveBeenCalled();
  });

  it('returns false and sets error on failure', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockRejectedValue(new Error('no perms')) });

    const ret = await deleteEntry(api, 'entry.md', null);

    expect(ret).toBe(false);
    expect(setError).toHaveBeenCalledWith('no perms');
  });

  it('retries onSuccess after short delay if first attempt fails', async () => {
    vi.useFakeTimers();
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });
    const onSuccess = vi.fn()
      .mockRejectedValueOnce(new Error('temp'))
      .mockResolvedValue(undefined);

    // Start deleteEntry — it will call onSuccess (rejects), schedule a setTimeout retry
    const promise = deleteEntry(api, 'entry.md', null, onSuccess);
    // Let the async work complete (onSuccess reject + setTimeout schedule)
    await promise;
    expect(onSuccess).toHaveBeenCalledTimes(1);

    // Advance past the 500ms retry delay
    await vi.advanceTimersByTimeAsync(600);
    // The retry setTimeout callback is async, flush microtasks
    await vi.runAllTimersAsync();
    expect(onSuccess).toHaveBeenCalledTimes(2);
  });
});

// ===========================================================================
// moveEntry
// ===========================================================================

describe('moveEntry', () => {
  it('attaches entry to new parent and returns true', async () => {
    const api = makeApi({ attachEntryToParent: vi.fn().mockResolvedValue(undefined) });
    const onSuccess = vi.fn();

    const ret = await moveEntry(api, 'child.md', 'newparent/', onSuccess);

    expect(ret).toBe(true);
    expect(api.attachEntryToParent).toHaveBeenCalledWith('child.md', 'newparent/');
    expect(onSuccess).toHaveBeenCalled();
  });

  it('returns false when moving to self', async () => {
    const api = makeApi();

    const ret = await moveEntry(api, 'entry.md', 'entry.md');

    expect(ret).toBe(false);
    expect(api.attachEntryToParent).not.toHaveBeenCalled();
  });

  it('returns false and sets error on failure', async () => {
    const api = makeApi({ attachEntryToParent: vi.fn().mockRejectedValue(new Error('cycle')) });

    const ret = await moveEntry(api, 'a.md', 'b.md');

    expect(ret).toBe(false);
    expect(setError).toHaveBeenCalledWith('cycle');
  });
});

// ===========================================================================
// handlePropertyChange
// ===========================================================================

describe('handlePropertyChange', () => {
  it('handles title change with rename', async () => {
    const entry = makeEntry({ path: 'old.md', frontmatter: { title: 'Old' } });
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockResolvedValue('new.md'),
    });
    const expanded = new Set(['old.md']);
    const onRefreshTree = vi.fn();

    const result = await handlePropertyChange(api, entry, 'title', 'New Title', expanded, onRefreshTree);

    expect(result).toEqual({ success: true, newPath: 'new.md' });
    expect(expanded.has('new.md')).toBe(true);
    expect(expanded.has('old.md')).toBe(false);
    expect(setTitleError).toHaveBeenCalledWith(null);
    expect(onRefreshTree).toHaveBeenCalled();
  });

  it('handles title change without rename', async () => {
    const entry = makeEntry({ path: 'entry.md', frontmatter: { title: 'Old' } });
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockResolvedValue(null),
    });

    const result = await handlePropertyChange(api, entry, 'title', 'New Title', new Set());

    expect(result).toEqual({ success: true, newPath: undefined });
    expect(setCurrentEntry).toHaveBeenCalled();
    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter.title).toBe('New Title');
  });

  it('treats unchanged title values as a no-op', async () => {
    const entry = makeEntry({ path: 'entry.md', frontmatter: { title: 'Same Title' } });
    const api = makeApi({
      setFrontmatterProperty: vi.fn(),
    });
    const onRefreshTree = vi.fn();

    const result = await handlePropertyChange(api, entry, 'title', 'Same Title', new Set(), onRefreshTree);

    expect(result).toEqual({ success: true });
    expect(api.setFrontmatterProperty).not.toHaveBeenCalled();
    expect(setTitleError).toHaveBeenCalledWith(null);
    expect(onRefreshTree).not.toHaveBeenCalled();
  });

  it('sets title error for "already exists" rename failure', async () => {
    const entry = makeEntry();
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockRejectedValue(new Error('Destination already exists')),
    });

    const result = await handlePropertyChange(api, entry, 'title', 'Dup', new Set());

    expect(result).toEqual({ success: false });
    expect(setTitleError).toHaveBeenCalledWith('A file with that name already exists. Choose a different title.');
  });

  it('sets generic title error for other rename failures', async () => {
    const entry = makeEntry();
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockRejectedValue(new Error('FS error')),
    });

    const result = await handlePropertyChange(api, entry, 'title', 'Bad', new Set());

    expect(result).toEqual({ success: false });
    expect(setTitleError).toHaveBeenCalledWith('Could not rename: FS error');
  });

  it('ignores empty title values', async () => {
    const entry = makeEntry({ frontmatter: { title: 'Existing' } });
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockResolvedValue(undefined),
    });

    // Empty string title goes through the non-title branch
    const result = await handlePropertyChange(api, entry, 'title', '   ', new Set());

    // Since value.trim() is falsy, it falls through to the else branch (non-title)
    expect(result).toEqual({ success: true });
  });

  it('handles non-title property changes', async () => {
    const entry = makeEntry({ frontmatter: { title: 'T' } });
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockResolvedValue(undefined),
    });

    const result = await handlePropertyChange(api, entry, 'tags', ['a', 'b'], new Set());

    expect(result).toEqual({ success: true });
    expect(api.setFrontmatterProperty).toHaveBeenCalledWith('journal/entry.md', 'tags', ['a', 'b'], undefined);
    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter.tags).toEqual(['a', 'b']);
  });

  it('refreshes tree for "contents" property changes', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });
    const onRefreshTree = vi.fn();

    await handlePropertyChange(api, entry, 'contents', ['a.md'], new Set(), onRefreshTree);

    expect(onRefreshTree).toHaveBeenCalled();
  });

  it('refreshes tree for "part_of" property changes', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });
    const onRefreshTree = vi.fn();

    await handlePropertyChange(api, entry, 'part_of', 'parent.md', new Set(), onRefreshTree);

    expect(onRefreshTree).toHaveBeenCalled();
  });

  it('does not refresh tree for other property changes', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });
    const onRefreshTree = vi.fn();

    await handlePropertyChange(api, entry, 'custom_field', 'value', new Set(), onRefreshTree);

    expect(onRefreshTree).not.toHaveBeenCalled();
  });

  it('normalizes Map frontmatter before updating', async () => {
    const fm = new Map([['title', 'Map Title']]);
    const entry = makeEntry({ frontmatter: fm });
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });

    await handlePropertyChange(api, entry, 'status', 'draft', new Set());

    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter).toEqual({ title: 'Map Title', status: 'draft' });
  });

  it('sets error on outer catch for non-title property failures', async () => {
    const entry = makeEntry();
    const api = makeApi({
      setFrontmatterProperty: vi.fn().mockRejectedValue(new Error('backend down')),
    });

    const result = await handlePropertyChange(api, entry, 'status', 'draft', new Set());

    expect(result).toEqual({ success: false });
    expect(setError).toHaveBeenCalledWith('backend down');
  });

  it('passes rootIndexPath option through', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(null) });

    await handlePropertyChange(api, entry, 'title', 'T', new Set(), undefined, { rootIndexPath: 'README.md' });

    expect(api.setFrontmatterProperty).toHaveBeenCalledWith('journal/entry.md', 'title', 'T', 'README.md');
  });
});

// ===========================================================================
// removeProperty
// ===========================================================================

describe('removeProperty', () => {
  it('removes property and updates local state', async () => {
    const entry = makeEntry({ frontmatter: { title: 'T', tags: ['a'] } });
    const api = makeApi({ removeFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });

    const ret = await removeProperty(api, entry, 'tags');

    expect(ret).toBe(true);
    expect(api.removeFrontmatterProperty).toHaveBeenCalledWith('journal/entry.md', 'tags');
    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter).toEqual({ title: 'T' });
    expect('tags' in updatedEntry.frontmatter).toBe(false);
  });

  it('returns false and sets error on failure', async () => {
    const entry = makeEntry();
    const api = makeApi({ removeFrontmatterProperty: vi.fn().mockRejectedValue(new Error('oops')) });

    const ret = await removeProperty(api, entry, 'title');

    expect(ret).toBe(false);
    expect(setError).toHaveBeenCalledWith('oops');
  });

  it('normalizes Map frontmatter before removing', async () => {
    const fm = new Map<string, unknown>([['title', 'T'], ['status', 'draft']]);
    const entry = makeEntry({ frontmatter: fm });
    const api = makeApi({ removeFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });

    const ret = await removeProperty(api, entry, 'status');

    expect(ret).toBe(true);
    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter).toEqual({ title: 'T' });
  });
});

// ===========================================================================
// addProperty
// ===========================================================================

describe('addProperty', () => {
  it('adds property via api and updates local state', async () => {
    const entry = makeEntry({ frontmatter: { title: 'T' } });
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });

    const ret = await addProperty(api, entry, 'status', 'published');

    expect(ret).toBe(true);
    expect(api.setFrontmatterProperty).toHaveBeenCalledWith('journal/entry.md', 'status', 'published', undefined);
    const updatedEntry = setCurrentEntry.mock.calls[0][0];
    expect(updatedEntry.frontmatter.status).toBe('published');
  });

  it('passes rootIndexPath', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockResolvedValue(undefined) });

    await addProperty(api, entry, 'key', 'val', 'README.md');

    expect(api.setFrontmatterProperty).toHaveBeenCalledWith('journal/entry.md', 'key', 'val', 'README.md');
  });

  it('returns false and sets error on failure', async () => {
    const entry = makeEntry();
    const api = makeApi({ setFrontmatterProperty: vi.fn().mockRejectedValue(new Error('nope')) });

    const ret = await addProperty(api, entry, 'k', 'v');

    expect(ret).toBe(false);
    expect(setError).toHaveBeenCalledWith('nope');
  });
});

// ===========================================================================
// renameEntry
// ===========================================================================

describe('renameEntry', () => {
  it('renames entry and sets up body sync for new path', async () => {
    const api = makeApi({ renameEntry: vi.fn().mockResolvedValue('journal/renamed.md') });
    const onSuccess = vi.fn();

    const ret = await renameEntry(api, 'journal/old.md', 'renamed.md', onSuccess);

    expect(ret).toBe('journal/renamed.md');
    expect(api.renameEntry).toHaveBeenCalledWith('journal/old.md', 'renamed.md');
    expect(dispatchFileOpenedEventMock).toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalled();
  });

  it('propagates errors from api', async () => {
    const api = makeApi({ renameEntry: vi.fn().mockRejectedValue(new Error('conflict')) });

    await expect(renameEntry(api, 'a.md', 'b.md')).rejects.toThrow('conflict');
  });
});

// ===========================================================================
// duplicateEntry
// ===========================================================================

describe('duplicateEntry', () => {
  it('duplicates entry and ensures body sync', async () => {
    const api = makeApi({ duplicateEntry: vi.fn().mockResolvedValue('journal/copy.md') });
    const onSuccess = vi.fn();

    const ret = await duplicateEntry(api, 'journal/entry.md', onSuccess);

    expect(ret).toBe('journal/copy.md');
    expect(api.duplicateEntry).toHaveBeenCalledWith('journal/entry.md');
    expect(dispatchFileOpenedEventMock).toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalled();
  });

  it('propagates errors from api', async () => {
    const api = makeApi({ duplicateEntry: vi.fn().mockRejectedValue(new Error('disk full')) });

    await expect(duplicateEntry(api, 'entry.md')).rejects.toThrow('disk full');
  });
});

// ===========================================================================
// deleteEntryWithSync
// ===========================================================================

describe('deleteEntryWithSync', () => {
  it('deletes entry and returns true', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });
    const onSuccess = vi.fn();

    const ret = await deleteEntryWithSync(api, 'entry.md', 'other.md', onSuccess);

    expect(ret).toBe(true);
    expect(api.deleteEntry).toHaveBeenCalledWith('entry.md');
    expect(onSuccess).toHaveBeenCalled();
  });

  it('clears current entry when deleting the open entry', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });

    await deleteEntryWithSync(api, 'entry.md', 'entry.md');

    expect(setCurrentEntry).toHaveBeenCalledWith(null);
    expect(markClean).toHaveBeenCalled();
  });

  it('returns false and sets error on failure', async () => {
    const api = makeApi({ deleteEntry: vi.fn().mockRejectedValue(new Error('locked')) });

    const ret = await deleteEntryWithSync(api, 'entry.md', null);

    expect(ret).toBe(false);
    expect(setError).toHaveBeenCalledWith('locked');
  });

  it('retries onSuccess on transient refresh failure', async () => {
    vi.useFakeTimers();
    const api = makeApi({ deleteEntry: vi.fn().mockResolvedValue(undefined) });
    const onSuccess = vi.fn()
      .mockRejectedValueOnce(new Error('temp'))
      .mockResolvedValue(undefined);

    const promise = deleteEntryWithSync(api, 'entry.md', null, onSuccess);
    await promise;

    expect(onSuccess).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(600);
    await vi.runAllTimersAsync();
    expect(onSuccess).toHaveBeenCalledTimes(2);
  });
});

// ===========================================================================
// createChildEntryWithSync
// ===========================================================================

describe('createChildEntryWithSync', () => {
  it('creates child, ensures body sync, and calls onSuccess with result', async () => {
    const result = { child_path: 'parent/child.md', parent_path: 'parent/index.md', parent_converted: true, original_parent_path: 'parent.md' };
    const api = makeApi({ createChildEntry: vi.fn().mockResolvedValue(result) });
    const onSuccess = vi.fn();

    const ret = await createChildEntryWithSync(api, 'parent.md', onSuccess);

    expect(ret).toEqual(result);
    expect(api.createChildEntry).toHaveBeenCalledWith('parent.md');
    expect(dispatchFileOpenedEventMock).toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalledWith(result);
  });

  it('returns null and sets error on failure', async () => {
    const api = makeApi({ createChildEntry: vi.fn().mockRejectedValue(new Error('no space')) });

    const ret = await createChildEntryWithSync(api, 'parent.md');

    expect(ret).toBeNull();
    expect(setError).toHaveBeenCalledWith('no space');
  });
});

// ===========================================================================
// createEntryWithSync
// ===========================================================================

describe('createEntryWithSync', () => {
  it('creates entry, ensures body sync, calls onSuccess, and closes modal', async () => {
    const api = makeApi({ createEntry: vi.fn().mockResolvedValue('new/entry.md') });
    const onSuccess = vi.fn();

    const ret = await createEntryWithSync(api, 'new', { title: 'New Entry' }, onSuccess);

    expect(ret).toBe('new/entry.md');
    expect(dispatchFileOpenedEventMock).toHaveBeenCalled();
    expect(onSuccess).toHaveBeenCalled();
    expect(closeNewEntryModal).toHaveBeenCalled();
  });

  it('passes template and rootIndexPath options', async () => {
    const api = makeApi({ createEntry: vi.fn().mockResolvedValue('new/entry.md') });

    await createEntryWithSync(api, 'new', { title: 'T', template: 'daily', rootIndexPath: 'README.md' });

    expect(api.createEntry).toHaveBeenCalledWith('new', { title: 'T', template: 'daily', rootIndexPath: 'README.md' });
  });

  it('returns null and sets error on failure, still closes modal', async () => {
    const api = makeApi({ createEntry: vi.fn().mockRejectedValue(new Error('exists')) });

    const ret = await createEntryWithSync(api, 'new', { title: 'Dup' });

    expect(ret).toBeNull();
    expect(setError).toHaveBeenCalledWith('exists');
    expect(closeNewEntryModal).toHaveBeenCalled();
  });
});
