import { describe, expect, it, vi } from 'vitest';

import {
  getWorkspaceDirectoryPath,
  isMarkdownPath,
  isMarkdownTreePath,
  isRootIndexPath,
  normalizeSlashes,
  normalizeTreePath,
  normalizeWorkspacePathCandidate,
  normalizeWorkspaceRootPath,
  parentDirectory,
  resolveWorkspaceValidationRootPath,
  toWorkspaceRelativePath,
} from './path';

describe('utils/path', () => {
  describe('normalizeSlashes', () => {
    it('replaces backslashes with forward slashes', () => {
      expect(normalizeSlashes('a\\b\\c')).toBe('a/b/c');
    });

    it('leaves forward-slash paths unchanged', () => {
      expect(normalizeSlashes('a/b/c')).toBe('a/b/c');
    });
  });

  describe('normalizeTreePath', () => {
    it('strips leading ./', () => {
      expect(normalizeTreePath('./foo/bar.md')).toBe('foo/bar.md');
    });

    it('preserves leading / (for absolute Tauri paths)', () => {
      expect(normalizeTreePath('/foo/bar.md')).toBe('/foo/bar.md');
    });

    it('converts backslashes', () => {
      expect(normalizeTreePath('foo\\bar.md')).toBe('foo/bar.md');
    });
  });

  describe('isMarkdownPath', () => {
    it('accepts .md suffix', () => {
      expect(isMarkdownPath('foo.md')).toBe(true);
      expect(isMarkdownPath('a/b/c.MD')).toBe(true);
    });

    it('rejects non-md suffix', () => {
      expect(isMarkdownPath('foo.txt')).toBe(false);
      expect(isMarkdownPath('foo')).toBe(false);
    });
  });

  describe('isMarkdownTreePath', () => {
    it('accepts .md and .markdown', () => {
      expect(isMarkdownTreePath('a/b.md')).toBe(true);
      expect(isMarkdownTreePath('a/b.markdown')).toBe(true);
    });

    it('rejects null/undefined/empty', () => {
      expect(isMarkdownTreePath(null)).toBe(false);
      expect(isMarkdownTreePath(undefined)).toBe(false);
      expect(isMarkdownTreePath('')).toBe(false);
    });
  });

  describe('isRootIndexPath', () => {
    it('matches README.md and index.md at path ends', () => {
      expect(isRootIndexPath('foo/README.md')).toBe(true);
      expect(isRootIndexPath('foo/index.md')).toBe(true);
      expect(isRootIndexPath('README.md')).toBe(true);
      expect(isRootIndexPath('index.md')).toBe(true);
    });

    it('rejects other markdown files', () => {
      expect(isRootIndexPath('foo/other.md')).toBe(false);
      expect(isRootIndexPath('READMEx.md')).toBe(false);
    });
  });

  describe('parentDirectory', () => {
    it('returns the parent', () => {
      expect(parentDirectory('a/b/c')).toBe('a/b');
    });

    it('returns null for root-level', () => {
      expect(parentDirectory('foo')).toBeNull();
    });

    it('returns null for leading-slash single segment', () => {
      expect(parentDirectory('/foo')).toBeNull();
    });
  });

  describe('getWorkspaceDirectoryPath', () => {
    it('returns directories unchanged', () => {
      expect(getWorkspaceDirectoryPath('/workspace')).toBe('/workspace');
    });

    it('returns parent directory for root markdown files', () => {
      expect(getWorkspaceDirectoryPath('/workspace/Diaryx.md')).toBe('/workspace');
      expect(getWorkspaceDirectoryPath('/workspace/README.md')).toBe('/workspace');
      expect(getWorkspaceDirectoryPath('/workspace/index.md')).toBe('/workspace');
    });

    it('returns current directory for bare markdown filenames', () => {
      expect(getWorkspaceDirectoryPath('Diaryx.md')).toBe('.');
    });

    it('preserves trailing-slash-stripped directory', () => {
      expect(getWorkspaceDirectoryPath('/workspace/')).toBe('/workspace');
    });
  });

  describe('normalizeWorkspaceRootPath', () => {
    it('strips README.md / index.md suffixes', () => {
      expect(normalizeWorkspaceRootPath('/workspace/README.md')).toBe('/workspace');
      expect(normalizeWorkspaceRootPath('/workspace/index.md')).toBe('/workspace');
    });

    it('leaves other markdown files alone', () => {
      expect(normalizeWorkspaceRootPath('/workspace/Diaryx.md')).toBe('/workspace/Diaryx.md');
    });

    it('returns null for null/empty', () => {
      expect(normalizeWorkspaceRootPath(null)).toBeNull();
      expect(normalizeWorkspaceRootPath('')).toBeNull();
      expect(normalizeWorkspaceRootPath(undefined)).toBeNull();
    });

    it('trims trailing slashes', () => {
      expect(normalizeWorkspaceRootPath('/workspace/')).toBe('/workspace');
    });
  });

  describe('normalizeWorkspacePathCandidate', () => {
    it('trims trailing slashes and normalizes backslashes', () => {
      expect(normalizeWorkspacePathCandidate('a\\b\\')).toBe('a/b');
    });

    it('returns null for null/empty', () => {
      expect(normalizeWorkspacePathCandidate(null)).toBeNull();
      expect(normalizeWorkspacePathCandidate('')).toBeNull();
    });
  });

  describe('toWorkspaceRelativePath', () => {
    it('strips workspace directory prefix', () => {
      expect(toWorkspaceRelativePath('/ws/README.md', '/ws/notes/day.md')).toBe(
        'notes/day.md',
      );
    });

    it('leaves unrelated paths alone (minus leading slash)', () => {
      expect(toWorkspaceRelativePath('/ws/README.md', '/other/day.md')).toBe(
        'other/day.md',
      );
    });

    it('handles workspace paths given as a directory', () => {
      expect(toWorkspaceRelativePath('/ws', '/ws/notes/day.md')).toBe('notes/day.md');
    });
  });

  describe('resolveWorkspaceValidationRootPath', () => {
    it('prefers the loaded tree path', async () => {
      const api = { resolveWorkspaceRootIndexPath: vi.fn() };

      await expect(
        resolveWorkspaceValidationRootPath(
          api as any,
          { path: '/workspace/Diaryx.md' } as any,
          '/workspace',
        ),
      ).resolves.toBe('/workspace/Diaryx.md');
      expect(api.resolveWorkspaceRootIndexPath).not.toHaveBeenCalled();
    });

    it('falls back to backend root-index resolution', async () => {
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue('/workspace/Diaryx.md'),
      };

      await expect(
        resolveWorkspaceValidationRootPath(api as any, null, '/workspace/Diaryx.md'),
      ).resolves.toBe('/workspace/Diaryx.md');
      expect(api.resolveWorkspaceRootIndexPath).toHaveBeenCalledWith(
        '/workspace/Diaryx.md',
      );
    });

    it('falls back to the backend path if resolveWorkspaceRootIndexPath returns null', async () => {
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue(null),
      };

      await expect(
        resolveWorkspaceValidationRootPath(api as any, null, '/workspace/Diaryx.md'),
      ).resolves.toBe('/workspace/Diaryx.md');
    });
  });
});
