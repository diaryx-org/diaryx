/**
 * TipTap Node extension for raw HTML blocks.
 *
 * Renders raw HTML blocks as a preview in the editor with a toggle to view/edit
 * the source. Round-trips correctly through the markdown pipeline: markdown →
 * editor → markdown preserves the original HTML verbatim.
 *
 * Only captures block-level HTML (starting at the beginning of a line).
 * Inline HTML (e.g., <span> inside a paragraph) is not handled.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import type { Api } from "$lib/backend/api";
import HtmlBlockNodeView from "../components/HtmlBlockNodeView.svelte";
import { mount, unmount } from "svelte";

// HTML void elements that don't have closing tags
const VOID_ELEMENTS = new Set([
  "area",
  "base",
  "br",
  "col",
  "embed",
  "hr",
  "img",
  "input",
  "link",
  "meta",
  "param",
  "source",
  "track",
  "wbr",
]);

export interface HtmlBlockOptions {
  entryPath: string;
  api: Api | null;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    htmlBlock: {
      /** Insert an HTML block with optional content */
      insertHtmlBlock: (content?: string) => ReturnType;
    };
  }
}

export const HtmlBlock = Node.create<HtmlBlockOptions>({
  name: "htmlBlock",

  group: "block",

  atom: true,

  draggable: true,

  selectable: true,

  addOptions() {
    return {
      entryPath: "",
      api: null,
    };
  },

  addAttributes() {
    return {
      content: {
        default: "",
      },
    };
  },

  parseHTML() {
    return [{ tag: "div[data-html-block]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes(HTMLAttributes, { "data-html-block": "" }),
      0,
    ];
  },

  addCommands() {
    return {
      insertHtmlBlock:
        (content?: string) =>
        ({ commands }) => {
          return commands.insertContent({
            type: this.name,
            attrs: { content: content ?? "" },
          });
        },
    };
  },

  addNodeView() {
    const { entryPath, api } = this.options;

    return ({ node, getPos, editor }) => {
      const dom = document.createElement("div");
      dom.classList.add("html-block-node-wrapper");
      dom.setAttribute("contenteditable", "false");

      let currentContent = node.attrs.content as string;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      let svelteComponent: Record<string, any> | null = null;

      const onUpdate = (newContent: string) => {
        const pos = getPos();
        if (typeof pos !== "number") return;
        const tr = editor.view.state.tr.setNodeMarkup(pos, null, {
          content: newContent,
        });
        editor.view.dispatch(tr);
      };

      function mountComponent(content: string) {
        svelteComponent = mount(HtmlBlockNodeView, {
          target: dom,
          props: {
            content,
            readonly: !editor.isEditable,
            entryPath,
            api,
            onUpdate,
          },
        });
      }

      mountComponent(currentContent);

      return {
        dom,
        // Let events through to the node view DOM (textarea, buttons) instead
        // of having ProseMirror intercept them
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          if (updatedNode.type.name !== "htmlBlock") return false;
          const newContent = updatedNode.attrs.content as string;
          if (newContent !== currentContent) {
            currentContent = newContent;
            if (svelteComponent) {
              unmount(svelteComponent);
            }
            mountComponent(newContent);
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

  // Custom block-level tokenizer for marked.js
  markdownTokenizer: {
    name: "htmlBlock",
    level: "block",
    start(src: string) {
      const match = src.match(/^<[a-zA-Z][a-zA-Z0-9]*/m);
      return match ? match.index! : -1;
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[]) {
      // Must start with an HTML tag at the beginning of the line
      const openTagMatch = /^<([a-zA-Z][a-zA-Z0-9]*)(\s[^>]*)?\/?>/
        .exec(src);
      if (!openTagMatch) return undefined;

      const tagName = openTagMatch[1].toLowerCase();
      const fullOpenTag = openTagMatch[0];

      // Self-closing tag (e.g., <br />, <hr />, <img ... />)
      if (fullOpenTag.endsWith("/>")) {
        return {
          type: "htmlBlock",
          raw: fullOpenTag,
          content: fullOpenTag.trim(),
        };
      }

      // Void element (e.g., <br>, <hr>, <img>)
      if (VOID_ELEMENTS.has(tagName)) {
        return {
          type: "htmlBlock",
          raw: fullOpenTag,
          content: fullOpenTag.trim(),
        };
      }

      // Paired tag — count depth to find matching close
      let depth = 0;
      let pos = 0;
      // Regex to find opening and closing tags of the same name
      // Using case-insensitive match for tag names
      const tagPattern = new RegExp(
        `<(/?)${tagName}(?:\\s[^>]*)?>`,
        "gi",
      );

      let match: RegExpExecArray | null;
      while ((match = tagPattern.exec(src)) !== null) {
        if (match[1] === "/") {
          // Closing tag
          depth--;
          if (depth === 0) {
            pos = match.index + match[0].length;
            break;
          }
        } else {
          // Opening tag — but skip if self-closing
          if (!match[0].endsWith("/>")) {
            depth++;
          }
        }
      }

      // If we didn't find a matching close tag, don't tokenize
      if (depth !== 0 || pos === 0) return undefined;

      const raw = src.slice(0, pos);
      return {
        type: "htmlBlock",
        raw,
        content: raw.trim(),
      };
    },
  },

  // Parse the htmlBlock token into a node
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  parseMarkdown(token: any, helpers: any) {
    return helpers.createNode("htmlBlock", {
      content: token.content || token.raw,
    });
  },
}).extend({
  // Render HTML block nodes back to raw HTML in markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    return (node.attrs?.content ?? "") + "\n";
  },
});
