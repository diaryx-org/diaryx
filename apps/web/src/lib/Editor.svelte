<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Editor, type JSONContent } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { Markdown } from "@tiptap/markdown";
  import Link from "@tiptap/extension-link";
  import TaskList from "@tiptap/extension-task-list";
  import TaskItem from "@tiptap/extension-task-item";
  import Placeholder from "@tiptap/extension-placeholder";
  import Code from "@tiptap/extension-code";
  import CodeBlock from "@tiptap/extension-code-block";
  import Typography from "@tiptap/extension-typography";
  import Image from "@tiptap/extension-image";
  import {
    formatMarkdownDestination,
    formatDroppedAttachmentPathForEntry,
    getPathForBlobUrl,
    getBlobUrl,
    isVideoFile,
    isAudioFile,
    isHtmlFile,
    isPreviewableAttachmentKind,
    queueResolveAttachment,
    stripWorkspacePrefixFromAttachmentPath,
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
  // Visibility directive extensions for audience filtering
  import { VisibilityMark } from "./extensions/VisibilityMark";
  import {
    VisibilityBlock,
    canWrapSelectionInVisibilityBlock,
    getVisibilityBlockForSelection,
  } from "./extensions/VisibilityBlock";
  import { EditorGutter } from "./extensions/EditorGutter";
  import { toast } from "svelte-sonner";
  import { getTemplateContextStore } from "./stores/templateContextStore.svelte";
  import { getAudiencePanelStore, CLEAR_BRUSH } from "./stores/audiencePanelStore.svelte";
  import { getEditorExtensions, getPluginExtensionsVersion } from "$lib/plugins/browserPluginManager.svelte";
  import { getPreservedEditorExtensions } from "$lib/plugins/preservedEditorExtensions.svelte";
  import { getTauriEditorExtensions } from "$lib/plugins/tauriEditorExtensions";
  import { getHttpEditorExtensions } from "$lib/plugins/httpEditorExtensions";
  import { setEditorExtensionIframeContext } from "$lib/plugins/editorExtensionFactory";
  import type { Api } from "$lib/backend/api";
  import { isTauri, isNativePluginBackend, isHttpBackend } from "$lib/backend/interface";
  import { isIOS } from "$lib/hooks/useMobile.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { getThemeStore } from "@/models/stores";
  import { getLinkFormatStore } from "$lib/stores/linkFormatStore.svelte";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import type { TreeNode } from "$lib/backend";
  import { shouldKeepBubbleMenuVisible } from "./editorMenuVisibility";

  // On iOS Tauri, a native UIToolbar replaces the web BubbleMenu
  const useNativeToolbar = isTauri() && isIOS();
  const nativeLinkFormatStore = useNativeToolbar ? getLinkFormatStore() : null;
  const pluginStore = getPluginStore();
  const themeStore = getThemeStore();
  const appearanceStore = getAppearanceStore();

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
      filename?: string;
      sourceEntryPath: string;
    }) => void;
    /** Called when user requests to preview a media attachment in the editor */
    onPreviewMedia?: (attachmentSrc: string) => void;
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
    onPreviewMedia,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);

  // Template context store — used by ConditionalBlock decorations to detect context changes
  const templateContextStore = getTemplateContextStore();

  // Audience panel store — read here so closures created during editor build
  // (e.g. onSelectionUpdate) capture a real reference, not a TDZ binding.
  const audiencePanelStore = getAudiencePanelStore();

  // FloatingMenu element ref - must exist before editor creation
  let floatingMenuElement: HTMLDivElement | undefined = $state();
  // FloatingMenu component ref - for programmatic expansion
  let floatingMenuRef: { expand: () => void } | undefined = $state();
  // BubbleMenu element ref - must exist before editor creation
  let bubbleMenuElement: HTMLDivElement | undefined = $state();
  let bubbleMenuLinkPopoverOpen = $state(false);
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates
  let templateContextDispatchErrorShown = false;
  let invalidContentRecoveryAttempted = false;
  let recoveringInvalidContent = false;

  // Track the last content prop value applied into the editor.
  // We intentionally do not update this on local typing, so parent prop updates
  // remain the only thing that can programmatically replace editor content.
  let lastAppliedContentProp: string | undefined = undefined;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
  let lastReadonly: boolean | null = null;
  let lastPlaceholder: string | null = null;
  let lastPluginKey: string | null = null;

  const HTML_ATTACHMENT_CSS_VAR_NAMES = [
    "--background", "--foreground",
    "--card", "--card-foreground",
    "--popover", "--popover-foreground",
    "--primary", "--primary-foreground",
    "--secondary", "--secondary-foreground",
    "--muted", "--muted-foreground",
    "--accent", "--accent-foreground",
    "--destructive",
    "--border", "--input", "--ring", "--radius",
    "--sidebar", "--sidebar-foreground",
    "--sidebar-primary", "--sidebar-primary-foreground",
    "--sidebar-accent", "--sidebar-accent-foreground",
    "--sidebar-border", "--sidebar-ring",
    "--editor-font-family", "--editor-font-size",
    "--editor-line-height", "--editor-content-max-width",
  ] as const;

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
    templateContextDispatchErrorShown = false;
    if (typeof globalThis !== 'undefined') {
      (globalThis as any).__diaryx_tiptapEditor = null;
    }
  }

  function collectHtmlAttachmentCssVars(): Record<string, string> {
    const computed = getComputedStyle(document.documentElement);
    const vars: Record<string, string> = {};
    for (const name of HTML_ATTACHMENT_CSS_VAR_NAMES) {
      const value = computed.getPropertyValue(name).trim();
      if (value) vars[name] = value;
    }
    return vars;
  }

  function postThemeToHtmlAttachmentIframe(
    iframe: HTMLIFrameElement,
    type: "init" | "theme-update",
  ) {
    const win = iframe.contentWindow;
    if (!win) return;
    win.postMessage(
      {
        type,
        theme: themeStore.isDark ? "dark" : "light",
        cssVars: collectHtmlAttachmentCssVars(),
        entry: entryPath ? { path: entryPath } : null,
      },
      "*",
    );
  }

  function postThemeToHtmlAttachmentIframes(type: "init" | "theme-update" = "theme-update") {
    if (!element) return;
    const iframes = element.querySelectorAll<HTMLIFrameElement>("iframe.editor-html-island");
    for (const iframe of iframes) {
      postThemeToHtmlAttachmentIframe(iframe, type);
    }
  }

  function clampHtmlAttachmentPreviewHeight(height: number): number {
    return Math.max(240, Math.min(Math.round(height), 4000));
  }

  /** Walk up from `el` to find the nearest ancestor with overflow scroll/auto. */
  function getScrollParent(el: HTMLElement | null): HTMLElement | Window {
    if (!el || el === document.documentElement) return window;
    const { overflow, overflowY } = getComputedStyle(el);
    if (/(auto|scroll)/.test(overflow + overflowY)) return el;
    return getScrollParent(el.parentElement);
  }

  function normalizeTopLevelInlineImageNodes(
    doc: JSONContent | null | undefined,
  ): JSONContent | null {
    if (!doc || doc.type !== "doc" || !Array.isArray(doc.content)) {
      return null;
    }

    const normalized: JSONContent[] = [];
    let bufferedInlineImages: JSONContent[] = [];
    let changed = false;

    const flushInlineImages = () => {
      if (bufferedInlineImages.length === 0) return;
      normalized.push({
        type: "paragraph",
        content: bufferedInlineImages,
      });
      bufferedInlineImages = [];
      changed = true;
    };

    for (const node of doc.content) {
      if (node?.type === "image") {
        bufferedInlineImages.push(node);
        continue;
      }

      flushInlineImages();
      normalized.push(node);
    }

    flushInlineImages();

    if (!changed) return null;

    return {
      ...doc,
      content: normalized,
    };
  }

  function createEditor(overrideContent?: string | JSONContent) {
    // Update global iframe context so iframe node views read the current entry
    setEditorExtensionIframeContext({ entryPath, api: api ?? null });

    // When rebuilding an existing editor (e.g. plugin extension change) we
    // preserve the live editor markdown so unsaved edits aren't dropped.
    const preservingLiveEdits = overrideContent === undefined && editor !== null;
    const initialContent =
      overrideContent ?? (editor ? appendFootnoteDefinitions(editor) : content);
    const initialMarkdown =
      typeof initialContent === "string"
        ? initialContent
        : (lastAppliedContentProp ?? content ?? "");
    // After a live-edits-preserving rebuild, the editor body diverges from the
    // `content` prop (the prop only updates when the parent reloads from disk).
    // We must seed lastAppliedContentProp with the current prop value so the
    // sync effect at the bottom of this file doesn't observe a mismatch and
    // overwrite the preserved live edits with stale prop content. (See the
    // data-loss bug where installing a plugin caused recent edits to vanish.)
    const appliedContentPropSeed = preservingLiveEdits
      ? (content ?? initialMarkdown)
      : initialMarkdown;
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

    function handleEditorLinkClick(event: MouseEvent): boolean {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return false;

      const link = target.closest("a[href]");
      if (!(link instanceof HTMLAnchorElement)) return false;

      const href = link.getAttribute("href")?.trim() ?? "";
      if (!href) return false;

      // Prevent the webview from treating note links as navigation.
      event.preventDefault();
      event.stopPropagation();

      if (onLinkClick) {
        onLinkClick(href);
        return true;
      }

      if (href.startsWith("http://") || href.startsWith("https://")) {
        window.open(href, "_blank", "noopener,noreferrer");
        return true;
      }

      return true;
    }

    // Build extensions array
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const extensions: any[] = [
      StarterKit.configure({
        code: false, // We register Code explicitly below with custom excludes
        codeBlock: false, // We'll use the separate extension
        link: false, // Disable Link in StarterKit; we register Link explicitly below
      }),
      // Override Code's excludes so visibilityMark (and other metadata-like
      // marks) can coexist with inline code. Without this, ProseMirror strips
      // the vis mark from code text nodes, splitting `:vis[text with `code` in it]{a}`
      // into three fragments on serialization.
      Code.extend({
        excludes: "bold italic strike",
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
                  return handleEditorLinkClick(event);
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
        addAttributes() {
          return {
            ...this.parent?.(),
            width: { default: null },
            height: { default: null },
          };
        },
        renderMarkdown: (node: any) => {
          const src = stripWorkspacePrefixFromAttachmentPath(
            node.attrs?.src ?? "",
            workspaceStore.backend?.getWorkspacePath?.() ?? null,
          );
          let alt = node.attrs?.alt ?? "";
          const title = node.attrs?.title ?? "";
          const width = node.attrs?.width;
          const height = node.attrs?.height;
          const formattedSrc = formatMarkdownDestination(src);
          // Obsidian-style: embed dimensions as |WIDTHxHEIGHT or |WIDTH in alt text
          if (width) {
            alt += height ? `|${width}x${height}` : `|${width}`;
          }
          return title
            ? `![${alt}](${formattedSrc} "${title}")`
            : `![${alt}](${formattedSrc})`;
        },
        parseMarkdown: (token: any, h: any) => {
          const rawAlt = token.text || '';
          const href = token.href || '';
          const title = token.title || '';
          // Parse Obsidian-style dimensions: alt|WIDTHxHEIGHT or alt|WIDTH
          const dimMatch = rawAlt.match(/^(.*?)\|(\d+)(?:x(\d+))?$/);
          const attrs: Record<string, any> = {
            src: href,
            alt: dimMatch ? dimMatch[1] : rawAlt,
            title,
            width: dimMatch ? parseInt(dimMatch[2]) : null,
            height: dimMatch && dimMatch[3] ? parseInt(dimMatch[3]) : null,
          };
          return h.createNode('image', attrs);
        },
        addNodeView() {
          // Capture entryPath and api from the outer scope (Editor props)
          const ep = entryPath;
          const epApi = api;
          return ({ node, HTMLAttributes, getPos, editor: viewEditor }) => {
            const src = node.attrs.src || "";
            const alt = node.attrs.alt || "";
            const title = node.attrs.title || "";

            const isLocalPath = src && !src.startsWith('blob:') && !src.startsWith('http://') && !src.startsWith('https://') && !src.startsWith('data:');

            // For local paths, check media type from the raw path
            // For blob URLs, look up the original path
            const originalPath = isLocalPath ? src : getPathForBlobUrl(src);
            const checkPath = originalPath || src;
            const noteBackedTypeHint =
              checkPath.endsWith(".md") && (title || alt)
                ? (title || alt)
                : checkPath;
            const isVideo = isVideoFile(noteBackedTypeHint);
            const isAudio = isAudioFile(noteBackedTypeHint);
            const isHtmlEmbed = isHtmlFile(noteBackedTypeHint);

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

            let mediaEl: HTMLElement;
            let removeHtmlAttachmentListeners: (() => void) | null = null;

            if (isHtmlEmbed) {
              const iframe = document.createElement("iframe");
              iframe.setAttribute("sandbox", "allow-scripts");
              iframe.className = "editor-image editor-html-island";
              iframe.scrolling = "auto";
              iframe.style.display = "block";
              iframe.addEventListener("load", () => {
                postThemeToHtmlAttachmentIframe(iframe, "init");
              });
              if (alt) iframe.title = alt;
              iframe.style.width = "100%";
              iframe.style.height = node.attrs.height
                ? `${node.attrs.height}px`
                : "420px";
              iframe.style.minHeight = "240px";
              iframe.style.border = "1px solid var(--border-color, #e0e0e0)";
              iframe.style.borderRadius = "6px";
              if (isLocalPath && epApi) {
                const cached = getBlobUrl(src);
                if (cached) {
                  iframe.src = cached;
                } else {
                  setLoading(iframe);
                  queueResolveAttachment(epApi, ep, src).then((blobUrl) => {
                    if (blobUrl) {
                      iframe.src = blobUrl;
                      clearLoading(iframe);
                    }
                  });
                }
              } else {
                iframe.src = src;
              }

              const handleHtmlAttachmentMessage = (event: MessageEvent) => {
                if (event.source !== iframe.contentWindow) return;
                const data = event.data;
                if (!data || typeof data !== "object") return;
                if ((data as { type?: string }).type !== "diaryx-html-attachment-size") return;
                if (typeof node.attrs.height === "number" && Number.isFinite(node.attrs.height)) {
                  return;
                }

                const nextHeight = (data as { height?: unknown }).height;
                const heightValue =
                  typeof nextHeight === "number"
                    ? nextHeight
                    : typeof nextHeight === "string"
                      ? Number.parseFloat(nextHeight)
                      : Number.NaN;

                if (!Number.isFinite(heightValue) || heightValue <= 0) return;
                iframe.style.height = `${clampHtmlAttachmentPreviewHeight(heightValue)}px`;
              };

              window.addEventListener("message", handleHtmlAttachmentMessage);
              removeHtmlAttachmentListeners = () => {
                window.removeEventListener("message", handleHtmlAttachmentMessage);
              };
              mediaEl = iframe;
            } else if (isVideo) {
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
              mediaEl = video;
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
              mediaEl = audio;
            } else {
              const img = document.createElement("img");
              img.alt = alt;
              img.loading = "lazy";
              img.className = HTMLAttributes.class || "editor-image";
              if (title) img.title = title;
              // Apply stored dimensions
              if (node.attrs.width) img.style.width = `${node.attrs.width}px`;
              if (node.attrs.height) img.style.height = `${node.attrs.height}px`;
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
              mediaEl = img;
            }

            // ── Wrapper with selection ring + dropdown ──────────────
            const wrapper = document.createElement("div");
            wrapper.className = "editor-media-wrapper";
            if (isHtmlEmbed) {
              wrapper.classList.add("editor-media-wrapper--html");
              wrapper.style.width =
                typeof node.attrs.width === "number" && Number.isFinite(node.attrs.width)
                  ? `${node.attrs.width}px`
                  : "100%";
            }
            wrapper.appendChild(mediaEl);

            let selected = false;
            let menu: HTMLElement | null = null;

            function removeMenu() {
              if (menu) {
                menu.remove();
                menu = null;
              }
            }

            function showMenu(clientX: number, clientY: number) {
              removeMenu();
              menu = document.createElement("div");
              menu.className = "editor-media-menu";

              const items: { label: string; action: () => void }[] = [];

              // Preview (images/videos only, not audio)
              if (!isAudio && onPreviewMedia) {
                items.push({
                  label: "Preview",
                  action: () => {
                    removeMenu();
                    onPreviewMedia(src);
                  },
                });
              }

              // Alt text
              if (!isAudio && !isVideo) {
                items.push({
                  label: "Edit alt text",
                  action: () => {
                    removeMenu();
                    const currentAlt = node.attrs.alt || "";
                    const newAlt = window.prompt("Alt text:", currentAlt);
                    if (newAlt !== null) {
                      const pos = getPos();
                      if (pos != null) {
                        viewEditor.chain().focus()
                          .command(({ tr }) => {
                            tr.setNodeMarkup(pos, undefined, { ...node.attrs, alt: newAlt });
                            return true;
                          }).run();
                      }
                    }
                  },
                });
              }

              // Replace
              items.push({
                label: "Replace",
                action: () => {
                  removeMenu();
                  const input = document.createElement("input");
                  input.type = "file";
                  input.accept = isVideo ? "video/*" : isAudio ? "audio/*" : "image/*,video/*";
                  input.onchange = async () => {
                    const file = input.files?.[0];
                    if (!file || !onFileDrop) return;
                    const result = await onFileDrop(file);
                    if (result) {
                      const pos = getPos();
                      if (pos != null) {
                        viewEditor.chain().focus()
                          .command(({ tr }) => {
                            tr.setNodeMarkup(pos, undefined, {
                              ...node.attrs,
                              src: result.blobUrl || result.attachmentPath,
                            });
                            return true;
                          }).run();
                      }
                    }
                  };
                  input.click();
                },
              });

              // Delete
              items.push({
                label: "Delete",
                action: () => {
                  removeMenu();
                  const pos = getPos();
                  if (pos != null) {
                    viewEditor.chain().focus()
                      .command(({ tr }) => {
                        tr.delete(pos, pos + node.nodeSize);
                        return true;
                      }).run();
                  }
                },
              });

              for (const item of items) {
                const btn = document.createElement("button");
                btn.type = "button";
                btn.className = "editor-media-menu-item";
                btn.textContent = item.label;
                btn.addEventListener("mousedown", (e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  item.action();
                });
                menu.appendChild(btn);
              }

              // Resize submenu (images and HTML embeds)
              if (!isAudio && !isVideo) {
                const resizeContainer = document.createElement("div");
                resizeContainer.className = "editor-media-menu-submenu-container";

                const resizeBtn = document.createElement("button");
                resizeBtn.type = "button";
                resizeBtn.className = "editor-media-menu-item editor-media-menu-item--submenu";
                resizeBtn.innerHTML = 'Resize <span class="editor-media-menu-arrow">&#9656;</span>';

                const submenu = document.createElement("div");
                submenu.className = "editor-media-submenu";

                function applyResize(w: number | null, h: number | null) {
                  removeMenu();
                  const pos = getPos();
                  if (pos != null) {
                    viewEditor.chain().focus()
                      .command(({ tr }) => {
                        tr.setNodeMarkup(pos, undefined, { ...node.attrs, width: w, height: h });
                        return true;
                      }).run();
                  }
                }

                const imgEl = mediaEl instanceof HTMLImageElement ? mediaEl : null;
                const containerWidth =
                  wrapper.parentElement instanceof HTMLElement
                    ? Math.round(wrapper.parentElement.getBoundingClientRect().width)
                    : Math.round(wrapper.getBoundingClientRect().width);
                const baseWidth =
                  imgEl?.naturalWidth ||
                  (typeof node.attrs.width === "number" ? node.attrs.width : null) ||
                  containerWidth ||
                  null;
                const presetHeight = isHtmlEmbed ? (node.attrs.height ?? null) : null;
                const presets = [
                  { label: "25%", factor: 0.25 },
                  { label: "50%", factor: 0.5 },
                  { label: "75%", factor: 0.75 },
                  { label: "100% (original)", factor: 1 },
                ];

                for (const preset of presets) {
                  const presetBtn = document.createElement("button");
                  presetBtn.type = "button";
                  presetBtn.className = "editor-media-menu-item";
                  presetBtn.textContent = preset.label;
                  presetBtn.addEventListener("mousedown", (e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    if (preset.factor === 1) {
                      // Reset to full-width/default sizing — clear stored dimensions
                      applyResize(null, null);
                    } else if (baseWidth) {
                      applyResize(Math.round(baseWidth * preset.factor), presetHeight);
                    }
                  });
                  submenu.appendChild(presetBtn);
                }

                // Custom size option
                const customBtn = document.createElement("button");
                customBtn.type = "button";
                customBtn.className = "editor-media-menu-item";
                customBtn.textContent = "Custom\u2026";
                customBtn.addEventListener("mousedown", (e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  const currentW = node.attrs.width || baseWidth || "";
                  const input = window.prompt(
                    "Enter size (width or widthxheight):",
                    String(currentW),
                  );
                  if (input !== null) {
                    const sizeMatch = input.trim().match(/^(\d+)(?:x(\d+))?$/);
                    if (sizeMatch) {
                      const w = parseInt(sizeMatch[1]);
                      const h = sizeMatch[2] ? parseInt(sizeMatch[2]) : null;
                      applyResize(w, h);
                    }
                  }
                });
                submenu.appendChild(customBtn);

                resizeContainer.appendChild(resizeBtn);
                resizeContainer.appendChild(submenu);
                // Insert resize before the last item (Delete)
                const deleteBtn = menu.lastElementChild;
                if (deleteBtn) {
                  menu.insertBefore(resizeContainer, deleteBtn);
                } else {
                  menu.appendChild(resizeContainer);
                }
              }

              // Position at click point, flip above if not enough space below
              document.body.appendChild(menu);
              const menuRect = menu.getBoundingClientRect();
              const spaceBelow = window.innerHeight - clientY;
              const spaceAbove = clientY;
              // Also clamp horizontally so it doesn't go off the right edge
              const left = Math.min(clientX, window.innerWidth - menuRect.width - 8);
              if (spaceBelow >= menuRect.height + 8) {
                menu.style.top = `${clientY + 4}px`;
              } else if (spaceAbove >= menuRect.height + 8) {
                menu.style.top = `${clientY - menuRect.height - 4}px`;
              } else {
                // Not enough room either way — just pin to bottom
                menu.style.top = `${window.innerHeight - menuRect.height - 8}px`;
              }
              menu.style.left = `${Math.max(8, left)}px`;
            }

            // Show menu on click when already selected (desktop only)
            wrapper.addEventListener("mousedown", (e) => {
              if (useNativeToolbar) return;
              if (selected && !menu && !(e.target as HTMLElement).closest(".editor-media-menu")) {
                // Prevent ProseMirror from stealing focus / re-selecting
                e.preventDefault();
                showMenu(e.clientX, e.clientY);
              }
            });

            // Native iOS context menu: send image metadata on touchstart
            if (useNativeToolbar) {
              wrapper.addEventListener("touchstart", () => {
                const imgEl = wrapper.querySelector("img");
                if (!imgEl || isAudio) return;

                // Generate a small thumbnail for the peek preview
                let thumbnailBase64: string | null = null;
                try {
                  const canvas = document.createElement("canvas");
                  const maxDim = 300;
                  const scale = Math.min(maxDim / imgEl.naturalWidth, maxDim / imgEl.naturalHeight, 1);
                  canvas.width = Math.round(imgEl.naturalWidth * scale);
                  canvas.height = Math.round(imgEl.naturalHeight * scale);
                  const ctx = canvas.getContext("2d");
                  if (ctx) {
                    ctx.drawImage(imgEl, 0, 0, canvas.width, canvas.height);
                    thumbnailBase64 = canvas.toDataURL("image/jpeg", 0.7).split(",")[1] || null;
                  }
                } catch {
                  // Canvas taint from cross-origin blob URLs — fall back to no preview
                }

                (window as any).webkit?.messageHandlers?.editorToolbar?.postMessage({
                  type: "imageContextPrepare",
                  nodePos: getPos(),
                  src: src,
                  alt: node.attrs.alt || "",
                  width: node.attrs.width,
                  height: node.attrs.height,
                  naturalWidth: imgEl.naturalWidth || null,
                  naturalHeight: imgEl.naturalHeight || null,
                  isVideo: isVideo,
                  thumbnailBase64: thumbnailBase64,
                });
              }, { passive: true });

              const clearContextCache = () => {
                setTimeout(() => {
                  (window as any).webkit?.messageHandlers?.editorToolbar?.postMessage({
                    type: "imageContextClear",
                  });
                }, 2500);
              };
              wrapper.addEventListener("touchend", clearContextCache, { passive: true });
              wrapper.addEventListener("touchcancel", clearContextCache, { passive: true });
            }

            // Close dropdown when clicking outside
            function handleDocClick(e: MouseEvent) {
              if (menu && !menu.contains(e.target as Node) && !wrapper.contains(e.target as Node)) {
                removeMenu();
              }
            }
            document.addEventListener("mousedown", handleDocClick, true);

            return {
              dom: wrapper,
              stopEvent(event: Event) {
                // Let video/audio controls work normally
                if (isVideo || isAudio) {
                  const target = event.target as HTMLElement;
                  if (mediaEl.contains(target) && target !== mediaEl) return true;
                  if (target === mediaEl && event.type !== "mousedown") return true;
                }
                return false;
              },
              selectNode() {
                selected = true;
                wrapper.classList.add("editor-media-selected");
              },
              deselectNode() {
                selected = false;
                wrapper.classList.remove("editor-media-selected");
                removeMenu();
              },
              ignoreMutation() {
                return true;
              },
              destroy() {
                removeMenu();
                document.removeEventListener("mousedown", handleDocClick, true);
                removeHtmlAttachmentListeners?.();
              },
            };
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
      // Visibility directive extensions for audience filtering
      EditorGutter,
      VisibilityMark,
      VisibilityBlock,
      // Footnote extension
      FootnoteRef,
      // Raw HTML block extension
      HtmlBlock.configure({
        entryPath,
        api,
        useNativeToolbar,
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
      // Tauri / HTTP-with-plugins: use native backend (plugins loaded by host)
      // Web: use browser Extism plugins (loaded async, editor rebuilds when ready)
      ...(isTauri()
        ? getTauriEditorExtensions()
        : isHttpBackend() && isNativePluginBackend()
          ? getHttpEditorExtensions()
          : getEditorExtensions()),
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
            appendTo: () => document.body,
            options: {
              strategy: "fixed",
              placement: "top",
              offset: 10,
              // Keep BubbleMenu within viewport bounds (especially important on mobile)
              shift: {
                padding: 8,
              },
              scrollTarget: getScrollParent(element),
              onShow: () => {
                if (bubbleMenuElement) {
                  bubbleMenuElement.style.pointerEvents = "auto";
                }
              },
              onHide: () => {
                if (bubbleMenuElement) {
                  bubbleMenuElement.style.pointerEvents = "none";
                }
                // Panel visibility is managed by audiencePanelStore now
              },
            },
            shouldShow: ({ editor: ed, view, state, from, to }) => {
              // Must be editable
              if (!ed.isEditable) return false;

              // Keep the bubble menu open while focus moves into the menu itself
              // (for example, the link popover URL field) or while the link
              // popover is explicitly open. Without this, opening the link UI
              // can immediately hide the BubbleMenu on desktop webviews.
              if (
                !shouldKeepBubbleMenuVisible({
                  bubbleMenuElement,
                  activeElement: document.activeElement,
                  editorHasFocus: view.hasFocus(),
                  linkPopoverOpen: bubbleMenuLinkPopoverOpen,
                })
              ) {
                return false;
              }

              if (bubbleMenuLinkPopoverOpen) {
                return true;
              }

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

    function buildEditorInstance(editorContent: string | JSONContent) {
      return new Editor({
        element,
        extensions,
        content:
          typeof editorContent === "string"
            ? preprocessFootnotes(editorContent)
            : editorContent,
        ...(typeof editorContent === "string"
          ? { contentType: "markdown" as const }
          : {}),
        editable: !readonly,
        onCreate: () => {
          // Track the last external content value that has been applied so we don't
          // overwrite the editor unless the prop actually changes.
          lastAppliedContentProp = appliedContentPropSeed;
        },
        onUpdate: () => {
          if (onchange && !isUpdatingContent) {
            onchange();
          }
        },
        onSelectionUpdate: ({ editor: ed }) => {
          const { from, to } = ed.state.selection;
          audiencePanelStore.setHasEditorSelection(from !== to);
        },
        onBlur: () => {
          onblur?.();
        },
        editorProps: {
          attributes: {
            class: "editor-content",
          },
          handleDOMEvents: {
            click: (_view, event) => handleEditorLinkClick(event),
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
      editor = buildEditorInstance(initialContent);
      invalidContentRecoveryAttempted = false;
    } catch (err) {
      console.error("[Editor] Failed to load content, recovering with empty document:", err);
      toast.error("Entry contains invalid content", {
        description: "The editor recovered by loading an empty document. Your data is safe — try re-opening the entry or removing incompatible plugins.",
      });
      editor = buildEditorInstance("");
      invalidContentRecoveryAttempted = false;
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

      let nativeToolbarAudiences: string[] = [];

      function mergeAudienceLists(available: string[], current: string[]): string[] {
        const result: string[] = [];
        const seen = new Set<string>();
        for (const audience of [...available, ...current]) {
          const trimmed = audience.trim();
          if (!trimmed) continue;
          const key = trimmed.toLowerCase();
          if (seen.has(key)) continue;
          seen.add(key);
          result.push(trimmed);
        }
        return result;
      }

      async function loadNativeToolbarAudiences(): Promise<string[]> {
        const rootPath = workspaceStore.tree?.path ?? "";
        if (!api || !rootPath) {
          nativeToolbarAudiences = [];
          return nativeToolbarAudiences;
        }

        try {
          nativeToolbarAudiences = await api.getAvailableAudiences(rootPath);
        } catch {
          nativeToolbarAudiences = [];
        }
        return nativeToolbarAudiences;
      }

      function readNativeBlockSelection() {
        return editor ? getVisibilityBlockForSelection(editor.state) : null;
      }

      function readNativeInlineAudiences(): string[] {
        if (!editor) return [];

        const attrs = editor.getAttributes("visibilityMark");
        if (attrs?.audiences?.length) return attrs.audiences as string[];

        const { from, to } = editor.state.selection;
        let found: string[] | null = null;
        editor.state.doc.nodesBetween(from, to, (node) => {
          if (found) return false;
          for (const mark of node.marks) {
            if (mark.type.name === "visibilityMark" && mark.attrs.audiences?.length) {
              found = mark.attrs.audiences as string[];
              return false;
            }
          }
        });
        if (found) return found;

        const storedMarks = editor.state.storedMarks ?? [];
        for (const mark of storedMarks) {
          if (mark.type.name === "visibilityMark" && mark.attrs.audiences?.length) {
            return mark.attrs.audiences as string[];
          }
        }

        return [];
      }

      function shouldUseNativeVisibilityBlock(): boolean {
        if (!editor) return false;
        if (readNativeBlockSelection() !== null) return true;
        if (readNativeInlineAudiences().length > 0) return false;
        return canWrapSelectionInVisibilityBlock(editor.state);
      }

      function readNativeVisibilityAudiences(): string[] {
        const block = readNativeBlockSelection();
        if (block) return block.open.audiences;
        return readNativeInlineAudiences();
      }

      function writeNativeVisibilityAudiences(audiences: string[]) {
        if (!editor) return;

        const nextAudiences = mergeAudienceLists([], audiences);
        const useBlock = shouldUseNativeVisibilityBlock();
        if (nextAudiences.length === 0) {
          if (useBlock) {
            editor.chain().focus().unsetVisibilityBlock().run();
          } else {
            editor.chain().focus().unsetVisibility().run();
          }
          return;
        }

        if (useBlock) {
          editor.chain().focus().setVisibilityBlock({ audiences: nextAudiences }).run();
        } else {
          editor.chain().focus().setVisibility({ audiences: nextAudiences }).run();
        }
      }

      function nativeVisibilityState(available = nativeToolbarAudiences) {
        const currentAudiences = readNativeVisibilityAudiences();
        const blockSelection = readNativeBlockSelection();
        const shouldUseBlock = shouldUseNativeVisibilityBlock();
        return {
          audiences: mergeAudienceLists(available, currentAudiences),
          currentAudiences,
          mode: blockSelection ? "block" : shouldUseBlock ? "wrap-block" : "inline",
          active: currentAudiences.length > 0,
        };
      }

      void loadNativeToolbarAudiences();

      (globalThis as any).__diaryx_nativeToolbar = {
        triggerPreviewMedia: (mediaSrc: string) => {
          if (onPreviewMedia) onPreviewMedia(mediaSrc);
        },
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
        getVisibilityPickerState: () => nativeVisibilityState(),
        getVisibilityPickerStateAsync: async () => {
          const available = await loadNativeToolbarAudiences();
          return nativeVisibilityState(available);
        },
        hasVisibility: () => readNativeVisibilityAudiences().length > 0,
        toggleVisibilityAudience: (audience: string) => {
          const name = audience.trim();
          if (!name) return;

          const current = readNativeVisibilityAudiences();
          const isSelected = current.some((a) => a.toLowerCase() === name.toLowerCase());
          const nextAudiences = isSelected
            ? current.filter((a) => a.toLowerCase() !== name.toLowerCase())
            : [...current, name];
          if (!isSelected) {
            nativeToolbarAudiences = mergeAudienceLists(nativeToolbarAudiences, [name]);
          }
          writeNativeVisibilityAudiences(nextAudiences);
        },
        removeVisibility: () => {
          writeNativeVisibilityAudiences([]);
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

  $effect(() => {
    const isDark = themeStore.isDark;
    void isDark;
    void appearanceStore.appearance;
    postThemeToHtmlAttachmentIframes("theme-update");
  });

  function recoverInvalidEditorState(): boolean {
    if (!editor || recoveringInvalidContent || invalidContentRecoveryAttempted) {
      return false;
    }

    invalidContentRecoveryAttempted = true;
    recoveringInvalidContent = true;

    let recoveryContent: string | JSONContent = lastAppliedContentProp ?? content ?? "";

    try {
      editor.state.doc.check();
      recoveryContent = appendFootnoteDefinitions(editor);
    } catch (err) {
      try {
        const normalizedDoc = normalizeTopLevelInlineImageNodes(editor.getJSON());
        if (normalizedDoc) {
          recoveryContent = normalizedDoc;
        } else {
          recoveryContent = appendFootnoteDefinitions(editor);
        }
      } catch (serializationErr) {
        console.warn(
          "[Editor] Failed to serialize invalid content for recovery, falling back to last applied content:",
          serializationErr,
        );
        console.warn("[Editor] Original invalid-content error:", err);
      }
    }

    try {
      createEditor(recoveryContent);
      invalidContentRecoveryAttempted = true;
      return true;
    } finally {
      recoveringInvalidContent = false;
    }
  }

  export function getMarkdown(): string | undefined {
    if (!editor) return undefined;
    return appendFootnoteDefinitions(editor);
  }

  /**
   * Set content from markdown
   */
  /**
   * Tell the editor that the given markdown was saved from its own content,
   * so the content-sync $effect should not re-apply it (which would reset
   * the cursor position).
   */
  export function acknowledgeSavedContent(markdown: string): void {
    lastAppliedContentProp = markdown;
  }

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
   * Insert an attachment at the current cursor position, using the appropriate
   * block type for drawing/audio files or a regular image for everything else.
   */
  export async function handleAttachmentDrop(
    attachmentRaw: string,
    sourceEntryPath?: string,
  ): Promise<void> {
    if (!editor) return;
    const { path: attachmentPath, label } = await formatDroppedAttachmentPathForEntry(
      api,
      entryPath,
      attachmentRaw,
      {
        sourceEntryPath,
        workspacePath: workspaceStore.backend?.getWorkspacePath?.() ?? null,
      },
    );
    const filename = attachmentPath.split("/").pop() || label || "";
    const drawingMatch = filename.match(/^drawing-(.+)\.svg$/);
    const audioMatch = filename.match(/^audio-(.+)\.\w+$/);

    if (drawingMatch && editor.extensionManager.extensions.some(ext => ext.name === "drawingBlock")) {
      editor.chain().insertContent({
        type: "drawingBlock",
        attrs: { source: `${drawingMatch[1]}](${formatMarkdownDestination(attachmentPath)}` },
      }).run();
      return;
    }
    if (audioMatch && editor.extensionManager.extensions.some(ext => ext.name === "audioBlock")) {
      editor.chain().insertContent({
        type: "audioBlock",
        attrs: { source: `${audioMatch[1]}](${formatMarkdownDestination(attachmentPath)}` },
      }).run();
      return;
    }
    editor.chain().setImage({ src: attachmentPath, alt: label || filename }).run();
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
      try {
        const tr = editor.state.tr.setMeta("templateContextChanged", true);
        editor.view.dispatch(tr);
      } catch (err) {
        if (recoverInvalidEditorState()) {
          return;
        }
        if (!templateContextDispatchErrorShown) {
          templateContextDispatchErrorShown = true;
          console.error("[Editor] Failed to refresh template-context decorations:", err);
          toast.error("Could not refresh editor decorations", {
            description: "The entry contains invalid content, so some audience or conditional highlights may be stale until you reopen it.",
          });
        }
      }
    }
  });

  // ── Paint-mode apply ────────────────────────────────────────────────
  // Register a callback so the audience panel can apply the brush via a button.
  // (audiencePanelStore is declared near the top of the script — see above.)

  function applyPaintBrush(): boolean {
    if (!editor || readonly) return false;
    const brushes = audiencePanelStore.paintBrushes;
    if (brushes.length === 0) return false;

    const { from, to } = editor.state.selection;
    if (from === to) return false;

    const isClear = brushes.length === 1 && brushes[0] === CLEAR_BRUSH;

    if (isClear) {
      try { editor.chain().focus().unsetVisibility().run(); } catch { /* noop */ }
      try { editor.chain().focus().unsetVisibilityBlock().run(); } catch { /* noop */ }
    } else {
      const { state } = editor;
      const isFullBlock =
        state.doc.resolve(from).parentOffset === 0 &&
        state.doc.resolve(to).parentOffset === state.doc.resolve(to).parent.content.size;

      if (isFullBlock) {
        editor.chain().focus().setVisibilityBlock({ audiences: [...brushes] }).run();
      } else {
        editor.chain().focus().setVisibility({ audiences: [...brushes] }).run();
      }
    }
    return true;
  }

  // Register/unregister the callback with the panel store
  $effect(() => {
    audiencePanelStore.registerApplyPaintBrush(applyPaintBrush);
    return () => {
      audiencePanelStore.registerApplyPaintBrush(null);
      audiencePanelStore.setHasEditorSelection(false);
    };
  });
</script>

<!-- Editor content area - scrolling handled by parent EditorContent -->
<div
  bind:this={element}
  class="min-h-full"
  role="application"
  ondragover={(e) => {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = e.dataTransfer.types.includes("text/x-diaryx-attachment") ? "move" : "copy";
    }
  }}
  ondrop={async (e) => {
    e.preventDefault();

    // Handle attachment drops from the sidebar.
    const attachmentRaw = e.dataTransfer?.getData("text/x-diaryx-attachment");
    if (attachmentRaw && editor) {
      // Resolve drop coordinates to a document position so the content is
      // inserted where the user actually dropped, not at the current cursor.
      const dropPos = editor.view.posAtCoords({ left: e.clientX, top: e.clientY });
      editor.commands.focus();
      if (dropPos) {
        editor.commands.setTextSelection(dropPos.pos);
      } else {
        editor.commands.setTextSelection(editor.state.doc.content.size);
      }
      const sourceEntryPath = e.dataTransfer?.getData("text/x-diaryx-source-entry") || undefined;
      await handleAttachmentDrop(attachmentRaw, sourceEntryPath);
      return;
    }

    // Handle OS file drops
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
  <BubbleMenuComponent
    {editor}
    bind:element={bubbleMenuElement}
    bind:linkPopoverOpen={bubbleMenuLinkPopoverOpen}
    {entryPath}
    {api}
  />
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

  /* Media wrapper: selection ring + dropdown */
  :global(.editor-media-wrapper) {
    position: relative;
    display: inline-block;
    max-width: 100%;
    border-radius: 6px;
    transition: box-shadow 0.15s ease;
    cursor: pointer;
  }

  :global(.editor-media-wrapper--html) {
    display: block;
    width: 100%;
  }

  :global(.editor-media-wrapper img),
  :global(.html-block-preview img) {
    -webkit-touch-callout: none;
  }

  :global(.editor-media-selected) {
    box-shadow: 0 0 0 2px var(--primary);
    border-radius: 6px;
  }

  :global(.editor-media-menu) {
    position: fixed;
    z-index: 50;
    min-width: 140px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.12);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }

  :global(.editor-media-menu-item) {
    display: block;
    width: 100%;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: inherit;
    font-size: 13px;
    text-align: left;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.1s;
  }

  :global(.editor-media-menu-item:hover) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  :global(.editor-media-menu-item--submenu) {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  :global(.editor-media-menu-arrow) {
    font-size: 10px;
    opacity: 0.6;
    margin-left: 8px;
  }

  :global(.editor-media-menu-submenu-container) {
    position: relative;
  }

  :global(.editor-media-submenu) {
    display: none;
    position: absolute;
    left: 100%;
    top: 0;
    min-width: 130px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.12);
    padding: 4px;
    flex-direction: column;
    z-index: 51;
  }

  :global(.editor-media-menu-submenu-container:hover > .editor-media-submenu) {
    display: flex;
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

  /* ── EditorGutter ────────────────────────────────────────────────── */

  /* When any gutter indicators are present, reserve left space */
  :global(.editor-content.editor-gutter-active) {
    padding-left: 20px;
    position: relative;
  }

  :global(.gutter-indicator) {
    position: absolute;
    left: 0;
    pointer-events: auto;
    z-index: 5;
    user-select: none;
  }

  :global(.gutter-dot) {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    margin-top: 0.55em; /* Vertically center with first line of text */
  }

  /* Multi-dot: stacked vertically when multiple audiences on one line */
  :global(.gutter-multi-dot) {
    display: inline-flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 0.35em;
  }

  :global(.gutter-multi-dot-segment) {
    display: block;
    width: 6px;
    height: 4px;
    border-radius: 3px;
  }

  :global(.gutter-dot--filtered) {
    opacity: 0.3;
  }

  :global(.gutter-dot--filtered .gutter-multi-dot-segment),
  :global(.gutter-dot.gutter-dot--filtered) {
    width: 4px;
    height: 4px;
    border: 1px dashed currentColor;
    background: transparent !important;
  }

  :global(.gutter-collapse) {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 1px;
    opacity: 0.5;
    transform: rotate(45deg);
    margin-top: 0.55em;
  }

  /* Eye icon for preview mode gutter */
  :global(.gutter-eye) {
    display: inline-flex;
    align-items: center;
    margin-top: 0.3em;
    color: var(--muted-foreground);
    opacity: 0.4;
    transition: opacity 0.15s ease, color 0.15s ease;
  }

  :global(.gutter-eye:hover) {
    opacity: 0.8;
  }

  :global(.gutter-eye--active) {
    opacity: 1;
    color: var(--primary);
  }

  :global(.gutter-eye svg) {
    display: block;
  }

  /* ── VisibilityMark (inline) ───────────────────────────────────── */

  /* The mark's own HTML span gets no underline — it can't span code
     gaps. The decoration-based .vis-underline handles all underlines.
     This rule exists only to reset any inherited text-decoration. */
  :global(.vis-mark) {
    text-decoration: none;
  }

  /* Decoration-based underline: spans the full bridged range including
     code gaps. Color comes from --vis-color set as inline style. */
  :global(.vis-underline) {
    background-image: radial-gradient(
      circle,
      color-mix(in oklch, var(--vis-color, var(--muted-foreground)) 30%, transparent) 1px,
      transparent 1.2px
    );
    background-position: left calc(100% - 1px);
    background-repeat: repeat-x;
    background-size: 6px 3px;
    transition: background-image 0.15s ease;
  }

  :global(.vis-underline--hovered) {
    background-image: radial-gradient(
      circle,
      var(--vis-color, var(--muted-foreground)) 1px,
      transparent 1.2px
    );
  }

  /* Brighten underline when the cursor or selection overlaps */
  :global(.vis-underline--selected) {
    background-image: radial-gradient(
      circle,
      var(--vis-color, var(--muted-foreground)) 1px,
      transparent 1.2px
    );
  }

  /* In preview mode, keep the matching directive visible with a softer tint. */
  :global(.vis-underline--preview) {
    background-image: radial-gradient(
      circle,
      color-mix(in oklch, var(--vis-color, var(--muted-foreground)) 75%, transparent) 1px,
      transparent 1.2px
    );
  }

  /* Revealed highlight: persistent (from gutter click) */
  :global(.vis-mark--revealed) {
    background: color-mix(in oklch, var(--vis-hover-color) 18%, transparent) !important;
    box-shadow: inset 0 -1.5px 0 0 color-mix(in oklch, var(--vis-hover-color) 65%, transparent);
  }

  :global(.vis-mark--revealed code),
  :global(.vis-mark--revealed strong),
  :global(.vis-mark--revealed em),
  :global(.vis-mark--revealed a),
  :global(.vis-mark--revealed mark) {
    background: color-mix(in oklch, var(--vis-hover-color) 18%, transparent) !important;
  }

  /* Revealed-included: matching text highlighted in preview mode when
     the eye icon is clicked — softer than the normal revealed style. */
  :global(.vis-mark--revealed-included) {
    background: color-mix(in oklch, var(--vis-hover-color) 12%, transparent) !important;
    box-shadow: inset 0 -1.5px 0 0 color-mix(in oklch, var(--vis-hover-color) 50%, transparent);
  }

  :global(.vis-mark--revealed-included code),
  :global(.vis-mark--revealed-included strong),
  :global(.vis-mark--revealed-included em),
  :global(.vis-mark--revealed-included a),
  :global(.vis-mark--revealed-included mark) {
    background: color-mix(in oklch, var(--vis-hover-color) 12%, transparent) !important;
  }

  /* Gutter dot active state when ranges are revealed */
  :global(.gutter-dot--revealed),
  :global(.gutter-multi-dot.gutter-dot--revealed) {
    transform: scale(1.3);
    filter: brightness(1.2);
    transition: transform 0.1s ease, filter 0.1s ease;
  }

  /* Filter mode: hide non-matching inline content */
  :global(.vis-mark--hidden) {
    font-size: 0;
    line-height: 0;
    overflow: hidden;
    display: inline-block;
    width: 0;
    height: 0;
    opacity: 0;
    pointer-events: none;
  }

  /* Revealed-filtered: gutter click in preview mode shows hidden text
     with strikethrough + muted styling to indicate it's excluded. */
  :global(.vis-mark--revealed-filtered) {
    text-decoration: line-through;
    text-decoration-color: color-mix(in oklch, var(--vis-hover-color) 60%, transparent);
    text-decoration-thickness: 2px;
    opacity: 0.5;
    background: color-mix(in oklch, var(--vis-hover-color) 8%, transparent);
    box-shadow: inset 0 -1.5px 0 0 color-mix(in oklch, var(--vis-hover-color) 35%, transparent);
    transition: opacity 0.15s ease;
  }

  :global(.vis-mark--revealed-filtered code),
  :global(.vis-mark--revealed-filtered strong),
  :global(.vis-mark--revealed-filtered em),
  :global(.vis-mark--revealed-filtered a),
  :global(.vis-mark--revealed-filtered mark) {
    text-decoration: line-through;
    text-decoration-color: inherit;
    background: color-mix(in oklch, var(--vis-hover-color) 8%, transparent) !important;
  }

  /* ── VisibilityBlock ───────────────────────────────────────────── */

  :global(.vis-block-marker-wrapper) {
    user-select: none;
    margin: 2px 0;
  }

  :global(.vis-block-marker-wrapper--hidden) {
    display: none;
  }

  /* Visibility block gutter: colored bar via ::before on each block node */
  :global(.vis-block-gutter-node) {
    position: relative;
  }

  :global(.vis-block-gutter-node::before) {
    content: "";
    position: absolute;
    left: -17px;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--vis-gutter-color, oklch(0.554 0.022 257.417));
    opacity: 0.85;
    pointer-events: none;
  }

  /* Middle/last segments extend up through the margin above them */
  :global(.vis-block-gutter-middle::before),
  :global(.vis-block-gutter-last::before) {
    top: calc(-0.75em - 1px);
  }

  /* Round the caps */
  :global(.vis-block-gutter-first::before),
  :global(.vis-block-gutter-only::before) {
    border-top-left-radius: 3px;
    border-top-right-radius: 3px;
  }

  :global(.vis-block-gutter-last::before),
  :global(.vis-block-gutter-only::before) {
    border-bottom-left-radius: 3px;
    border-bottom-right-radius: 3px;
  }

  /* Filter mode: hide non-matching block content */
  :global(.vis-block-content--hidden) {
    display: none;
  }

  :global(.vis-block--hidden) {
    display: none;
  }

  /* Revealed-filtered block: gutter click in preview mode shows hidden block
     content with strikethrough + muted styling to indicate it's excluded. */
  :global(.vis-block-content--revealed-filtered) {
    text-decoration: line-through;
    text-decoration-color: color-mix(in oklch, var(--vis-gutter-color) 60%, transparent);
    text-decoration-thickness: 2px;
    opacity: 0.5;
    background: color-mix(in oklch, var(--vis-gutter-color) 8%, transparent);
    border-left: 2px solid color-mix(in oklch, var(--vis-gutter-color) 35%, transparent);
    padding-left: 8px;
    transition: opacity 0.15s ease;
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
