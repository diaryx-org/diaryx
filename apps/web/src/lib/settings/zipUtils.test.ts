import { describe, it, expect, vi, beforeEach } from 'vitest';
import JSZip from 'jszip';
import {
  addFilesToZip,
  importFilesFromZip,
  type TreeNode,
  type ExportFileReader,
  type ImportFileWriter,
} from './zipUtils';

// ============================================================================
// Test Helpers
// ============================================================================

/** In-memory file store used by mock reader/writer. */
let fileStore: Map<string, string | Uint8Array>;

function mockReader(): ExportFileReader {
  return {
    readText: vi.fn(async (path: string) => {
      const v = fileStore.get(path);
      if (typeof v === 'string') return v;
      throw new Error(`Not found: ${path}`);
    }),
    readBinary: vi.fn(async (path: string) => {
      const v = fileStore.get(path);
      if (v instanceof Uint8Array) return v;
      throw new Error(`Not found: ${path}`);
    }),
  };
}

function mockWriter(): ImportFileWriter & {
  written: Map<string, string | Uint8Array>;
} {
  const written = new Map<string, string | Uint8Array>();
  return {
    written,
    writeText: vi.fn(async (path: string, content: string) => {
      written.set(path, content);
    }),
    writeBinary: vi.fn(async (path: string, data: Uint8Array) => {
      written.set(path, data);
    }),
  };
}

/** Build a JSZip from a map of relativePath → content. */
async function buildZip(
  files: Record<string, string | Uint8Array>,
): Promise<JSZip> {
  const zip = new JSZip();
  for (const [name, content] of Object.entries(files)) {
    if (content instanceof Uint8Array) {
      zip.file(name, content, { binary: true });
    } else {
      zip.file(name, content);
    }
  }
  // Round-trip through binary so the JSZip instance looks like a real loaded zip
  const blob = await zip.generateAsync({ type: 'arraybuffer' });
  return JSZip.loadAsync(blob);
}

// ============================================================================
// Export (addFilesToZip)
// ============================================================================

describe('addFilesToZip (export)', () => {
  beforeEach(() => {
    fileStore = new Map();
  });

  it('exports leaf files', async () => {
    fileStore.set('ws/notes.md', '# Notes');
    fileStore.set('ws/todo.md', '# Todo');

    const tree: TreeNode = {
      path: 'ws',
      children: [
        { path: 'ws/notes.md' },
        { path: 'ws/todo.md' },
      ],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(2);
    expect(zip.file('notes.md')).not.toBeNull();
    expect(zip.file('todo.md')).not.toBeNull();
  });

  it('exports index files (nodes with children)', async () => {
    fileStore.set('ws/journal.md', '---\ntitle: Journal\ncontents: []\n---\n');
    fileStore.set('ws/journal/day1.md', '# Day 1');
    fileStore.set('ws/journal/day2.md', '# Day 2');

    // The filesystem tree represents the index file as the parent node's path
    // Children are the leaf files (index file itself is NOT in children)
    const tree: TreeNode = {
      path: 'ws',
      children: [
        {
          path: 'ws/journal.md',
          children: [
            { path: 'ws/journal/day1.md' },
            { path: 'ws/journal/day2.md' },
          ],
        },
      ],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(3);
    expect(zip.file('journal.md')).not.toBeNull();
    expect(zip.file('journal/day1.md')).not.toBeNull();
    expect(zip.file('journal/day2.md')).not.toBeNull();
  });

  it('exports binary attachments', async () => {
    const pngData = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
    fileStore.set('ws/index.md', '---\ntitle: Root\n---\n# Root');
    fileStore.set('ws/photo.png', pngData);

    const tree: TreeNode = {
      path: 'ws/index.md',
      children: [{ path: 'ws/photo.png' }],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(2);
    expect(zip.file('index.md')).not.toBeNull();
    expect(zip.file('photo.png')).not.toBeNull();
  });

  it('skips hidden files and directories', async () => {
    fileStore.set('ws/visible.md', '# Visible');
    fileStore.set('ws/.hidden.md', '# Hidden');

    const tree: TreeNode = {
      path: 'ws',
      children: [
        { path: 'ws/visible.md' },
        { path: 'ws/.hidden.md' },
      ],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(1);
    expect(zip.file('visible.md')).not.toBeNull();
    expect(zip.file('.hidden.md')).toBeNull();
  });

  it('handles deeply nested index hierarchy', async () => {
    fileStore.set('ws/root.md', '---\ntitle: Root\ncontents: []\n---\n');
    fileStore.set('ws/ch1/ch1.md', '---\ntitle: Ch1\ncontents: []\n---\n');
    fileStore.set('ws/ch1/sec1.md', '# Section 1');
    fileStore.set('ws/ch1/sec2/sec2.md', '---\ntitle: Sec2\ncontents: []\n---\n');
    fileStore.set('ws/ch1/sec2/page.md', '# Page');

    const tree: TreeNode = {
      path: 'ws/root.md',
      children: [
        {
          path: 'ws/ch1/ch1.md',
          children: [
            { path: 'ws/ch1/sec1.md' },
            {
              path: 'ws/ch1/sec2/sec2.md',
              children: [{ path: 'ws/ch1/sec2/page.md' }],
            },
          ],
        },
      ],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(5);
    expect(zip.file('root.md')).not.toBeNull();
    expect(zip.file('ch1/ch1.md')).not.toBeNull();
    expect(zip.file('ch1/sec1.md')).not.toBeNull();
    expect(zip.file('ch1/sec2/sec2.md')).not.toBeNull();
    expect(zip.file('ch1/sec2/page.md')).not.toBeNull();
  });

  it('exports mixed markdown and binary attachments in nested dirs', async () => {
    fileStore.set('ws/journal.md', '---\ntitle: Journal\ncontents: []\n---\n');
    fileStore.set('ws/journal/entry.md', '# Entry\n![photo](photo.jpg)');
    fileStore.set('ws/journal/photo.jpg', new Uint8Array([0xff, 0xd8]));

    const tree: TreeNode = {
      path: 'ws/journal.md',
      children: [
        { path: 'ws/journal/entry.md' },
        { path: 'ws/journal/photo.jpg' },
      ],
    };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(3);
    expect(zip.file('journal.md')).not.toBeNull();
    expect(zip.file('journal/entry.md')).not.toBeNull();
    expect(zip.file('journal/photo.jpg')).not.toBeNull();
  });

  it('handles empty workspace (no children)', async () => {
    const tree: TreeNode = { path: 'ws' };

    const zip = new JSZip();
    const reader = mockReader();
    const count = await addFilesToZip(zip, tree, 'ws', reader);

    expect(count).toBe(0);
  });
});

// ============================================================================
// Import (importFilesFromZip)
// ============================================================================

describe('importFilesFromZip (import)', () => {
  it('imports markdown files as text', async () => {
    const zip = await buildZip({
      'notes.md': '# Notes',
      'journal/day1.md': '# Day 1',
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(2);
    expect(result.files_skipped).toBe(0);
    expect(writer.written.get('./notes.md')).toBe('# Notes');
    expect(writer.written.get('./journal/day1.md')).toBe('# Day 1');
  });

  it('imports binary attachments', async () => {
    const pngData = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
    const zip = await buildZip({
      'index.md': '# Root',
      'photo.png': pngData,
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(2);
    expect(writer.written.get('./index.md')).toBe('# Root');
    expect(writer.written.get('./photo.png')).toBeInstanceOf(Uint8Array);
  });

  it('strips common root directory prefix', async () => {
    const zip = await buildZip({
      'My Journal/index.md': '# Root',
      'My Journal/notes.md': '# Notes',
      'My Journal/photo.png': new Uint8Array([1, 2, 3]),
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, 'workspace', writer);

    expect(result.files_imported).toBe(3);
    expect(writer.written.has('workspace/index.md')).toBe(true);
    expect(writer.written.has('workspace/notes.md')).toBe(true);
    expect(writer.written.has('workspace/photo.png')).toBe(true);
    // Should NOT have the "My Journal/" prefix
    expect(writer.written.has('workspace/My Journal/index.md')).toBe(false);
  });

  it('strips common root prefix even when __MACOSX metadata is present', async () => {
    const zip = await buildZip({
      '__MACOSX/._index.md': 'metadata',
      'Backup/index.md': '# Root',
      'Backup/notes.md': '# Notes',
      'Backup/photo.png': new Uint8Array([1, 2, 3]),
      'Backup/.DS_Store': 'junk',
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, 'workspace', writer);

    expect(result.files_imported).toBe(3);
    expect(result.files_skipped).toBe(2);
    expect(writer.written.has('workspace/index.md')).toBe(true);
    expect(writer.written.has('workspace/notes.md')).toBe(true);
    expect(writer.written.has('workspace/photo.png')).toBe(true);
    expect(writer.written.has('workspace/Backup/index.md')).toBe(false);
  });

  it('skips hidden files', async () => {
    const zip = await buildZip({
      'visible.md': '# Visible',
      '.hidden.md': '# Hidden',
      '.git/config': 'gitconfig',
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(1);
    expect(result.files_skipped).toBe(2);
    expect(writer.written.has('./visible.md')).toBe(true);
    expect(writer.written.has('./.hidden.md')).toBe(false);
  });

  it('skips system files', async () => {
    const zip = await buildZip({
      'notes.md': '# Notes',
      'Thumbs.db': 'thumbs',
      'desktop.ini': 'ini',
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(1);
    expect(result.files_skipped).toBe(2);
  });

  it('skips unsupported file types', async () => {
    const zip = await buildZip({
      'notes.md': '# Notes',
      'script.js': 'console.log("hi")',
      'styles.css': 'body {}',
      'data.csv': 'a,b,c',
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(1);
    expect(result.files_skipped).toBe(3);
  });

  it('imports all common attachment types', async () => {
    const bin = new Uint8Array([1]);
    const zip = await buildZip({
      'a.png': bin,
      'b.jpg': bin,
      'c.jpeg': bin,
      'd.gif': bin,
      'e.svg': bin,
      'f.pdf': bin,
      'g.webp': bin,
      'h.mp3': bin,
      'i.mp4': bin,
      'j.docx': bin,
      'k.xlsx': bin,
    });

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(11);
    expect(result.files_skipped).toBe(0);
  });

  it('reports progress', async () => {
    const zip = await buildZip({
      'a.md': '# A',
      'b.md': '# B',
      'c.md': '# C',
    });

    const writer = mockWriter();
    const progressCalls: [number, number][] = [];
    await importFilesFromZip(zip, '.', writer, (done, total) => {
      progressCalls.push([done, total]);
    });

    expect(progressCalls.length).toBe(3);
    // Each call should have incrementing done, same total
    expect(progressCalls[progressCalls.length - 1][0]).toBe(3);
    expect(progressCalls[0][1]).toBe(3);
  });

  it('handles empty zip', async () => {
    const zip = await buildZip({});

    const writer = mockWriter();
    const result = await importFilesFromZip(zip, '.', writer);

    expect(result.files_imported).toBe(0);
    expect(result.files_skipped).toBe(0);
    expect(result.success).toBe(true);
  });
});

// ============================================================================
// Round-trip (export → import)
// ============================================================================

describe('export → import round-trip', () => {
  beforeEach(() => {
    fileStore = new Map();
  });

  it('round-trips a workspace with index files, leaves, and attachments', async () => {
    // Set up source files
    const rootIndex = '---\ntitle: My Journal\ncontents:\n  - journal\n---\n# Welcome';
    const journalIndex = '---\ntitle: Journal\ncontents:\n  - day1\n---\n';
    const day1 = '# Day 1\n\nSome entry.\n\n![photo](photo.png)';
    const photoData = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
    const readme = '# README\n\nPlain leaf file.';

    fileStore.set('ws/index.md', rootIndex);
    fileStore.set('ws/journal/journal.md', journalIndex);
    fileStore.set('ws/journal/day1.md', day1);
    fileStore.set('ws/journal/photo.png', photoData);
    fileStore.set('ws/readme.md', readme);

    // Tree as returned by build_filesystem_tree
    const tree: TreeNode = {
      path: 'ws/index.md',
      children: [
        {
          path: 'ws/journal/journal.md',
          children: [
            { path: 'ws/journal/day1.md' },
            { path: 'ws/journal/photo.png' },
          ],
        },
        { path: 'ws/readme.md' },
      ],
    };

    // Export
    const zip = new JSZip();
    const reader = mockReader();
    const exportCount = await addFilesToZip(zip, tree, 'ws', reader);
    expect(exportCount).toBe(5);

    // Verify ZIP contents
    const zipFiles = Object.keys(zip.files).filter(n => !zip.files[n].dir);
    expect(zipFiles.sort()).toEqual([
      'index.md',
      'journal/day1.md',
      'journal/journal.md',
      'journal/photo.png',
      'readme.md',
    ]);

    // Import into a different workspace
    const roundTrippedZip = await JSZip.loadAsync(
      await zip.generateAsync({ type: 'arraybuffer' }),
    );
    const writer = mockWriter();
    const importResult = await importFilesFromZip(roundTrippedZip, 'new-ws', writer);

    expect(importResult.files_imported).toBe(5);
    expect(importResult.files_skipped).toBe(0);

    // Verify all files were written with correct content
    expect(writer.written.get('new-ws/index.md')).toBe(rootIndex);
    expect(writer.written.get('new-ws/journal/journal.md')).toBe(journalIndex);
    expect(writer.written.get('new-ws/journal/day1.md')).toBe(day1);
    expect(writer.written.get('new-ws/readme.md')).toBe(readme);

    // Binary round-trip
    const importedPhoto = writer.written.get('new-ws/journal/photo.png');
    expect(importedPhoto).toBeInstanceOf(Uint8Array);
    expect(Array.from(importedPhoto as Uint8Array)).toEqual(Array.from(photoData));
  });
});
