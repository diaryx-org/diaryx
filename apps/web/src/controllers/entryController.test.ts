import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { markClean, setSaving, setError } = vi.hoisted(() => ({
  markClean: vi.fn(),
  setSaving: vi.fn(),
  setError: vi.fn(),
}));

vi.mock('../models/stores', () => ({
  entryStore: {
    isSaving: false,
    setSaving,
    markClean,
  },
  uiStore: {
    setError,
  },
  collaborationStore: {},
}));

vi.mock('../models/services', () => ({
  reverseBlobUrlsToAttachmentPaths: (markdown: string) => markdown,
  transformAttachmentPaths: vi.fn(),
  revokeBlobUrls: vi.fn(),
}));

vi.mock('../lib/crdt/workspaceCrdtBridge', () => ({
  ensureBodySync: vi.fn(),
  closeBodySync: vi.fn(),
}));

import { saveEntryWithSync } from './entryController';

describe('entryController saveEntryWithSync', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('retries transient save failures and eventually saves', async () => {
    vi.useFakeTimers();

    const api = {
      saveEntry: vi
        .fn()
        .mockRejectedValueOnce(new Error('NotFoundError: temporary window'))
        .mockResolvedValue(undefined),
    };

    const currentEntry = { path: 'README.md' } as any;
    const editorRef = { getMarkdown: () => '# updated' };

    const promise = saveEntryWithSync(api as any, currentEntry, editorRef);
    await vi.runAllTimersAsync();
    await promise;

    expect(api.saveEntry).toHaveBeenCalledTimes(2);
    expect(markClean).toHaveBeenCalledTimes(1);
    expect(setError).not.toHaveBeenCalled();
  });

  it('surfaces non-transient save errors without retry loops', async () => {
    const api = {
      saveEntry: vi.fn().mockRejectedValue(new Error('Permission denied')),
    };
    const currentEntry = { path: 'README.md' } as any;
    const editorRef = { getMarkdown: () => '# updated' };

    await saveEntryWithSync(api as any, currentEntry, editorRef);

    expect(api.saveEntry).toHaveBeenCalledTimes(1);
    expect(markClean).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledTimes(1);
  });
});
