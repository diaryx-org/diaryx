import {
  FileIcon,
  FileText,
  FileSpreadsheet,
} from "@lucide/svelte";
import type { Api } from "$lib/backend/api";
import {
  getFilename,
  getAttachmentThumbnailUrl,
  getAttachmentMediaKind,
  getMimeType,
  isPreviewableAttachmentKind,
  type AttachmentMediaKind,
} from "$lib/../models/services/attachmentService";
import { enqueueIncrementalAttachmentUpload } from "@/controllers/attachmentController";

export interface AttachmentSelection {
  path: string;
  kind: AttachmentMediaKind;
  blobUrl?: string;
  /** The entry path where this attachment lives (for getting data) */
  sourceEntryPath: string;
}

export interface AttachmentGroup {
  entryPath: string;
  entryTitle: string | null;
  attachments: Array<{
    path: string;
    kind: AttachmentMediaKind;
    thumbnail?: string;
  }>;
}

export interface UseAttachmentPickerOptions {
  getEntryPath: () => string;
  getApi: () => Api | null;
  onSelect: (result: AttachmentSelection) => void;
  autoLoad?: boolean;
}

export function useAttachmentPicker(options: UseAttachmentPickerOptions) {
  const { getEntryPath, getApi, onSelect, autoLoad = false } = options;

  let groups = $state<AttachmentGroup[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let activeTab = $state<"existing" | "upload">("existing");
  let isDragging = $state(false);
  let loadGeneration = 0;

  if (autoLoad) {
    $effect(() => {
      const api = getApi();
      const entryPath = getEntryPath();
      if (api && entryPath) {
        load();
      }
    });
  }

  async function formatSourceRelativePath(
    sourceEntryPath: string,
    attachmentPath: string,
    fallbackPath?: string,
  ): Promise<string> {
    const api = getApi();
    if (!api) return attachmentPath;
    const trimmed = attachmentPath.trim();
    const candidates = [trimmed];
    if (trimmed.startsWith("[") && trimmed.includes("](") && !trimmed.endsWith(")")) {
      candidates.push(`${trimmed})`);
    }
    for (const candidate of candidates) {
      try {
        const canonical = await api.canonicalizeLink(candidate, sourceEntryPath);
        return await api.formatLink(
          canonical,
          getFilename(canonical) || "attachment",
          "plain_relative",
          sourceEntryPath,
        );
      } catch {
        // Try next candidate.
      }
    }
    return fallbackPath ?? attachmentPath;
  }

  async function load() {
    const api = getApi();
    const entryPath = getEntryPath();
    if (!api) return;
    const generation = ++loadGeneration;
    loading = true;
    error = null;

    try {
      const ancestorResult = await api.getAncestorAttachments(entryPath);
      const newGroups: AttachmentGroup[] = [];

      for (let i = 0; i < ancestorResult.entries.length; i++) {
        const entry = ancestorResult.entries[i];
        const isCurrentEntry = i === 0;

        const normalizedPaths = await Promise.all(
          entry.attachments.map((rawPath: string) =>
            formatSourceRelativePath(entry.entry_path, rawPath),
          ),
        );
        const attachments = Array.from(
          new Set(normalizedPaths),
        ).map((path) => ({
          path,
          kind: getAttachmentMediaKind(path),
          thumbnail: undefined as string | undefined,
        }));

        newGroups.push({
          entryPath: entry.entry_path,
          entryTitle: isCurrentEntry
            ? "Current Entry"
            : entry.entry_title || getFilename(entry.entry_path),
          attachments,
        });
      }

      if (generation !== loadGeneration) return;
      groups = newGroups;
      loading = false;

      if (newGroups.length === 0) {
        activeTab = "upload";
      }
    } catch (e) {
      if (generation !== loadGeneration) return;
      error = e instanceof Error ? e.message : String(e);
      loading = false;
    }
  }

  async function ensureThumbnail(
    attachment: AttachmentGroup["attachments"][number],
    sourceEntryPath: string,
  ): Promise<string | undefined> {
    const api = getApi();
    if (!api || attachment.kind !== "image") return undefined;
    if (attachment.thumbnail) return attachment.thumbnail;

    const url = await getAttachmentThumbnailUrl(
      api,
      sourceEntryPath,
      attachment.path,
    );
    if (url && attachment.thumbnail !== url) {
      attachment.thumbnail = url;
      groups = [...groups];
    }
    return url;
  }

  function lazyThumbnailTarget(
    node: HTMLElement,
    params: {
      attachment: AttachmentGroup["attachments"][number];
      sourceEntryPath: string;
    },
  ) {
    let current = params;
    let cancelled = false;
    let observer: IntersectionObserver | null = null;

    const doLoad = async () => {
      if (cancelled) return;
      await ensureThumbnail(current.attachment, current.sourceEntryPath);
    };

    const startObserving = () => {
      if (current.attachment.kind !== "image" || current.attachment.thumbnail) return;
      if (typeof IntersectionObserver === "undefined") {
        void doLoad();
        return;
      }
      observer?.disconnect();
      observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) {
            observer?.disconnect();
            void doLoad();
          }
        },
        { rootMargin: "200px" },
      );
      observer.observe(node);
    };

    startObserving();

    return {
      update(next: typeof params) {
        current = next;
        startObserving();
      },
      destroy() {
        cancelled = true;
        observer?.disconnect();
      },
    };
  }

  function getFileIcon(path: string) {
    const ext = path.split(".").pop()?.toLowerCase();
    switch (ext) {
      case "pdf":
        return FileText;
      case "csv":
      case "xlsx":
      case "xls":
        return FileSpreadsheet;
      default:
        return FileIcon;
    }
  }

  async function handleSelect(
    attachment: AttachmentGroup["attachments"][0],
    sourceEntryPath: string,
  ) {
    const api = getApi();
    let blobUrl = attachment.thumbnail;

    if (!blobUrl && attachment.kind === "image" && api) {
      blobUrl = await ensureThumbnail(attachment, sourceEntryPath);
    }

    onSelect({
      path: attachment.path,
      kind: attachment.kind,
      blobUrl,
      sourceEntryPath,
    });
  }

  async function handleUpload(file: File) {
    const api = getApi();
    const entryPath = getEntryPath();
    if (!api) return;

    try {
      loading = true;
      error = null;

      const arrayBuffer = await file.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);

      const attachmentPath = await api.uploadAttachment(
        entryPath,
        file.name,
        bytes,
      );
      const canonicalAttachmentPath = await api.canonicalizeLink(
        attachmentPath,
        entryPath,
      );
      const entryRelativePath = await formatSourceRelativePath(
        entryPath,
        canonicalAttachmentPath,
        attachmentPath,
      );
      await enqueueIncrementalAttachmentUpload(
        entryPath,
        canonicalAttachmentPath,
        file,
        bytes,
      );

      const kind = getAttachmentMediaKind(file.name, file.type);
      const blobUrl = isPreviewableAttachmentKind(kind)
        ? URL.createObjectURL(
            new Blob([bytes as unknown as BlobPart], {
              type: file.type || getMimeType(file.name),
            }),
          )
        : undefined;

      onSelect({
        path: entryRelativePath,
        kind,
        blobUrl,
        sourceEntryPath: entryPath,
      });
    } catch (e) {
      console.error("[AttachmentPicker] Upload failed:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function handleFileInputChange(e: Event) {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file) {
      handleUpload(file);
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    const file = e.dataTransfer?.files?.[0];
    if (file) {
      handleUpload(file);
    }
  }

  function reset() {
    loadGeneration += 1;
    groups = [];
    error = null;
  }

  return {
    get groups() { return groups; },
    get loading() { return loading; },
    get error() { return error; },
    get activeTab() { return activeTab; },
    set activeTab(value: "existing" | "upload") { activeTab = value; },
    get isDragging() { return isDragging; },

    load,
    reset,
    handleSelect,
    handleUpload,
    handleFileInputChange,
    handleDragOver,
    handleDragLeave,
    handleDrop,
    lazyThumbnailTarget,
    getFileIcon,
  };
}
