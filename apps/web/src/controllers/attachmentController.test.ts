import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  // attachmentService
  trackBlobUrl: vi.fn(),
  computeRelativeAttachmentPath: vi.fn(() => "./_attachments/photo.png"),
  formatMarkdownDestination: vi.fn((p: string) => p),
  getMimeType: vi.fn(() => "image/png"),
  getAttachmentMediaKind: vi.fn((_path: string) => "image" as "image" | "video" | "audio" | "file"),
  isPreviewableAttachmentKind: vi.fn(() => true),
  isHtmlFile: vi.fn(() => false),

  // attachmentSyncService
  enqueueAttachmentUpload: vi.fn(() => "queue-item-1"),
  indexAttachmentRefs: vi.fn(),
  sha256Hex: vi.fn(async (_data?: unknown) => "a".repeat(64)),
  isAttachmentSyncEnabled: vi.fn(() => false),
  onQueueItemStateChange: vi.fn(() => () => {}),

  // toastService
  showLoading: vi.fn(() => ({
    success: vi.fn(),
    error: vi.fn(),
    update: vi.fn(),
  })),

  // localWorkspaceRegistry
  getCurrentWorkspaceId: vi.fn(() => null as string | null),
  getServerWorkspaceId: vi.fn(() => null as string | null),
  isWorkspaceSyncEnabled: vi.fn(() => false),

  // entryStore
  setCurrentEntry: vi.fn(),
}));

// ---------------------------------------------------------------------------
// vi.mock calls
// ---------------------------------------------------------------------------

vi.mock("../models/services/attachmentService", () => ({
  trackBlobUrl: mocks.trackBlobUrl,
  computeRelativeAttachmentPath: mocks.computeRelativeAttachmentPath,
  formatMarkdownDestination: mocks.formatMarkdownDestination,
  getMimeType: mocks.getMimeType,
  getAttachmentMediaKind: mocks.getAttachmentMediaKind,
  isPreviewableAttachmentKind: mocks.isPreviewableAttachmentKind,
  isHtmlFile: mocks.isHtmlFile,
}));

vi.mock("$lib/sync/attachmentSyncService", () => ({
  enqueueAttachmentUpload: mocks.enqueueAttachmentUpload,
  indexAttachmentRefs: mocks.indexAttachmentRefs,
  sha256Hex: mocks.sha256Hex,
  isAttachmentSyncEnabled: mocks.isAttachmentSyncEnabled,
  onQueueItemStateChange: mocks.onQueueItemStateChange,
}));

vi.mock("../models/services/toastService", () => ({
  showLoading: mocks.showLoading,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  getCurrentWorkspaceId: mocks.getCurrentWorkspaceId,
  getServerWorkspaceId: mocks.getServerWorkspaceId,
  isWorkspaceSyncEnabled: mocks.isWorkspaceSyncEnabled,
}));

vi.mock("../models/stores", () => ({
  entryStore: {
    setCurrentEntry: mocks.setCurrentEntry,
  },
}));

// svelte-sonner is mocked globally in setup.ts
import { toast } from "svelte-sonner";

// ---------------------------------------------------------------------------
// Import module under test AFTER mocks
// ---------------------------------------------------------------------------

import {
  handleAddAttachment,
  handleAttachmentFileSelect,
  handleEditorFileDrop,
  handleDeleteAttachment,
  handleAttachmentInsert,
  handleMoveAttachment,
  enqueueIncrementalAttachmentUpload,
  getPendingAttachmentPath,
  setPendingAttachmentPath,
  getAttachmentError,
  setAttachmentError,
  clearAttachmentError,
} from "./attachmentController";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createMockApi(overrides: Record<string, any> = {}): any {
  return {
    uploadAttachment: vi.fn(async () => "_attachments/photo.png"),
    canonicalizeLink: vi.fn(
      async () => "entries/journal/_attachments/photo.png"
    ),
    formatLink: vi.fn(async () => "./_attachments/photo.png"),
    getEntry: vi.fn(async () => ({
      path: "entries/journal/2024-01-01.md",
      title: "Jan 1",
      content: "# Hello",
      frontmatter: {},
    })),
    deleteAttachment: vi.fn(async () => undefined),
    moveAttachment: vi.fn(async () => "new-path"),
    ...overrides,
  };
}

function createMockEntry(path = "entries/journal/2024-01-01.md"): any {
  return {
    path,
    title: "Jan 1",
    content: "# Hello",
    frontmatter: {},
  };
}

function createMockFile(
  name = "photo.png",
  size = 1024,
  type = "image/png"
): File {
  const buffer = new ArrayBuffer(size);
  return new File([buffer], name, { type });
}

function createMockEditorRef() {
  return {
    insertImage: vi.fn(),
    getMarkdown: vi.fn(() => "# content"),
    setContent: vi.fn(),
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("attachmentController", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset module-level state
    setPendingAttachmentPath("");
    setAttachmentError(null);
  });

  // ========================================================================
  // State getters / setters
  // ========================================================================

  describe("state management", () => {
    it("getPendingAttachmentPath returns empty string by default", () => {
      expect(getPendingAttachmentPath()).toBe("");
    });

    it("setPendingAttachmentPath updates the value", () => {
      setPendingAttachmentPath("some/path.md");
      expect(getPendingAttachmentPath()).toBe("some/path.md");
    });

    it("getAttachmentError returns null by default", () => {
      expect(getAttachmentError()).toBeNull();
    });

    it("setAttachmentError stores the error", () => {
      setAttachmentError("boom");
      expect(getAttachmentError()).toBe("boom");
    });

    it("clearAttachmentError resets error to null", () => {
      setAttachmentError("boom");
      clearAttachmentError();
      expect(getAttachmentError()).toBeNull();
    });
  });

  // ========================================================================
  // handleAddAttachment
  // ========================================================================

  describe("handleAddAttachment", () => {
    it("sets pending path and triggers file input click", () => {
      const input = { click: vi.fn() } as unknown as HTMLInputElement;
      handleAddAttachment("entries/journal/2024-01-01.md", input);
      expect(getPendingAttachmentPath()).toBe("entries/journal/2024-01-01.md");
      expect(input.click).toHaveBeenCalled();
    });

    it("clears previous attachment error", () => {
      setAttachmentError("old error");
      const input = { click: vi.fn() } as unknown as HTMLInputElement;
      handleAddAttachment("path.md", input);
      expect(getAttachmentError()).toBeNull();
    });

    it("handles null fileInput gracefully", () => {
      expect(() => handleAddAttachment("path.md", null)).not.toThrow();
      expect(getPendingAttachmentPath()).toBe("path.md");
    });
  });

  // ========================================================================
  // handleAttachmentFileSelect
  // ========================================================================

  describe("handleAttachmentFileSelect", () => {
    it("returns early when no file is selected", async () => {
      const api = createMockApi();
      const event = {
        target: { files: [], value: "" },
      } as unknown as Event;
      setPendingAttachmentPath("entry.md");
      await handleAttachmentFileSelect(event, api, null, null);
      expect(api.uploadAttachment).not.toHaveBeenCalled();
    });

    it("returns early when pendingAttachmentPath is empty", async () => {
      const file = createMockFile();
      const api = createMockApi();
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      // pendingAttachmentPath is '' by default from beforeEach
      await handleAttachmentFileSelect(event, api, null, null);
      expect(api.uploadAttachment).not.toHaveBeenCalled();
    });

    it("sets error for files exceeding 10MB", async () => {
      const bigFile = createMockFile("huge.png", 11 * 1024 * 1024);
      const api = createMockApi();
      const event = {
        target: { files: [bigFile], value: "huge.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entry.md");

      await handleAttachmentFileSelect(event, api, null, null);

      expect(getAttachmentError()).toMatch(/File too large/);
      expect(getAttachmentError()).toMatch(/10MB/);
      expect(api.uploadAttachment).not.toHaveBeenCalled();
    });

    it("uploads file and refreshes entry when the same entry is open", async () => {
      const file = createMockFile("photo.png", 512);
      const api = createMockApi();
      const currentEntry = createMockEntry("entries/journal/2024-01-01.md");
      const editorRef = createMockEditorRef();
      const onEntryUpdate = vi.fn();
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entries/journal/2024-01-01.md");

      await handleAttachmentFileSelect(
        event,
        api,
        currentEntry,
        editorRef,
        onEntryUpdate
      );

      expect(api.uploadAttachment).toHaveBeenCalledWith(
        "entries/journal/2024-01-01.md",
        "photo.png",
        expect.any(Uint8Array)
      );
      expect(api.canonicalizeLink).toHaveBeenCalled();
      expect(api.getEntry).toHaveBeenCalledWith("entries/journal/2024-01-01.md");
      expect(mocks.setCurrentEntry).toHaveBeenCalled();
      expect(onEntryUpdate).toHaveBeenCalled();
      expect(getAttachmentError()).toBeNull();
      expect(getPendingAttachmentPath()).toBe("");
    });

    it("inserts previewable image into editor via blob url", async () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(true);
      mocks.getAttachmentMediaKind.mockReturnValue("image");

      const file = createMockFile("photo.png", 256);
      const api = createMockApi();
      const currentEntry = createMockEntry("entries/journal/2024-01-01.md");
      const editorRef = createMockEditorRef();
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entries/journal/2024-01-01.md");

      await handleAttachmentFileSelect(event, api, currentEntry, editorRef);

      expect(mocks.trackBlobUrl).toHaveBeenCalled();
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "blob:mock-url",
        "photo.png"
      );
    });

    it("does not insert into editor for non-previewable files", async () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.getAttachmentMediaKind.mockReturnValue("file");

      const file = createMockFile("data.csv", 256, "text/csv");
      const api = createMockApi();
      const currentEntry = createMockEntry("entries/journal/2024-01-01.md");
      const editorRef = createMockEditorRef();
      const event = {
        target: { files: [file], value: "data.csv" },
      } as unknown as Event;
      setPendingAttachmentPath("entries/journal/2024-01-01.md");

      await handleAttachmentFileSelect(event, api, currentEntry, editorRef);

      expect(editorRef.insertImage).not.toHaveBeenCalled();
    });

    it("does not refresh entry when a different entry is open", async () => {
      const file = createMockFile("photo.png", 256);
      const api = createMockApi();
      const currentEntry = createMockEntry("entries/other/entry.md");
      const editorRef = createMockEditorRef();
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entries/journal/2024-01-01.md");

      await handleAttachmentFileSelect(event, api, currentEntry, editorRef);

      expect(api.getEntry).not.toHaveBeenCalled();
      expect(editorRef.insertImage).not.toHaveBeenCalled();
    });

    it("sets attachment error on upload failure", async () => {
      const file = createMockFile("photo.png", 256);
      const api = createMockApi({
        uploadAttachment: vi
          .fn()
          .mockRejectedValue(new Error("Network error")),
      });
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entry.md");

      await handleAttachmentFileSelect(event, api, null, null);

      expect(getAttachmentError()).toBe("Network error");
    });

    it("handles non-Error thrown values", async () => {
      const file = createMockFile("photo.png", 256);
      const api = createMockApi({
        uploadAttachment: vi.fn().mockRejectedValue("string error"),
      });
      const event = {
        target: { files: [file], value: "photo.png" },
      } as unknown as Event;
      setPendingAttachmentPath("entry.md");

      await handleAttachmentFileSelect(event, api, null, null);

      expect(getAttachmentError()).toBe("string error");
    });

    it("resets input value after processing", async () => {
      const file = createMockFile("photo.png", 256);
      const api = createMockApi();
      const target = { files: [file], value: "photo.png" };
      const event = { target } as unknown as Event;
      setPendingAttachmentPath("entry.md");

      await handleAttachmentFileSelect(event, api, null, null);

      expect(target.value).toBe("");
    });
  });

  // ========================================================================
  // handleEditorFileDrop
  // ========================================================================

  describe("handleEditorFileDrop", () => {
    it("returns null when no current entry", async () => {
      const result = await handleEditorFileDrop(
        createMockFile(),
        createMockApi(),
        null
      );
      expect(result).toBeNull();
    });

    it("returns null and sets error for oversized file", async () => {
      const bigFile = createMockFile("huge.png", 11 * 1024 * 1024);
      const result = await handleEditorFileDrop(
        bigFile,
        createMockApi(),
        createMockEntry()
      );
      expect(result).toBeNull();
      expect(getAttachmentError()).toMatch(/File too large/);
    });

    it("uploads and returns blob URL for previewable files", async () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(true);
      mocks.getAttachmentMediaKind.mockReturnValue("image");

      const file = createMockFile("photo.png", 512);
      const api = createMockApi();
      const entry = createMockEntry();
      const onEntryUpdate = vi.fn();

      const result = await handleEditorFileDrop(file, api, entry, onEntryUpdate);

      expect(result).not.toBeNull();
      expect(result!.blobUrl).toBe("blob:mock-url");
      expect(result!.kind).toBe("image");
      expect(api.uploadAttachment).toHaveBeenCalled();
      expect(api.getEntry).toHaveBeenCalled();
      expect(mocks.setCurrentEntry).toHaveBeenCalled();
      expect(onEntryUpdate).toHaveBeenCalled();
      expect(mocks.trackBlobUrl).toHaveBeenCalled();
    });

    it("returns empty blobUrl for non-previewable files", async () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.getAttachmentMediaKind.mockReturnValue("file");

      const file = createMockFile("data.csv", 512, "text/csv");
      const api = createMockApi();
      const entry = createMockEntry();

      const result = await handleEditorFileDrop(file, api, entry);

      expect(result).not.toBeNull();
      expect(result!.blobUrl).toBe("");
      expect(result!.kind).toBe("file");
    });

    it("returns null on API error", async () => {
      const file = createMockFile("photo.png", 256);
      const api = createMockApi({
        uploadAttachment: vi
          .fn()
          .mockRejectedValue(new Error("Upload failed")),
      });

      const result = await handleEditorFileDrop(
        file,
        api,
        createMockEntry()
      );

      expect(result).toBeNull();
      expect(getAttachmentError()).toBe("Upload failed");
    });

    it("calls onEntryUpdate when provided", async () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.getAttachmentMediaKind.mockReturnValue("file");

      const onEntryUpdate = vi.fn();
      await handleEditorFileDrop(
        createMockFile(),
        createMockApi(),
        createMockEntry(),
        onEntryUpdate
      );

      expect(onEntryUpdate).toHaveBeenCalled();
    });
  });

  // ========================================================================
  // handleDeleteAttachment
  // ========================================================================

  describe("handleDeleteAttachment", () => {
    it("returns early when no current entry", async () => {
      const api = createMockApi();
      await handleDeleteAttachment("_attachments/photo.png", api, null);
      expect(api.deleteAttachment).not.toHaveBeenCalled();
    });

    it("deletes and refreshes entry", async () => {
      const api = createMockApi();
      const entry = createMockEntry();
      const onEntryUpdate = vi.fn();

      await handleDeleteAttachment(
        "_attachments/photo.png",
        api,
        entry,
        onEntryUpdate
      );

      expect(api.deleteAttachment).toHaveBeenCalledWith(
        entry.path,
        "_attachments/photo.png"
      );
      expect(api.getEntry).toHaveBeenCalledWith(entry.path);
      expect(mocks.setCurrentEntry).toHaveBeenCalled();
      expect(onEntryUpdate).toHaveBeenCalled();
      expect(getAttachmentError()).toBeNull();
    });

    it("sets error on failure", async () => {
      const api = createMockApi({
        deleteAttachment: vi.fn().mockRejectedValue(new Error("Denied")),
      });
      const entry = createMockEntry();

      await handleDeleteAttachment("_attachments/photo.png", api, entry);

      expect(getAttachmentError()).toBe("Denied");
    });

    it("handles non-Error thrown values on delete failure", async () => {
      const api = createMockApi({
        deleteAttachment: vi.fn().mockRejectedValue("raw string"),
      });

      await handleDeleteAttachment(
        "_attachments/x.png",
        api,
        createMockEntry()
      );

      expect(getAttachmentError()).toBe("raw string");
    });
  });

  // ========================================================================
  // handleAttachmentInsert
  // ========================================================================

  describe("handleAttachmentInsert", () => {
    it("returns early when selection is falsy", () => {
      const editorRef = createMockEditorRef();
      handleAttachmentInsert(null as any, editorRef, createMockEntry());
      expect(editorRef.insertImage).not.toHaveBeenCalled();
    });

    it("returns early when editorRef is null", () => {
      handleAttachmentInsert(
        {
          path: "photo.png",
          kind: "image",
          blobUrl: "blob:x",
          sourceEntryPath: "e.md",
        },
        null,
        createMockEntry()
      );
      // no crash = pass
    });

    it("returns early when currentEntry is null", () => {
      const editorRef = createMockEditorRef();
      handleAttachmentInsert(
        {
          path: "photo.png",
          kind: "image",
          blobUrl: "blob:x",
          sourceEntryPath: "e.md",
        },
        editorRef,
        null
      );
      expect(editorRef.insertImage).not.toHaveBeenCalled();
    });

    it("inserts previewable image with blobUrl and tracks it", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/photo.png"
      );

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/photo.png",
          kind: "image",
          blobUrl: "blob:abc",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(mocks.trackBlobUrl).toHaveBeenCalledWith(
        "./_attachments/photo.png",
        "blob:abc"
      );
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "blob:abc",
        "photo.png"
      );
    });

    it("inserts previewable image without blobUrl using relative path", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/photo.png"
      );

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/photo.png",
          kind: "image",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(mocks.trackBlobUrl).not.toHaveBeenCalled();
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "./_attachments/photo.png",
        "photo.png"
      );
    });

    it("inserts non-previewable file as markdown embed", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/data.csv"
      );
      mocks.formatMarkdownDestination.mockReturnValue("./_attachments/data.csv");

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/data.csv",
          kind: "file",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(editorRef.setContent).toHaveBeenCalledWith(
        expect.stringContaining("![data.csv](./_attachments/data.csv)")
      );
    });

    it("inserts HTML file via insertImage instead of markdown embed", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.isHtmlFile.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/widget.html"
      );

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/widget.html",
          kind: "file",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "./_attachments/widget.html",
        "widget.html"
      );
      expect(editorRef.setContent).not.toHaveBeenCalled();
    });

    it("inserts HTML file with blobUrl and tracks it", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.isHtmlFile.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/widget.html"
      );

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/widget.html",
          kind: "file",
          blobUrl: "blob:html123",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(mocks.trackBlobUrl).toHaveBeenCalledWith(
        "./_attachments/widget.html",
        "blob:html123"
      );
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "blob:html123",
        "widget.html"
      );
    });

    it("treats uploaded HTML attachment-note refs as HTML when filename is preserved", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(false);
      mocks.isHtmlFile.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue(
        "./_attachments/Sample.html.md"
      );

      const editorRef = createMockEditorRef();
      const entry = createMockEntry();

      handleAttachmentInsert(
        {
          path: "_attachments/Sample.html.md",
          kind: "file",
          filename: "Sample.html",
          sourceEntryPath: entry.path,
        },
        editorRef,
        entry
      );

      expect(mocks.isHtmlFile).toHaveBeenCalledWith("Sample.html");
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "./_attachments/Sample.html.md",
        "Sample.html"
      );
      expect(editorRef.setContent).not.toHaveBeenCalled();
    });

    it("uses the filename from the last segment of path", () => {
      mocks.isPreviewableAttachmentKind.mockReturnValue(true);
      mocks.computeRelativeAttachmentPath.mockReturnValue("./deep/nested.png");

      const editorRef = createMockEditorRef();
      handleAttachmentInsert(
        {
          path: "deep/nested/photo.png",
          kind: "image",
          sourceEntryPath: "e.md",
        },
        editorRef,
        createMockEntry()
      );

      // The filename extracted should be "photo.png" from the path
      expect(editorRef.insertImage).toHaveBeenCalledWith(
        "./deep/nested.png",
        "photo.png"
      );
    });
  });

  // ========================================================================
  // handleMoveAttachment
  // ========================================================================

  describe("handleMoveAttachment", () => {
    it("returns early when source equals target", async () => {
      const api = createMockApi();
      await handleMoveAttachment(
        "entry.md",
        "entry.md",
        "_attachments/x.png",
        api,
        createMockEntry()
      );
      expect(api.moveAttachment).not.toHaveBeenCalled();
    });

    it("moves attachment and refreshes when current entry is source", async () => {
      const api = createMockApi();
      const currentEntry = createMockEntry("source.md");
      const onEntryUpdate = vi.fn();

      await handleMoveAttachment(
        "source.md",
        "target.md",
        "_attachments/x.png",
        api,
        currentEntry,
        onEntryUpdate
      );

      expect(api.moveAttachment).toHaveBeenCalledWith(
        "source.md",
        "target.md",
        "_attachments/x.png"
      );
      expect(api.getEntry).toHaveBeenCalledWith("source.md");
      expect(mocks.setCurrentEntry).toHaveBeenCalled();
      expect(onEntryUpdate).toHaveBeenCalled();
      expect(toast.success).toHaveBeenCalledWith(
        "Attachment moved successfully"
      );
    });

    it("refreshes when current entry is target", async () => {
      const api = createMockApi();
      const currentEntry = createMockEntry("target.md");

      await handleMoveAttachment(
        "source.md",
        "target.md",
        "_attachments/x.png",
        api,
        currentEntry
      );

      expect(api.getEntry).toHaveBeenCalledWith("target.md");
    });

    it("does not refresh when current entry is unrelated", async () => {
      const api = createMockApi();
      const currentEntry = createMockEntry("other.md");

      await handleMoveAttachment(
        "source.md",
        "target.md",
        "_attachments/x.png",
        api,
        currentEntry
      );

      expect(api.getEntry).not.toHaveBeenCalled();
      expect(toast.success).toHaveBeenCalled();
    });

    it("shows error toast on failure", async () => {
      const api = createMockApi({
        moveAttachment: vi
          .fn()
          .mockRejectedValue(new Error("Permission denied")),
      });

      await handleMoveAttachment(
        "source.md",
        "target.md",
        "_attachments/x.png",
        api,
        createMockEntry("source.md")
      );

      expect(toast.error).toHaveBeenCalledWith(
        "Failed to move attachment: Permission denied"
      );
    });

    it("handles non-Error thrown values in move failure", async () => {
      const api = createMockApi({
        moveAttachment: vi.fn().mockRejectedValue("raw string"),
      });

      await handleMoveAttachment(
        "source.md",
        "target.md",
        "_attachments/x.png",
        api,
        createMockEntry("source.md")
      );

      expect(toast.error).toHaveBeenCalledWith(
        "Failed to move attachment: raw string"
      );
    });
  });

  // ========================================================================
  // enqueueIncrementalAttachmentUpload
  // ========================================================================

  describe("enqueueIncrementalAttachmentUpload", () => {
    it("does nothing when no current workspace id", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue(null);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        createMockFile()
      );

      expect(mocks.indexAttachmentRefs).not.toHaveBeenCalled();
      expect(mocks.enqueueAttachmentUpload).not.toHaveBeenCalled();
    });

    it("does nothing when workspace sync is disabled", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(false);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        createMockFile()
      );

      expect(mocks.enqueueAttachmentUpload).not.toHaveBeenCalled();
    });

    it("does nothing when server workspace id is null", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue(null);

      const file = createMockFile();
      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        file
      );

      expect(mocks.indexAttachmentRefs).not.toHaveBeenCalled();
      expect(mocks.enqueueAttachmentUpload).not.toHaveBeenCalled();
    });

    it("indexes metadata and enqueues upload when sync is active", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue("server-ws-1");
      mocks.isAttachmentSyncEnabled.mockReturnValue(false);

      const file = createMockFile("photo.png", 1024, "image/png");

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        file
      );

      expect(mocks.sha256Hex).toHaveBeenCalled();
      expect(mocks.indexAttachmentRefs).toHaveBeenCalledWith(
        "entry.md",
        expect.arrayContaining([
          expect.objectContaining({
            path: "_attachments/photo.png",
            source: "local",
            hash: "a".repeat(64),
          }),
        ]),
        "server-ws-1"
      );
      expect(mocks.enqueueAttachmentUpload).toHaveBeenCalledWith(
        expect.objectContaining({
          workspaceId: "server-ws-1",
          entryPath: "entry.md",
          attachmentPath: "_attachments/photo.png",
          hash: "a".repeat(64),
          mimeType: "image/png",
          sizeBytes: 1024,
        })
      );
    });

    it("tracks upload toast when attachment sync is enabled", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue("server-ws-1");
      mocks.isAttachmentSyncEnabled.mockReturnValue(true);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        createMockFile()
      );

      expect(mocks.onQueueItemStateChange).toHaveBeenCalled();
      expect(mocks.showLoading).toHaveBeenCalled();
    });

    it("does not show upload toast when attachment sync is disabled", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue("server-ws-1");
      mocks.isAttachmentSyncEnabled.mockReturnValue(false);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        createMockFile()
      );

      expect(mocks.showLoading).not.toHaveBeenCalled();
    });

    it("uses provided bytes instead of reading from file", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue("server-ws-1");

      const providedBytes = new Uint8Array([1, 2, 3]);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        createMockFile(),
        providedBytes
      );

      expect(mocks.sha256Hex).toHaveBeenCalledWith(providedBytes);
    });

    it("reads bytes from file when not provided", async () => {
      mocks.getCurrentWorkspaceId.mockReturnValue("local-1");
      mocks.isWorkspaceSyncEnabled.mockReturnValue(true);
      mocks.getServerWorkspaceId.mockReturnValue("server-ws-1");

      const file = createMockFile("photo.png", 64);

      await enqueueIncrementalAttachmentUpload(
        "entry.md",
        "_attachments/photo.png",
        file
      );

      // sha256Hex should be called with a Uint8Array (from file.arrayBuffer())
      const calledBytes = mocks.sha256Hex.mock.calls[0][0];
      expect(calledBytes).toBeInstanceOf(Uint8Array);
    });
  });

  // ========================================================================
  // normalizeFrontmatter (tested indirectly)
  // ========================================================================

  describe("normalizeFrontmatter handling", () => {
    it("converts Map frontmatter to plain object", async () => {
      const fmMap = new Map<string, any>([
        ["title", "Hello"],
        ["tags", ["a", "b"]],
      ]);
      const api = createMockApi({
        getEntry: vi.fn(async () => ({
          path: "entry.md",
          title: "Hello",
          content: "",
          frontmatter: fmMap,
        })),
      });
      const entry = createMockEntry("entry.md");

      await handleDeleteAttachment("_attachments/x.png", api, entry);

      const calledWith = mocks.setCurrentEntry.mock.calls[0][0];
      expect(calledWith.frontmatter).toEqual({
        title: "Hello",
        tags: ["a", "b"],
      });
      expect(calledWith.frontmatter).not.toBeInstanceOf(Map);
    });

    it("handles null frontmatter gracefully", async () => {
      const api = createMockApi({
        getEntry: vi.fn(async () => ({
          path: "entry.md",
          title: "Hello",
          content: "",
          frontmatter: null,
        })),
      });
      const entry = createMockEntry("entry.md");

      await handleDeleteAttachment("_attachments/x.png", api, entry);

      const calledWith = mocks.setCurrentEntry.mock.calls[0][0];
      expect(calledWith.frontmatter).toEqual({});
    });

    it("passes through plain object frontmatter unchanged", async () => {
      const api = createMockApi({
        getEntry: vi.fn(async () => ({
          path: "entry.md",
          title: "Hello",
          content: "",
          frontmatter: { title: "Hello", draft: true },
        })),
      });
      const entry = createMockEntry("entry.md");

      await handleDeleteAttachment("_attachments/x.png", api, entry);

      const calledWith = mocks.setCurrentEntry.mock.calls[0][0];
      expect(calledWith.frontmatter).toEqual({ title: "Hello", draft: true });
    });
  });
});
