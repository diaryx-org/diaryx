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
  import { ColoredHighlightMark } from "./extensions/ColoredHighlightMark";
  import Typography from "@tiptap/extension-typography";
  import Image from "@tiptap/extension-image";
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
  // Custom extension for Discord-style spoiler syntax
  import { SpoilerMark } from "./extensions/SpoilerMark";
  // Custom extension for raw HTML blocks
  import { HtmlBlock } from "./extensions/HtmlBlock";
  // Custom extension for inline drawing blocks
  import { DrawingBlock } from "./extensions/DrawingBlock";
  // Custom extension for Notion-style inline table controls
  import { TableControls } from "./extensions/TableControls";
  // Custom extension for markdown footnotes
  import { FootnoteRef, preprocessFootnotes, appendFootnoteDefinitions } from "./extensions/FootnoteRef";
  import type { Api } from "$lib/backend/api";

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: (markdown: string) => void;
    onblur?: () => void;
    readonly?: boolean;
    onFileDrop?: (
      file: File,
    ) => Promise<{ blobUrl: string; attachmentPath: string } | null>;
    // Debug mode for menus (logs shouldShow decisions to console)
    debugMenus?: boolean;
    // Callback when a link is clicked (for handling relative links to other notes)
    onLinkClick?: (href: string) => void;
    // Attachment picker options
    entryPath?: string;
    api?: Api | null;
    onAttachmentInsert?: (selection: {
      path: string;
      isImage: boolean;
      blobUrl?: string;
      sourceEntryPath: string;
    }) => void;
    // Formatting options
    enableSpoilers?: boolean;
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
    enableSpoilers = true,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);

  // FloatingMenu element ref - must exist before editor creation
  let floatingMenuElement: HTMLDivElement | undefined = $state();
  // FloatingMenu component ref - for programmatic expansion
  let floatingMenuRef: { expand: () => void } | undefined = $state();
  // BubbleMenu element ref - must exist before editor creation
  let bubbleMenuElement: HTMLDivElement | undefined = $state();
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Track the last content prop value we synced FROM, so we only sync when it actually changes
  // This prevents resetting editor content when the user is typing and the prop hasn't changed
  let lastSyncedContent: string | undefined = undefined;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
  let lastReadonly: boolean | null = null;
  let lastPlaceholder: string | null = null;
  let lastEnableSpoilers: boolean | null = null;

  function destroyEditor() {
    editor?.destroy();
    editor = null;
  }

  function createEditor() {
    destroyEditor();

    // In non-readonly mode, require FloatingMenu element
    if (!readonly && !floatingMenuElement) {
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
      // Always load SpoilerMark to ensure consistent parsing (tokenizer stays registered in marked.js)
      // Pass enabled option to control visual behavior
      SpoilerMark.configure({ enabled: enableSpoilers }),
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
      ColoredHighlightMark,
      Typography,
      Image.configure({
        inline: true,
        allowBase64: true,
        HTMLAttributes: {
          class: "editor-image",
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
      // Footnote extension
      FootnoteRef,
      // Raw HTML block extension
      HtmlBlock.configure({
        entryPath,
        api,
      }),
      // Inline drawing block extension
      DrawingBlock.configure({
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
    ];

    // Add FloatingMenu extension (for block formatting on empty lines)
    if (!readonly) {
      extensions.push(
        FloatingMenu.configure({
          element: floatingMenuElement,
          appendTo: () => document.body,
          options: {
            strategy: "fixed",
            placement: "left-start",
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
      // Only show on desktop - mobile uses the bottom InlineToolbar
      if (bubbleMenuElement) {
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

    editor = new Editor({
      element,
      extensions,
      content: preprocessFootnotes(content),
      contentType: "markdown",
      editable: !readonly,
      onCreate: () => {
        // Track the initial content so we don't reset it on the first effect run
        lastSyncedContent = content;
      },
      onUpdate: ({ editor }) => {
        if (onchange && !isUpdatingContent) {
          const markdown = appendFootnoteDefinitions(editor);
          onchange(markdown);
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

  export function getMarkdown(): string | undefined {
    if (!editor) return undefined;
    return appendFootnoteDefinitions(editor);
  }

  /**
   * Set content from markdown
   */
  export function setContent(markdown: string): void {
    if (!editor) return;
    editor.commands.setContent(preprocessFootnotes(markdown), { contentType: "markdown" });
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
        lastEnableSpoilers = enableSpoilers;
      }
      return;
    }

    // In edit mode, wait for menu elements
    if (!editorInitialized && hasEditorElement && hasFloatingMenu && hasBubbleMenu) {
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
      lastEnableSpoilers = enableSpoilers;
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

  // Rebuild editor when readonly, placeholder, or enableSpoilers changes
  $effect(() => {
    if (!element) return;
    // Skip if we haven't done initial creation yet
    if (!editorInitialized) return;

    const needsRebuild =
      readonly !== lastReadonly ||
      placeholder !== lastPlaceholder ||
      enableSpoilers !== lastEnableSpoilers;

    if (!needsRebuild) return;

    // Update tracking for what we're about to build
    lastReadonly = readonly;
    lastPlaceholder = placeholder;
    lastEnableSpoilers = enableSpoilers;

    createEditor();
  });

  // Update editor content when the content prop changes (e.g., switching files)
  // Only sync when the content PROP has actually changed from what we last synced
  // This prevents resetting user's typing when the prop hasn't changed
  $effect(() => {
    if (!editor) return;
    if (content === undefined) return;

    // Only sync if the content prop has actually changed from what we last synced
    // This prevents resetting the editor when the user is typing (prop stays the same,
    // but editor content changes)
    if (content === lastSyncedContent) return;

    // Content prop changed - sync it to the editor
    lastSyncedContent = content;
    isUpdatingContent = true;
    editor.commands.setContent(preprocessFootnotes(content), { contentType: "markdown" });
    setTimeout(() => {
      isUpdatingContent = false;
    }, 0);
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
      // Only insert into editor if it's an image with a blob URL
      if (result && result.blobUrl && editor && file.type.startsWith("image/")) {
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
{#if !readonly}
  <FloatingMenuComponent
    bind:this={floatingMenuRef}
    {editor}
    bind:element={floatingMenuElement}
  />
{/if}

<!-- BubbleMenu for inline formatting (appears when text is selected) -->
{#if !readonly}
  <BubbleMenuComponent {editor} bind:element={bubbleMenuElement} {enableSpoilers} {entryPath} {api} />
{/if}

<style global>
  :global(.editor-content) {
    outline: none;
    min-height: 100%;
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
    line-height: 1.6;
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

  /* Footnote ref styles */
  :global(.footnote-ref) {
    font-size: 0.75em;
    vertical-align: super;
    color: var(--primary);
    cursor: pointer;
    font-weight: 600;
    line-height: 1;
  }

  :global(.footnote-ref:hover) {
    opacity: 0.8;
  }

  /* Spoiler mark styles */
  :global(.spoiler-mark) {
    border-radius: 4px;
    padding: 0 2px;
    transition: all 0.2s ease;
  }

  :global(.spoiler-hidden) {
    background: var(--foreground);
    color: transparent;
    user-select: none;
    cursor: pointer;
  }

  :global(.spoiler-revealed) {
    background: var(--muted);
    color: var(--foreground);
    cursor: pointer;
  }

  /* When spoilers are disabled, show || around the text */
  :global(.spoiler-disabled)::before {
    content: "||";
    opacity: 0.5;
  }

  :global(.spoiler-disabled)::after {
    content: "||";
    opacity: 0.5;
  }

  :global(.spoiler-hidden:hover) {
    opacity: 0.8;
  }

  /* Table styles */
  :global(.editor-content table) {
    border-collapse: collapse;
    width: 100%;
    margin: 0.75em 0;
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

  /* Colored highlight mark styles */
  :global(.highlight-mark) {
    border-radius: 2px;
    padding: 0 2px;
  }

  /* Light mode highlight colors */
  :global(.highlight-red) {
    background: oklch(0.92 0.12 25);
  }

  :global(.highlight-orange) {
    background: oklch(0.93 0.1 60);
  }

  :global(.highlight-yellow) {
    background: oklch(0.95 0.12 95);
  }

  :global(.highlight-green) {
    background: oklch(0.92 0.08 145);
  }

  :global(.highlight-cyan) {
    background: oklch(0.92 0.08 195);
  }

  :global(.highlight-blue) {
    background: oklch(0.88 0.1 250);
  }

  :global(.highlight-violet) {
    background: oklch(0.9 0.1 300);
  }

  :global(.highlight-pink) {
    background: oklch(0.93 0.1 350);
  }

  :global(.highlight-brown) {
    background: oklch(0.88 0.06 60);
  }

  :global(.highlight-grey) {
    background: oklch(0.9 0 0);
  }

  /* Dark mode highlight colors */
  :global(.dark .highlight-red) {
    background: oklch(0.35 0.12 25);
  }

  :global(.dark .highlight-orange) {
    background: oklch(0.38 0.1 60);
  }

  :global(.dark .highlight-yellow) {
    background: oklch(0.42 0.12 95);
  }

  :global(.dark .highlight-green) {
    background: oklch(0.38 0.08 145);
  }

  :global(.dark .highlight-cyan) {
    background: oklch(0.38 0.08 195);
  }

  :global(.dark .highlight-blue) {
    background: oklch(0.35 0.1 250);
  }

  :global(.dark .highlight-violet) {
    background: oklch(0.38 0.1 300);
  }

  :global(.dark .highlight-pink) {
    background: oklch(0.4 0.1 350);
  }

  :global(.dark .highlight-brown) {
    background: oklch(0.38 0.06 60);
  }

  :global(.dark .highlight-grey) {
    background: oklch(0.4 0 0);
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
