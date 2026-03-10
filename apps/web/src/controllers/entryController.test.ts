import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { markClean, setSaving, setError, reverseBlobUrlsToAttachmentPaths } = vi.hoisted(() => ({
  markClean: vi.fn(),
  setSaving: vi.fn(),
  setError: vi.fn(),
  reverseBlobUrlsToAttachmentPaths: vi.fn((markdown: string) => markdown),
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
  reverseBlobUrlsToAttachmentPaths,
  transformAttachmentPaths: vi.fn(),
  revokeBlobUrls: vi.fn(),
}));

import { getEditorBodyMarkdown, saveEntryWithSync } from './entryController';

describe('entryController saveEntryWithSync', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    reverseBlobUrlsToAttachmentPaths.mockImplementation((markdown: string) => markdown);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('normalizes live editor markdown through attachment path reversal', () => {
    reverseBlobUrlsToAttachmentPaths.mockReturnValue('# normalized');

    expect(getEditorBodyMarkdown({ getMarkdown: () => '# raw' })).toBe('# normalized');
    expect(reverseBlobUrlsToAttachmentPaths).toHaveBeenCalledWith('# raw');
  });

  it('retries transient save failures and eventually saves', async () => {
    vi.useFakeTimers();
    reverseBlobUrlsToAttachmentPaths.mockReturnValue('# normalized');

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
    expect(api.saveEntry).toHaveBeenCalledWith('README.md', '# normalized', undefined, undefined);
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
