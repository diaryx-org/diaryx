<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { Markdown } from "@tiptap/markdown";
  import Link from "@tiptap/extension-link";
  import TaskList from "@tiptap/extension-task-list";
  import TaskItem from "@tiptap/extension-task-item";
  import Placeholder from "@tiptap/extension-placeholder";
  import CodeBlock from "@tiptap/extension-code-block";
  import Typography from "@tiptap/extension-typography";
  import Image from "@tiptap/extension-image";
  import {
    formatMarkdownDestination,
    getPathForBlobUrl,
    getBlobUrl,
    isVideoFile,
    isAudioFile,
    isPreviewableAttachmentKind,
    queueResolveAttachment,
    type AttachmentMediaKind,
  } from "../models/services/attachmentService";
  import { Table } from "@tiptap/extension-table";
  import { TableRow } from "@tiptap/extension-table-row";
  import { TableHeader } from "@tiptap/extension-table-header";
  import { TableCell } from "@tiptap/extension-table-cell";
  // FloatingMenu extension for block formatting
  import FloatingMenu from "@tiptap/extension-floating-menu";
  // BubbleMenu extension for inline formatting on selection
  import BubbleMenu from "@tiptap/extension-bubble-menu";
  // ProseMirror Plugin for link click handling
  import { Plugin as ProseMirrorPlugin } from "@tiptap/pm/state";

  // FloatingMenu for block formatting (headings, lists, etc.)
  import FloatingMenuComponent from "./components/FloatingMenuComponent.svelte";
  // BubbleMenu for inline formatting when text is selected
  import BubbleMenuComponent from "./components/BubbleMenuComponent.svelte";

  // Custom extension for inline attachment picker node
  import { AttachmentPickerNode } from "./extensions/AttachmentPickerNode";
  // Custom extension for inline block picker (replaces FloatingMenu expanded state)
  import { BlockPickerNode } from "./extensions/BlockPickerNode";
  // Custom extension for raw HTML blocks
  import { HtmlBlock } from "./extensions/HtmlBlock";
  // Custom extension for Notion-style inline table controls
  import { TableControls } from "./extensions/TableControls";
  // Custom extension for markdown footnotes
  import { FootnoteRef, preprocessFootnotes, appendFootnoteDefinitions } from "./extensions/FootnoteRef";
  import { SearchHighlight } from "./extensions/SearchHighlight";
  import { toast } from "svelte-sonner";
  import { getTemplateContextStore } from "./stores/templateContextStore.svelte";
  import { getEditorExtensions, getPluginExtensionsVersion } from "$lib/plugins/browserPluginManager.svelte";
  import { getPreservedEditorExtensions } from "$lib/plugins/preservedEditorExtensions.svelte";
  import { getTauriEditorExtensions } from "$lib/plugins/tauriEditorExtensions";
  import { setEditorExtensionIframeContext } from "$lib/plugins/editorExtensionFactory";
  import type { Api } from "$lib/backend/api";
  import { isTauri } from "$lib/backend/interface";
  import { isIOS } from "$lib/hooks/useMobile.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { getLinkFormatStore } from "$lib/stores/linkFormatStore.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import type { TreeNode } from "$lib/backend";

  // On iOS Tauri, a native UIToolbar replaces the web BubbleMenu
  const useNativeToolbar = isTauri() && isIOS();
  const nativeLinkFormatStore = useNativeToolbar ? getLinkFormatStore() : null;
  const pluginStore = getPluginStore();

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: () => void;
    onblur?: () => void;
    readonly?: boolean;
    onFileDrop?: (
      file: File,
    ) => Promise<{ blobUrl: string; attachmentPath: string; kind: AttachmentMediaKind } | null>;
    // Debug mode for menus (logs shouldShow decisions to console)
    debugMenus?: boolean;
    // Callback when a link is clicked (for handling relative links to other notes)
    onLinkClick?: (href: string) => void;
    // Attachment picker options
    entryPath?: string;
    api?: Api | null;
    onAttachmentInsert?: (selection: {
      path: string;
      kind: AttachmentMediaKind;
      blobUrl?: string;
      sourceEntryPath: string;
    }) => void;
  }

  let {
    content = "",
    placeholder = "Start writing...",
    onchange,
    onblur,
    readonly = false,
    onFileDrop,
    debugMenus = false,
    onLinkClick,
    entryPath = "",
    api = null,
    onAttachmentInsert,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);

  // Template context store — used by ConditionalBlock decorations to detect context changes
  const templateContextStore = getTemplateContextStore();

  // FloatingMenu element ref - must exist before editor creation
  let floatingMenuElement: HTMLDivElement | undefined = $state();
  // FloatingMenu component ref - for programmatic expansion
  let floatingMenuRef: { expand: () => void } | undefined = $state();
  // BubbleMenu element ref - must exist before editor creation
  let bubbleMenuElement: HTMLDivElement | undefined = $state();
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Track the last content prop value applied into the editor.
  // We intentionally do not update this on local typing, so parent prop updates
  // remain the only thing that can programmatically replace editor content.
  let lastAppliedContentProp: string | undefined = undefined;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
  let lastReadonly: boolean | null = null;
  let lastPlaceholder: string | null = null;
  let lastPluginKey: string | null = null;

  function getPluginExtensionKey(): string {
    if (isTauri()) {
      return JSON.stringify(
        pluginStore.allManifests.map((manifest) => ({
          id: String(manifest.id),
          version: String(manifest.version ?? ""),
          ui: manifest.ui,
        })),
      );
    }

    return `browser:${getPluginExtensionsVersion()}`;
  }

  function destroyEditor() {
    editor?.destroy();
    editor = null;
    if (typeof globalThis !== 'undefined') {
      (globalThis as any).__diaryx_tiptapEditor = null;
    }
  }

  /** Walk up from `el` to find the nearest ancestor with overflow scroll/auto. */
  function getScrollParent(el: HTMLElement | null): HTMLElement | Window {
    if (!el || el === document.documentElement) return window;
    const { overflow, overflowY } = getComputedStyle(el);
    if (/(auto|scroll)/.test(overflow + overflowY)) return el;
    return getScrollParent(el.parentElement);
  }

  function createEditor() {
    // Update global iframe context so iframe node views read the current entry
    setEditorExtensionIframeContext({ entryPath, api: api ?? null });

    const initialContent = editor ? appendFootnoteDefinitions(editor) : content;
    destroyEditor();

    // In non-readonly mode, require FloatingMenu unless native iOS toolbar is active
    if (!readonly && !useNativeToolbar && !floatingMenuElement) {
      if (debugMenus) {
        console.log(
          "[Editor] FloatingMenu element not ready, deferring editor creation",
        );
      }
      return;
    }

    // Build extensions array
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const extensions: any[] = [
      StarterKit.configure({
        codeBlock: false, // We'll use the separate extension
        link: false, // Disable Link in StarterKit; we register Link explicitly below
      }),
      Markdown.configure({
        //transformPastedText: true,
        //transformCopiedText: true,
        markedOptions: { gfm: true },
      }),
      Link.configure({
        openOnClick: false,
        HTMLAttributes: {
          class: "editor-link",
        },
      }).extend({
        // Wrap hrefs containing spaces in angle brackets per markdown spec
        renderMarkdown: (node: any, h: any) => {
          const href = node.attrs?.href ?? '';
          const title = node.attrs?.title ?? '';
          const text = h.renderChildren(node);
          const formattedHref = href.includes(' ') ? `<${href}>` : href;
          return title ? `[${text}](${formattedHref} "${title}")` : `[${text}](${formattedHref})`;
        },
        // Add click handler for links
        addProseMirrorPlugins() {
          const plugins = this.parent?.() ?? [];
          return [
            ...plugins,
            new ProseMirrorPlugin({
              props: {
                handleClick: (_view, _pos, event) => {
                  const target = event.target as HTMLElement;
                  const link = target.closest("a");
                  if (link && link.href) {
                    event.preventDefault();
                    const href = link.getAttribute("href") || "";
                    if (onLinkClick) {
                      onLinkClick(href);
                    } else if (
                      href.startsWith("http://") ||
                      href.startsWith("https://")
                    ) {
                      // External link - open in new tab
                      window.open(href, "_blank", "noopener,noreferrer");
                    }
                    return true;
                  }
                  return false;
                },
              },
            }),
          ];
        },
      }),
      TaskList,
      TaskItem.configure({
        nested: true,
      }),
      Placeholder.configure({
        placeholder,
      }),
      CodeBlock.configure({
        HTMLAttributes: {
          class: "editor-code-block",
        },
      }),
      Typography,
      Image.configure({
        inline: true,
        allowBase64: true,
        HTMLAttributes: {
          class: "editor-image",
          loading: "lazy",
        },
      }).extend({
        renderMarkdown: (node: any) => {
          const src = node.attrs?.src ?? "";
          const alt = node.attrs?.alt ?? "";
          const title = node.attrs?.title ?? "";
          const formattedSrc = formatMarkdownDestination(src);
          return title
            ? `![${alt}](${formattedSrc} "${title}")`
            : `![${alt}](${formattedSrc})`;
        },
        addNodeView() {
          // Capture entryPath and api from the outer scope (Editor props)
          const ep = entryPath;
          const epApi = api;
          return ({ node, HTMLAttributes }) => {
            const src = node.attrs.src || "";
            const alt = node.attrs.alt || "";
            const title = node.attrs.title || "";

            const isLocalPath = src && !src.startsWith('blob:') && !src.startsWith('http://') && !src.startsWith('https://') && !src.startsWith('data:');

            // For local paths, check media type from the raw path
            // For blob URLs, look up the original path
            const originalPath = isLocalPath ? src : getPathForBlobUrl(src);
            const checkPath = originalPath || src;
            const isVideo = isVideoFile(checkPath);
            const isAudio = isAudioFile(checkPath);

            // Transparent 1x1 GIF used as placeholder src for loading images.
            // Without a real src, <img> elements create "dead zones" in
            // contenteditable that capture mouse events and block text selection.
            const PLACEHOLDER_SRC = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';

            /** Mark an element as loading: add shimmer class and disable pointer events. */
            function setLoading(el: HTMLElement) {
              el.classList.add("editor-image--loading");
            }
            /** Clear loading state and restore pointer events. */
            function clearLoading(el: HTMLElement) {
              el.classList.remove("editor-image--loading");
            }

            let dom: HTMLElement;

            if (isVideo) {
              const video = document.createElement("video");
              video.controls = true;
              video.preload = "metadata";
              video.className = "editor-image editor-video";
              if (title) video.title = title;
              if (isLocalPath && epApi) {
                const cached = getBlobUrl(src);
                if (cached) {
                  video.src = cached;
                } else {
                  setLoading(video);
                  queueResolveAttachment(epApi, ep, src).then((blobUrl) => {
                    if (blobUrl) {
                      video.src = blobUrl;
                      clearLoading(video);
                    }
                  });
                }
              } else {
                video.src = src;
              }
              dom = video;
            } else if (isAudio) {
              const audio = document.createElement("audio");
              audio.controls = true;
              audio.preload = "metadata";
              audio.className = "editor-audio";
              if (title) audio.title = title;
              if (isLocalPath && epApi) {
                const cached = getBlobUrl(src);
                if (cached) {
                  audio.src = cached;
                } else {
                  setLoading(audio);
                  queueResolveAttachment(epApi, ep, src).then((blobUrl) => {
                    if (blobUrl) {
                      audio.src = blobUrl;
                      clearLoading(audio);
                    }
                  });
                }
              } else {
                audio.src = src;
              }
              dom = audio;
            } else {
              const img = document.createElement("img");
              img.alt = alt;
              img.loading = "lazy";
              img.className = HTMLAttributes.class || "editor-image";
              if (title) img.title = title;
              if (isLocalPath && epApi) {
                const cached = getBlobUrl(src);
                if (cached) {
                  img.src = cached;
                } else {
                  img.src = PLACEHOLDER_SRC;
                  setLoading(img);
                  queueResolveAttachment(epApi, ep, src).then((blobUrl) => {
                    if (blobUrl) {
                      img.src = blobUrl;
                      clearLoading(img);
                    }
                  });
                }
              } else {
                img.src = src;
              }
              dom = img;
            }

            return { dom };
          };
        },
      }),
      Table.configure({ resizable: false }).extend({
        // Custom markdown renderer/parser that fixes upstream issues:
        // 1. Empty cells emit &nbsp; (from Paragraph's renderMarkdown)
        // 2. No-header tables emit an extra empty header row on roundtrip
        // 3. Header-disabled state is lost on roundtrip (markdown always has a header row)
        //    Solved by prefixing the first cell with U+200B when headers are disabled;
        //    the parser detects this and creates tableCell instead of tableHeader.
        renderMarkdown: (node: any, h: any) => {
          if (!node?.content?.length) return '';
          const rows: { text: string; isHeader: boolean }[][] = [];
          for (const rowNode of node.content) {
            const cells: { text: string; isHeader: boolean }[] = [];
            for (const cellNode of rowNode.content ?? []) {
              const raw = cellNode.content ? h.renderChildren(cellNode.content) : '';
              const text = (raw || '').replace(/&nbsp;/g, '').replace(/\s+/g, ' ').trim();
              cells.push({ text, isHeader: cellNode.type === 'tableHeader' });
            }
            rows.push(cells);
          }
          const colCount = rows.reduce((m: number, r: any[]) => Math.max(m, r.length), 0);
          if (!colCount) return '';
          const colW = new Array(colCount).fill(3);
          for (const r of rows) for (let i = 0; i < colCount; i++) colW[i] = Math.max(colW[i], (r[i]?.text || '').length);
          const pad = (s: string, w: number) => s + ' '.repeat(Math.max(0, w - s.length));
          const hasHeader = rows[0]?.some(c => c.isHeader);
          const headerRow = rows[0] ?? [];
          const headerTexts = headerRow.map((c, i) => pad(c.text, colW[i]));
          // When headers are disabled, prefix first cell with U+200B as a marker
          if (!hasHeader && headerTexts.length > 0) {
            headerTexts[0] = '\u200B' + headerTexts[0];
            // Recalculate first column width to account for the marker
            colW[0] = Math.max(colW[0], headerTexts[0].length);
          }
          let out = '\n';
          out += `| ${headerTexts.join(' | ')} |\n`;
          out += `| ${colW.map((w: number) => '-'.repeat(w)).join(' | ')} |\n`;
          for (const r of rows.slice(1)) {
            out += `| ${new Array(colCount).fill(0).map((_: any, i: number) => pad(r[i]?.text || '', colW[i])).join(' | ')} |\n`;
          }
          return out;
        },
        parseMarkdown: (token: any, h: any) => {
          const rows = [];
          // Detect the U+200B marker in the first header cell to determine
          // whether this table had headers disabled.
          let noHeader = false;
          if (token.header?.length) {
            const firstCellRaw = token.header[0]?.text ?? '';
            if (firstCellRaw.startsWith('\u200B')) {
              noHeader = true;
              // Strip the marker from the token text so it doesn't appear in content
              token.header[0].text = firstCellRaw.slice(1);
              // Also strip from the raw tokens array if present
              if (token.header[0].tokens?.length) {
                const firstToken = token.header[0].tokens[0];
                if (firstToken.type === 'text' && firstToken.text?.startsWith('\u200B')) {
                  firstToken.text = firstToken.text.slice(1);
                  firstToken.raw = firstToken.raw?.replace('\u200B', '') ?? firstToken.raw;
                }
              }
            }
          }
          if (token.header) {
            const cellType = noHeader ? 'tableCell' : 'tableHeader';
            const headerCells = token.header.map((cell: any) =>
              h.createNode(cellType, {}, [{ type: 'paragraph', content: h.parseInline(cell.tokens) }])
            );
            rows.push(h.createNode('tableRow', {}, headerCells));
          }
          if (token.rows) {
            for (const row of token.rows) {
              const bodyCells = row.map((cell: any) =>
                h.createNode('tableCell', {}, [{ type: 'paragraph', content: h.parseInline(cell.tokens) }])
              );
              rows.push(h.createNode('tableRow', {}, bodyCells));
            }
          }
          return h.createNode('table', undefined, rows);
        },
      }),
      TableRow,
      TableHeader,
      TableCell,
      TableControls,
      // Search highlighting for find-in-file
      SearchHighlight,
      // Footnote extension
      FootnoteRef,
      // Raw HTML block extension
      HtmlBlock.configure({
        entryPath,
        api,
      }),
      // Inline attachment picker node extension
      AttachmentPickerNode.configure({
        entryPath,
        api,
        onAttachmentSelect: (selection) => {
          onAttachmentInsert?.(selection);
        },
      }),
      // Inline block picker node extension (replaces FloatingMenu expanded state)
      BlockPickerNode.configure({
        onInsertAttachment: onAttachmentInsert
          ? () => editor?.commands.insertAttachmentPicker()
          : undefined,
      }),
      // Plugin-generated editor extensions (e.g., math blocks)
      // Tauri: use native backend (plugins loaded synchronously, available immediately)
      // Web: use browser Extism plugins (loaded async, editor rebuilds when ready)
      ...(isTauri() ? getTauriEditorExtensions() : getEditorExtensions()),
      // Session-local fallback extensions keep removed plugin syntax round-trippable
      // until the next full reload clears marked's shared tokenizer registry.
      ...getPreservedEditorExtensions(),
    ];

    // Add FloatingMenu extension (for block formatting on empty lines)
    // On iOS Tauri, the native toolbar includes a block picker button instead
    if (!readonly && !useNativeToolbar) {
      extensions.push(
        FloatingMenu.configure({
          element: floatingMenuElement,
          appendTo: () => document.body,
          options: {
            strategy: "fixed",
            placement: "left-start",
            scrollTarget: getScrollParent(element),
            offset: 10,
            flip: {
              fallbackPlacements: ["right-start", "left", "right"],
            },
            shift: {
              padding: 8,
            },
            // Manually control visibility to prevent flash on initial load
            onShow: () => {
              if (floatingMenuElement) {
                floatingMenuElement.style.display = "block";
              }
            },
            onHide: () => {
              if (floatingMenuElement) {
                floatingMenuElement.style.display = "none";
              }
            },
          },
          shouldShow: ({ editor: ed, view, state }) => {
            const { selection } = state;
            const { empty } = selection;
            const anchor = selection.$anchor;

            if (debugMenus) {
              console.log("[FloatingMenu] shouldShow check", {
                empty,
                editable: ed.isEditable,
                hasFocus: view.hasFocus(),
                parentType: anchor.parent.type.name,
                contentSize: anchor.parent.content.size,
              });
            }

            // Must be editable
            if (!ed.isEditable) return false;

            // Must have focus - prevents menu from showing on initial load
            // before user has interacted with the editor
            if (!view.hasFocus()) return false;

            // Must be an empty selection (cursor, not a range)
            if (!empty) return false;

            // Only show on empty paragraph lines
            const isEmptyParagraph =
              anchor.parent.type.name === "paragraph" &&
              anchor.parent.content.size === 0;
            if (!isEmptyParagraph) return false;

            // Don't show in code blocks
            if (ed.isActive("codeBlock")) return false;

            // Don't show in tables
            if (ed.isActive("table")) return false;

            // Don't show in blockquotes
            if (ed.isActive("blockquote")) return false;

            // Don't show in lists
            if (
              ed.isActive("bulletList") ||
              ed.isActive("orderedList") ||
              ed.isActive("taskList")
            ) {
              return false;
            }

            if (debugMenus) {
              console.log("[FloatingMenu] shouldShow: true");
            }
            return true;
          },
        }),
      );

      // Add BubbleMenu extension (for inline formatting when text is selected)
      // On iOS Tauri, a native toolbar replaces this; on other mobile, uses the bottom InlineToolbar
      if (!useNativeToolbar && bubbleMenuElement) {
        extensions.push(
          BubbleMenu.configure({
            element: bubbleMenuElement,
            options: {
              offset: 10,
              // Keep BubbleMenu within viewport bounds (especially important on mobile)
              shift: {
                padding: 8,
              },
              // Manually control visibility to prevent flash on initial load
              onShow: () => {
                if (bubbleMenuElement) {
                  bubbleMenuElement.style.display = "flex";
                }
              },
              onHide: () => {
                if (bubbleMenuElement) {
                  bubbleMenuElement.style.display = "none";
                }
              },
            },
            shouldShow: ({ editor: ed, view, state, from, to }) => {
              // Must be editable
              if (!ed.isEditable) return false;

              // Must have focus
              if (!view.hasFocus()) return false;

              // Show in tables for header toggle / delete table controls
              if (ed.isActive("table")) return true;

              // Must have a selection (not just cursor)
              const { empty } = state.selection;
              if (empty) return false;

              // Check if the selection contains actual text content
              const text = state.doc.textBetween(from, to, " ");
              if (!text.trim()) return false;

              if (debugMenus) {
                console.log("[BubbleMenu] shouldShow: true");
              }
              return true;
            },
          }),
        );
      }
    }

    function createEditor(editorContent: string) {
      return new Editor({
        element,
        extensions,
        content: preprocessFootnotes(editorContent),
        contentType: "markdown",
        editable: !readonly,
        onCreate: () => {
          // Track the last external content value that has been applied so we don't
          // overwrite the editor unless the prop actually changes.
          lastAppliedContentProp = initialContent;
        },
        onUpdate: () => {
          if (onchange && !isUpdatingContent) {
            onchange();
          }
        },
        onBlur: () => {
          onblur?.();
        },
        editorProps: {
          attributes: {
            class: "editor-content",
          },
          handleKeyDown: (view, event) => {
            // Right Arrow on empty paragraph opens floating menu
            if (event.key === "ArrowRight" && floatingMenuRef) {
              const { state } = view;
              const { selection } = state;
              const { empty } = selection;
              const anchor = selection.$anchor;

              // Check if we're on an empty paragraph (same conditions as floating menu shouldShow)
              const isEmptyParagraph =
                anchor.parent.type.name === "paragraph" &&
                anchor.parent.content.size === 0;

              if (empty && isEmptyParagraph) {
                event.preventDefault();
                floatingMenuRef.expand();
                return true;
              }
            }
            return false;
          },
          handlePaste: (_view, event) => {
            const items = event.clipboardData?.items;
            if (!items) return false;

            for (const item of items) {
              // Handle pasted images
              if (item.type.startsWith('image/')) {
                const file = item.getAsFile();
                if (file && onFileDrop) {
                  event.preventDefault();
                  onFileDrop(file).then(result => {
                    if (result && result.blobUrl && editor) {
                      editor.chain().focus()
                        .setImage({ src: result.blobUrl, alt: file.name })
                        .run();
                    }
                  });
                  return true;
                }
              }
            }
            return false;
          },
        },
      });
    }

    try {
      editor = createEditor(initialContent);
    } catch (err) {
      console.error("[Editor] Failed to load content, recovering with empty document:", err);
      toast.error("Entry contains invalid content", {
        description: "The editor recovered by loading an empty document. Your data is safe — try re-opening the entry or removing incompatible plugins.",
      });
      editor = createEditor("");
    }

    if (typeof globalThis !== 'undefined') {
      (globalThis as any).__diaryx_tiptapEditor = editor;
    }

    // Expose link picker helpers for the native iOS toolbar
    if (useNativeToolbar) {
      function flattenTree(node: TreeNode | null): { path: string; name: string; displayPath: string }[] {
        if (!node) return [];
        // Use the directory containing the root node as the workspace root
        const lastSlash = node.path.lastIndexOf('/');
        const rootDir = lastSlash >= 0 ? node.path.slice(0, lastSlash + 1) : '';
        const entries: { path: string; name: string; displayPath: string }[] = [];
        function traverse(n: TreeNode) {
          const displayPath = rootDir && n.path.startsWith(rootDir)
            ? n.path.slice(rootDir.length)
            : n.path;
          entries.push({ path: n.path, name: n.name, displayPath });
          for (const child of n.children) traverse(child);
        }
        traverse(node);
        return entries;
      }

      (globalThis as any).__diaryx_nativeToolbar = {
        getEntries: () => flattenTree(workspaceStore.tree)
          .filter((e: { path: string }) => e.path !== entryPath),
        getEntryPath: () => entryPath ?? '',
        insertRemoteLink: (href: string) => {
          editor?.chain().focus().setLink({ href }).run();
        },
        insertLocalLink: async (path: string, name: string) => {
          if (api && entryPath) {
            try {
              const format = nativeLinkFormatStore?.format ?? 'markdown_relative';
              const formatted = await api.formatLink(path, name, format, entryPath);
              const mdMatch = formatted.match(/\[.*?\]\((.*?)\)/);
              let href = mdMatch ? mdMatch[1] : formatted;
              if (href.startsWith('<') && href.endsWith('>')) href = href.slice(1, -1);
              editor?.chain().focus().setLink({ href }).run();
            } catch {
              editor?.chain().focus().setLink({ href: path }).run();
            }
          } else {
            editor?.chain().focus().setLink({ href: path }).run();
          }
        },
        getPluginCommands: () => {
          const store = getPluginStore();
          const cmds = store.editorInsertCommands;
          return {
            marks: cmds.mark.map(c => ({
              extensionId: c.extensionId,
              label: c.label,
              iconName: c.iconName,
            })),
            inlineAtoms: cmds.inline.map(c => ({
              extensionId: c.extensionId,
              label: c.label,
              iconName: c.iconName,
            })),
            blockAtoms: cmds.block.map(c => ({
              extensionId: c.extensionId,
              label: c.label,
              iconName: c.iconName,
              placement: c.placement,
            })),
            blockPickerItems: store.blockPickerItems.map(item => ({
              id: item.contribution.id,
              label: item.contribution.label,
              iconName: item.contribution.icon,
              editorCommand: item.contribution.editor_command,
              params: item.contribution.params,
              prompt: item.contribution.prompt ? {
                message: item.contribution.prompt.message,
                defaultValue: item.contribution.prompt.default_value,
                paramKey: item.contribution.prompt.param_key,
              } : null,
            })),
            toolbarMarks: store.markToolbarEntries.map(entry => ({
              extensionId: entry.extensionId,
              label: entry.label,
              iconName: entry.iconName,
              attribute: entry.attribute ? {
                name: entry.attribute.name,
                defaultValue: entry.attribute.default,
                validValues: entry.attribute.validValues,
              } : null,
            })),
          };
        },
      };
    }
  }

  export function getMarkdown(): string | undefined {
    if (!editor) return undefined;
    return appendFootnoteDefinitions(editor);
  }

  /**
   * Set content from markdown
   */
  export function setContent(markdown: string): void {
    if (!editor) return;
    lastAppliedContentProp = markdown;
    try {
      editor.commands.setContent(preprocessFootnotes(markdown), { contentType: "markdown" });
    } catch (err) {
      console.error("[Editor] Failed to set content:", err);
      toast.error("Could not load entry content", {
        description: "The document may contain nodes from an incompatible plugin.",
      });
    }
  }

  /**
   * Focus the editor
   */
  export function focus(): void {
    editor?.commands.focus();
  }

  /**
   * Focus the editor at the end of the document and create a new paragraph if needed
   */
  export function focusAtEnd(): void {
    if (!editor) return;

    // Move cursor to end of document
    editor.commands.focus("end");

    // Check if we're on an empty paragraph - if not, create one
    const { selection } = editor.state;
    const currentNode = selection.$anchor.parent;
    const isEmptyParagraph = currentNode.type.name === "paragraph" && currentNode.content.size === 0;

    if (!isEmptyParagraph) {
      // Insert a new paragraph at the end
      editor.chain().focus("end").createParagraphNear().focus("end").run();
    }
  }

  /**
   * Check if editor is empty
   */
  export function isEmpty(): boolean {
    return editor?.isEmpty ?? true;
  }

  /**
   * Insert an image at cursor position
   */
  export function insertImage(src: string, alt?: string): void {
    if (!editor) return;
    editor
      .chain()
      .focus()
      .setImage({ src, alt: alt || "" })
      .run();
  }

  /**
   * Get the underlying TipTap Editor instance (for extensions like search)
   */
  export function getEditor(): Editor | null {
    return editor;
  }

  export function reorderFootnotes(): void {
    if (!editor) return;
    editor.chain().focus().reorderFootnotes().run();
  }

  onMount(() => {
    // Don't create editor here - let the $effect handle it once menu elements are ready
    // This ensures BubbleMenu and FloatingMenu extensions have elements to bind to
  });

  // Track if we've done initial editor creation
  let editorInitialized = $state(false);

  // Wait for menu elements to be available before creating editor
  // This effect runs when bubbleMenuElement or floatingMenuElement change
  $effect(() => {
    // Explicitly track the element refs so Svelte knows to re-run when they change
    const hasEditorElement = !!element;
    const hasFloatingMenu = !!floatingMenuElement;
    const hasBubbleMenu = !!bubbleMenuElement;
    const isReadonly = readonly;

    if (debugMenus) {
      console.log("[Editor] Init effect check", {
        hasEditorElement,
        hasFloatingMenu,
        hasBubbleMenu,
        isReadonly,
        editorInitialized,
      });
    }

    // In readonly mode, we don't need menu elements
    if (isReadonly) {
      if (!editorInitialized && hasEditorElement) {
        if (debugMenus) {
          console.log("[Editor] Creating editor (readonly mode)");
        }
        createEditor();
        editorInitialized = true;
        lastReadonly = readonly;
        lastPlaceholder = placeholder;
        lastPluginKey = getPluginExtensionKey();
      }
      return;
    }

    // In edit mode, wait for menu elements (native toolbar replaces BubbleMenu + FloatingMenu on iOS Tauri)
    const bubbleMenuReady = useNativeToolbar || hasBubbleMenu;
    const floatingMenuReady = useNativeToolbar || hasFloatingMenu;
    if (!editorInitialized && hasEditorElement && floatingMenuReady && bubbleMenuReady) {
      if (debugMenus) {
        console.log("[Editor] Menu elements ready, creating editor", {
          floatingMenuElement,
          bubbleMenuElement,
        });
      }
      createEditor();
      editorInitialized = true;
      lastReadonly = readonly;
      lastPlaceholder = placeholder;
      lastPluginKey = getPluginExtensionKey();
    }
  });

  // Reset editorInitialized when readonly changes so the effect can re-run
  $effect(() => {
    if (lastReadonly !== null && lastReadonly !== readonly) {
      editorInitialized = false;
    }
  });

  onDestroy(() => {
    destroyEditor();
  });

  // Rebuild editor when readonly, placeholder, or plugin extensions change
  $effect(() => {
    if (!element) return;
    // Skip if we haven't done initial creation yet
    if (!editorInitialized) return;

    const pluginKey = getPluginExtensionKey();
    const needsRebuild =
      readonly !== lastReadonly ||
      placeholder !== lastPlaceholder ||
      pluginKey !== lastPluginKey;

    if (!needsRebuild) return;

    // Update tracking for what we're about to build
    lastReadonly = readonly;
    lastPlaceholder = placeholder;
    lastPluginKey = pluginKey;

    createEditor();
  });

  // Update editor content when the content prop changes (e.g., switching files)
  // Only sync when the external content prop has actually changed from what we last applied.
  $effect(() => {
    if (!editor) return;
    if (content === undefined) return;

    const currentMarkdown = appendFootnoteDefinitions(editor);
    if (content === lastAppliedContentProp || content === currentMarkdown) {
      lastAppliedContentProp = content;
      return;
    }

    // Content prop changed independently from the live editor state,
    // so replace the document with the external source of truth.
    lastAppliedContentProp = content;
    isUpdatingContent = true;
    try {
      editor.commands.setContent(preprocessFootnotes(content), { contentType: "markdown" });
    } catch (err) {
      console.error("[Editor] Failed to sync content prop:", err);
      toast.error("Could not load entry content", {
        description: "The document may contain nodes from an incompatible plugin.",
      });
    }
    setTimeout(() => {
      isUpdatingContent = false;
    }, 0);
  });

  // Toggle overscroll padding when content exceeds 50% of viewport height
  $effect(() => {
    if (!editor) return;
    const editorEl = editor.view.dom;

    function updateOverscroll() {
      // Temporarily remove overscroll class to measure natural content height
      const hadClass = editorEl.classList.contains("overscroll");
      if (hadClass) editorEl.classList.remove("overscroll");
      const contentHeight = editorEl.scrollHeight;
      if (hadClass) editorEl.classList.add("overscroll");

      const threshold = window.innerHeight * 0.5;
      editorEl.classList.toggle("overscroll", contentHeight > threshold);
    }

    const ro = new ResizeObserver(updateOverscroll);
    ro.observe(editorEl);
    updateOverscroll();

    return () => ro.disconnect();
  });

  // Refresh conditional block decorations when template context changes
  // (e.g., frontmatter audience edited → active/inactive branch highlights update)
  $effect(() => {
    // Access reactive properties to track them as dependencies
    void templateContextStore.context;
    void templateContextStore.previewAudience;
    if (editor) {
      const tr = editor.state.tr.setMeta("templateContextChanged", true);
      editor.view.dispatch(tr);
    }
  });
</script>

<!-- Editor content area - scrolling handled by parent EditorContent -->
<div
  bind:this={element}
  class="min-h-full"
  role="application"
  ondragover={(e) => {
    e.preventDefault();
    e.dataTransfer && (e.dataTransfer.dropEffect = "copy");
  }}
  ondrop={async (e) => {
    e.preventDefault();
    const file = e.dataTransfer?.files?.[0];
    if (file && onFileDrop) {
      const result = await onFileDrop(file);
      if (result && result.blobUrl && editor && isPreviewableAttachmentKind(result.kind)) {
        editor
          .chain()
          .focus()
          .setImage({ src: result.blobUrl, alt: file.name })
          .run();
      }
    }
  }}
></div>


<!-- FloatingMenu for block formatting (appears on empty lines) -->
<!-- Element must exist before editor creation for extension to bind to it -->
<!-- On iOS Tauri, the native toolbar includes a block picker button instead -->
{#if !readonly && !useNativeToolbar}
  <FloatingMenuComponent
    bind:this={floatingMenuRef}
    {editor}
    bind:element={floatingMenuElement}
  />
{/if}

<!-- BubbleMenu for inline formatting (appears when text is selected) -->
<!-- On iOS Tauri, a native UIToolbar above the keyboard replaces this -->
{#if !readonly && !useNativeToolbar}
  <BubbleMenuComponent {editor} bind:element={bubbleMenuElement} {entryPath} {api} />
{/if}

<style global>
  /* Search highlighting */
  :global(.search-highlight) {
    background: oklch(0.9 0.15 90);
    border-radius: 2px;
  }
  :global(.search-highlight--current) {
    background: oklch(0.8 0.18 60);
    outline: 2px solid oklch(0.7 0.2 60);
  }
  :global(.dark .search-highlight) {
    background: oklch(0.45 0.12 90);
  }
  :global(.dark .search-highlight--current) {
    background: oklch(0.55 0.15 60);
    outline: 2px solid oklch(0.65 0.18 60);
  }

  :global(.editor-content) {
    outline: none;
    min-height: 100%;
    padding-bottom: 0;
    font-family: var(--editor-font-family);
    font-size: var(--editor-font-size);
    line-height: var(--editor-line-height);
  }

  :global(.editor-content.overscroll) {
    padding-bottom: 50vh;
  }

  :global(.editor-content > * + *) {
    margin-top: 0.75em;
  }

  :global(.editor-content h1) {
    font-size: 2em;
    font-weight: 700;
    line-height: 1.2;
    color: var(--foreground);
  }

  :global(.editor-content h2) {
    font-size: 1.5em;
    font-weight: 600;
    line-height: 1.3;
    color: var(--foreground);
  }

  :global(.editor-content h3) {
    font-size: 1.25em;
    font-weight: 600;
    line-height: 1.4;
    color: var(--foreground);
  }

  :global(.editor-content p) {
    line-height: var(--editor-line-height);
    color: var(--foreground);
  }

  :global(.editor-content ul),
  :global(.editor-content ol) {
    padding-left: 1.5em;
    color: var(--foreground);
  }

  :global(.editor-content ul:not([data-type="taskList"])) {
    list-style-type: disc;
  }

  :global(.editor-content ol) {
    list-style-type: decimal;
  }

  :global(.editor-content li) {
    margin: 0.25em 0;
  }

  :global(.editor-content ul[data-type="taskList"]) {
    list-style: none;
    padding-left: 0;
  }

  :global(.editor-content ul[data-type="taskList"] li) {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  :global(.editor-content ul[data-type="taskList"] li input) {
    margin-top: 4px;
    accent-color: var(--primary);
  }

  :global(.editor-content blockquote) {
    border-left: 3px solid var(--primary);
    padding-left: 1em;
    margin-left: 0;
    color: var(--muted-foreground);
    font-style: italic;
  }

  :global(.editor-content code) {
    background: var(--muted);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.9em;
  }

  :global(.editor-code-block) {
    background: var(--muted);
    padding: 12px 16px;
    border-radius: 6px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.9em;
    overflow-x: auto;
  }

  :global(.editor-code-block code) {
    background: none;
    padding: 0;
  }

  :global(.editor-link) {
    color: var(--primary);
    text-decoration: underline;
    cursor: pointer;
  }

  :global(.editor-content p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    color: var(--muted-foreground);
    pointer-events: none;
    height: 0;
  }

  :global(.editor-content hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 1.5em 0;
  }

  :global(.editor-content strong) {
    font-weight: 600;
  }

  :global(.editor-content em) {
    font-style: italic;
  }

  :global(.editor-content s) {
    text-decoration: line-through;
  }

  :global(.editor-content a) {
    color: var(--primary);
    text-decoration: underline;
  }

  :global(.editor-content a:hover) {
    opacity: 0.8;
  }

  :global(.editor-image) {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
    margin: 0.5em 0;
  }

  :global(.editor-video) {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
    margin: 0.5em 0;
  }

  :global(.editor-audio) {
    max-width: 100%;
    margin: 0.5em 0;
  }

  :global(.editor-image--loading) {
    min-height: 100px;
    min-width: 200px;
    background: var(--muted);
    border-radius: 6px;
    animation: shimmer 1.5s ease-in-out infinite;
    pointer-events: none;
  }

  @keyframes shimmer {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }

  /* Footnote ref styles */
  :global(.footnote-ref) {
    font-size: 0.75em;
    vertical-align: super;
    color: var(--primary);
    cursor: pointer;
    font-weight: 600;
    line-height: 1;
    -webkit-user-select: none;
    user-select: none;
  }

  :global(.footnote-ref:hover) {
    opacity: 0.8;
  }

  /* Template variable styles (fallback for renderHTML path) */
  :global(.template-variable) {
    display: inline;
  }

  /* Table styles */
  :global(.editor-content .tableWrapper) {
    overflow-x: auto;
    margin: 0.75em 0;
  }

  :global(.editor-content table) {
    border-collapse: collapse;
    width: 100%;
    min-width: max-content;
  }

  :global(.editor-content th),
  :global(.editor-content td) {
    border: 1px solid var(--border);
    padding: 8px 12px;
    text-align: left;
    vertical-align: top;
  }

  :global(.editor-content th) {
    background: var(--muted);
    font-weight: 600;
  }

  :global(.editor-content tr:hover td) {
    background: color-mix(in oklch, var(--muted) 50%, transparent);
  }

  :global(.editor-content .selectedCell) {
    background: color-mix(in oklch, var(--primary) 15%, transparent);
  }

  :global(.editor-content .column-resize-handle) {
    background-color: var(--primary);
    bottom: -2px;
    pointer-events: none;
    position: absolute;
    right: -2px;
    top: 0;
    width: 4px;
  }

  /* Table controls overlay (Notion-style grips + add buttons) */
  :global(.table-controls-container) {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    z-index: 10;
  }

  /* On mobile, always show controls (no hover) and keep them inside the table bounds */
  @media (max-width: 767px) {
    :global(.table-grip),
    :global(.table-add-btn) {
      opacity: 0.7;
      width: 20px;
      height: 20px;
    }
  }

  :global(.table-grip),
  :global(.table-add-btn),
  :global(.table-grip-popover) {
    pointer-events: auto;
  }

  :global(.table-grip) {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--popover);
    color: var(--muted-foreground);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity 0.15s ease, background 0.15s ease, color 0.15s ease;
  }

  /* Show grips when hovering the table area or when a grip is active */
  :global(.table-controls-container:hover .table-grip),
  :global(.table-grip.active) {
    opacity: 1;
  }

  :global(.table-grip:hover) {
    background: var(--accent);
    color: var(--accent-foreground);
    opacity: 1;
  }

  :global(.table-grip.active) {
    background: color-mix(in oklch, var(--primary) 15%, var(--popover));
    color: var(--primary);
    border-color: var(--primary);
  }

  :global(.table-grip.dragging) {
    opacity: 1;
    background: var(--primary);
    color: white;
    border-color: var(--primary);
    cursor: grabbing;
  }

  :global(.table-add-btn) {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px dashed var(--border);
    background: var(--popover);
    color: var(--muted-foreground);
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity 0.15s ease, background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }

  :global(.table-controls-container:hover .table-add-btn) {
    opacity: 1;
  }

  :global(.table-add-btn:hover) {
    background: var(--primary);
    color: white;
    border-color: var(--primary);
    border-style: solid;
    opacity: 1;
  }

  :global(.table-grip-popover) {
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 100;
    min-width: max-content;
    animation: tablePopoverFadeIn 0.12s ease;
  }

  @keyframes tablePopoverFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  :global(.table-grip-popover-item) {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 4px;
    background: transparent;
    border: none;
    color: var(--foreground);
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.1s ease;
    -webkit-user-select: none;
    user-select: none;
  }

  :global(.table-grip-popover-item:hover) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  :global(.table-grip-popover-item.destructive) {
    color: var(--destructive, oklch(0.577 0.245 27.325));
  }

  :global(.table-grip-popover-item.destructive:hover) {
    background: var(--destructive, oklch(0.577 0.245 27.325));
    color: white;
  }

  :global(.table-grip-popover-item.disabled) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  :global(.table-grip-popover-item.disabled:hover) {
    background: transparent;
    color: var(--foreground);
  }

  /* Collaborative cursor styles */
  :global(.collaboration-carets__caret) {
    border-left: 1px solid;
    border-right: 1px solid;
    margin-left: -1px;
    margin-right: -1px;
    pointer-events: none;
    position: relative;
    word-break: normal;
  }

  :global(.collaboration-carets__label) {
    border-radius: 3px 3px 3px 0;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    left: -1px;
    line-height: normal;
    padding: 0.1rem 0.3rem;
    position: absolute;
    top: -1.4em;
    user-select: none;
    white-space: nowrap;
  }

  /* ------------------------------------------------------------------ */
  /* Conditional block marker styles                                     */
  /* ------------------------------------------------------------------ */

  :global(.conditional-marker-wrapper) {
    margin: 4px 0;
    user-select: none;
  }

  /* Branch decoration: active (condition matches current context) */
  :global(.conditional-branch-active) {
    border-left: 3px solid color-mix(in oklch, var(--primary) 70%, transparent);
    padding-left: 12px !important;
    margin-left: -15px;
    background: color-mix(in oklch, var(--primary) 3%, transparent);
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  :global(.conditional-branch-active:hover) {
    border-left-color: var(--primary);
    box-shadow: inset 3px 0 0 0 var(--primary),
      2px 0 12px color-mix(in oklch, var(--primary) 8%, transparent);
  }

  /* Branch decoration: inactive (condition does not match) */
  :global(.conditional-branch-inactive) {
    border-left: 3px solid
      color-mix(in oklch, var(--muted-foreground) 20%, transparent);
    padding-left: 12px !important;
    margin-left: -15px;
    opacity: 0.5;
  }

  /* Preview mode: hide inactive branches and markers completely */
  :global(.conditional-branch-hidden) {
    display: none;
  }

  :global(.conditional-marker-hidden) {
    display: none;
  }

  /* Mobile-specific styles */
  @media (max-width: 767px) {
    :global(.editor-content) {
      /* Slightly larger touch targets on mobile */
      font-size: 16px; /* Prevents iOS zoom on focus */
    }

    :global(.editor-content h1) {
      font-size: 1.75em;
    }

    :global(.editor-content h2) {
      font-size: 1.35em;
    }

    :global(.editor-content h3) {
      font-size: 1.15em;
    }
  }
</style>
