/**
 * TipTap Node extension for markdown footnotes.
 *
 * Models footnotes as inline atom nodes storing both `label` and `content`
 * as attributes. Renders as a superscript number in the editor. Click opens
 * a popover to edit the footnote content.
 *
 * Markdown round-trip:
 * - Parse: Pre-process markdown to extract `[^label]: content` definitions,
 *   then inline tokenizer matches `[^label]` references.
 * - Serialize: `renderMarkdown` outputs `[^label]` inline. Post-processing
 *   appends `[^label]: content` definitions at the end.
 */

import { Node } from "@tiptap/core";
import type { Editor } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import FootnoteNodeView from "../components/FootnoteNodeView.svelte";
import { mount, unmount } from "svelte";

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    footnoteRef: {
      /** Insert a footnote with optional content */
      insertFootnote: (content?: string) => ReturnType;
      /** Renumber all footnotes sequentially based on document order */
      reorderFootnotes: () => ReturnType;
    };
  }
}

// Module-level map populated by preprocessFootnotes, read by parseMarkdown
let _footnoteDefinitions = new Map<string, string>();

// Flag: the next mounted footnote node view should auto-open its popover
let _autoOpenNext = false;

export const FootnoteRef = Node.create({
  name: "footnoteRef",

  group: "inline",

  inline: true,

  atom: true,

  addAttributes() {
    return {
      label: { default: "1" },
      content: { default: "" },
    };
  },

  parseHTML() {
    return [{ tag: "sup[data-footnote]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "sup",
      {
        "data-footnote": "",
        "data-label": HTMLAttributes.label,
        class: "footnote-ref",
        title: HTMLAttributes.content || "",
      },
      HTMLAttributes.label,
    ];
  },

  addCommands() {
    return {
      insertFootnote:
        (content?: string) =>
        ({ editor, tr, dispatch }) => {
          // Find next available numeric label
          const existingLabels = new Set<string>();
          editor.state.doc.descendants((node) => {
            if (node.type.name === "footnoteRef") {
              existingLabels.add(node.attrs.label);
            }
          });
          let nextNum = 1;
          while (existingLabels.has(String(nextNum))) {
            nextNum++;
          }

          // If text is selected, use it as footnote content and insert after selection
          const { from, to } = tr.selection;
          const selectedText = tr.doc.textBetween(from, to);
          const footnoteContent = content ?? selectedText ?? "";

          if (dispatch) {
            const footnoteNode = editor.schema.nodes.footnoteRef.create({
              label: String(nextNum),
              content: footnoteContent,
            });
            // Insert at the end of the selection (preserving selected text)
            _autoOpenNext = true;
            tr.insert(to, footnoteNode);
            // Collapse selection to after the footnote so BubbleMenu hides
            const posAfter = to + footnoteNode.nodeSize;
            tr.setSelection(TextSelection.create(tr.doc, posAfter));
            dispatch(tr);
          }
          return true;
        },
      reorderFootnotes:
        () =>
        ({ tr, dispatch }) => {
          // Collect only numeric-labeled footnotes in document order
          const numericFootnotes: { pos: number; attrs: { label: string; content: string } }[] = [];
          tr.doc.descendants((node, pos) => {
            if (node.type.name === "footnoteRef" && /^\d+$/.test(node.attrs.label)) {
              numericFootnotes.push({ pos, attrs: { label: node.attrs.label, content: node.attrs.content } });
            }
          });

          if (numericFootnotes.length === 0) return false;

          if (dispatch) {
            // Renumber 1, 2, 3, ... in document order (skip named labels)
            for (let i = 0; i < numericFootnotes.length; i++) {
              const newLabel = String(i + 1);
              const { pos, attrs } = numericFootnotes[i];
              if (attrs.label !== newLabel) {
                tr.setNodeMarkup(pos, null, { ...attrs, label: newLabel });
              }
            }
            dispatch(tr);
          }
          return true;
        },
    };
  },

  addNodeView() {
    return ({ node, getPos, editor }) => {
      const dom = document.createElement("sup");
      dom.classList.add("footnote-ref");
      dom.setAttribute("data-footnote", "");
      dom.setAttribute("contenteditable", "false");

      let currentLabel = node.attrs.label as string;
      let currentContent = node.attrs.content as string;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      let svelteComponent: Record<string, any> | null = null;

      const onUpdate = (newContent: string, newLabel?: string) => {
        const pos = getPos();
        if (typeof pos !== "number") return;
        const tr = editor.view.state.tr.setNodeMarkup(pos, null, {
          label: newLabel ?? currentLabel,
          content: newContent,
        });
        editor.view.dispatch(tr);
      };

      function mountComponent(label: string, content: string, autoOpen = false) {
        svelteComponent = mount(FootnoteNodeView, {
          target: dom,
          props: {
            label,
            content,
            readonly: !editor.isEditable,
            autoOpen,
            onUpdate,
          },
        });
      }

      // Consume the auto-open flag if set (from insertFootnote command)
      const shouldAutoOpen = _autoOpenNext;
      _autoOpenNext = false;
      mountComponent(currentLabel, currentContent, shouldAutoOpen);

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          if (updatedNode.type.name !== "footnoteRef") return false;
          const newLabel = updatedNode.attrs.label as string;
          const newContent = updatedNode.attrs.content as string;
          if (newLabel !== currentLabel || newContent !== currentContent) {
            currentLabel = newLabel;
            currentContent = newContent;
            if (svelteComponent) {
              unmount(svelteComponent);
            }
            mountComponent(newLabel, newContent);
          }
          return true;
        },
        destroy() {
          if (svelteComponent) {
            unmount(svelteComponent);
            svelteComponent = null;
          }
        },
      };
    };
  },

  // Custom inline tokenizer for [^label]
  // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
  markdownTokenizer: {
    name: "footnoteRef",
    level: "inline",
    start: "[^",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[]) {
      const match = /^\[\^([^\]]+)\]/.exec(src);
      if (!match) return undefined;
      // Don't match definition syntax [^label]:
      if (src[match[0].length] === ":") return undefined;
      return {
        type: "footnoteRef",
        raw: match[0],
        label: match[1],
      };
    },
  },

  // Parse the footnoteRef token into a node
  // @ts-ignore - parseMarkdown is a custom field for @tiptap/markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  parseMarkdown(token: any, helpers: any) {
    const content = _footnoteDefinitions.get(token.label) || "";
    return helpers.createNode("footnoteRef", {
      label: token.label,
      content,
    });
  },
}).extend({
  // Render footnote ref to [^label] in markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    return `[^${node.attrs?.label ?? "1"}]`;
  },
});

/**
 * Pre-process markdown before feeding it to the editor.
 * Extracts `[^label]: content` definitions into the module-level map
 * and strips them from the markdown.
 */
export function preprocessFootnotes(markdown: string): string {
  _footnoteDefinitions = new Map();
  // Match footnote definitions: [^label]: content (rest of line)
  const linesToRemove = new Set<number>();
  const lines = markdown.split("\n");

  // First pass: find definitions
  for (let i = 0; i < lines.length; i++) {
    const lineMatch = /^\[\^([^\]]+)\]:\s*(.+)$/.exec(lines[i]);
    if (lineMatch) {
      _footnoteDefinitions.set(lineMatch[1], lineMatch[2].trim());
      linesToRemove.add(i);
    }
  }

  if (linesToRemove.size === 0) return markdown;

  // Remove definition lines and any trailing blank lines they create
  const result = lines.filter((_, i) => !linesToRemove.has(i));

  // Trim trailing blank lines
  while (result.length > 0 && result[result.length - 1].trim() === "") {
    result.pop();
  }

  return result.join("\n");
}

/**
 * Post-process: traverse editor doc for footnoteRef nodes and append
 * definitions at the end of the markdown string.
 */
export function appendFootnoteDefinitions(editor: Editor): string {
  const markdown = editor.getMarkdown();
  const footnotes: { label: string; content: string }[] = [];
  const seen = new Set<string>();

  editor.state.doc.descendants((node) => {
    if (node.type.name === "footnoteRef" && !seen.has(node.attrs.label)) {
      seen.add(node.attrs.label);
      footnotes.push({
        label: node.attrs.label,
        content: node.attrs.content || "",
      });
    }
  });

  if (footnotes.length === 0) return markdown;

  // Ensure there's a blank line before definitions
  let result = markdown.trimEnd();
  result += "\n\n";
  result += footnotes
    .map((fn) => `[^${fn.label}]: ${fn.content}`)
    .join("\n");

  return result;
}
