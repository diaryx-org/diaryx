import { beforeEach, describe, expect, it, vi } from 'vitest';
import { toast } from 'svelte-sonner';

import { handleWordCount } from './commandPaletteController';

describe('commandPaletteController', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('handleWordCount', () => {
    it('includes page count in the toast description', () => {
      const editorRef = {
        getMarkdown: () => Array.from({ length: 450 }, () => 'word').join(' '),
      };

      handleWordCount(editorRef, { path: 'journal/entry.md' } as any);

      expect(toast.info).toHaveBeenCalledWith(
        'Word Count',
        expect.objectContaining({
          description: expect.stringContaining('Page Count: 1.5'),
          duration: 5000,
        })
      );
    });

    it('shows an error when there is no current entry', () => {
      handleWordCount({ getMarkdown: () => 'word' }, null);

      expect(toast.error).toHaveBeenCalledWith('No entry open');
    });
  });
});
