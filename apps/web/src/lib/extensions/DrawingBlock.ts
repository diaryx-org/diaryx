/**
 * TipTap Node extension for inline drawing blocks.
 *
 * Drawings are stored as SVG attachment files in `_attachments/`.
 * In markdown they use standard image syntax with a `drawing:` prefix in the
 * alt text:
 *
 *   ![drawing:My Sketch](_attachments/drawing-abc123.svg)
 *
 * The SVG contains both rendered paths (viewable anywhere) and a `<metadata>`
 * element with raw stroke data for re-editing in Diaryx.
 *
 * The block tokenizer intercepts these before the Image extension so they
 * render with the drawing node view (edit overlay, etc.) instead of a plain
 * `<img>` tag.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import type { Api } from "$lib/backend/api";
import DrawingBlockNodeView from "../components/DrawingBlockNodeView.svelte";
import { mount, unmount } from "svelte";

export interface DrawingBlockOptions {
  entryPath: string;
  api: Api | null;
  /** Called after the drawing SVG has been saved as an attachment. */
  onDrawingSave?: (result: {
    blobUrl: string;
    attachmentPath: string;
  }) => void;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    drawingBlock: {
      /** Insert a new (blank) drawing block. */
      insertDrawingBlock: () => ReturnType;
    };
  }
}

export const DrawingBlock = Node.create<DrawingBlockOptions>({
  name: "drawingBlock",

  group: "block",

  atom: true,

  draggable: true,

  selectable: true,

  addOptions() {
    return {
      entryPath: "",
      api: null,
      onDrawingSave: undefined,
    };
  },

  addAttributes() {
    return {
      src: { default: "" },
      alt: { default: "" },
      width: { default: 600 },
      height: { default: 300 },
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-drawing-block]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes(HTMLAttributes, { "data-drawing-block": "" }),
    ];
  },

  addCommands() {
    return {
      insertDrawingBlock:
        () =>
        ({ commands }) => {
          return commands.insertContent({
            type: this.name,
            attrs: { src: "", alt: "", width: 600, height: 300 },
          });
        },
    };
  },

  addNodeView() {
    const { entryPath, api, onDrawingSave } = this.options;

    return ({ node, getPos, editor }) => {
      const dom = document.createElement("div");
      dom.classList.add("drawing-block-node-wrapper");
      dom.setAttribute("contenteditable", "false");

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      let svelteComponent: Record<string, any> | null = null;

      const updateAttrs = (attrs: Record<string, unknown>) => {
        const pos = getPos();
        if (typeof pos !== "number") return;
        const tr = editor.view.state.tr.setNodeMarkup(pos, null, {
          ...node.attrs,
          ...attrs,
        });
        editor.view.dispatch(tr);
      };

      function mountComponent(attrs: typeof node.attrs) {
        svelteComponent = mount(DrawingBlockNodeView, {
          target: dom,
          props: {
            src: attrs.src as string,
            alt: attrs.alt as string,
            width: attrs.width as number,
            height: attrs.height as number,
            readonly: !editor.isEditable,
            entryPath,
            api,
            onUpdate: (newAttrs: Record<string, unknown>) => {
              updateAttrs(newAttrs);
            },
            onDrawingSave,
          },
        });
      }

      let currentSrc = node.attrs.src as string;
      mountComponent(node.attrs);

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          if (updatedNode.type.name !== "drawingBlock") return false;
          const newSrc = updatedNode.attrs.src as string;
          if (newSrc !== currentSrc) {
            currentSrc = newSrc;
            if (svelteComponent) {
              unmount(svelteComponent);
            }
            mountComponent(updatedNode.attrs);
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

  // Block-level tokenizer: catches ![drawing:...](...) at start of line
  // before the Image extension's inline tokenizer.
  markdownTokenizer: {
    name: "drawingBlock",
    level: "block",
    start(src: string) {
      const match = src.match(/^!\[drawing:/m);
      return match ? match.index! : -1;
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[]) {
      // Match ![drawing:alt](src) at start of line
      // Supports optional angle-bracket-wrapped paths for spaces
      const match = /^!\[drawing:([^\]]*)\]\((?:<([^>]+)>|([^)]+))\)/.exec(src);
      if (!match) return undefined;

      const alt = match[1];
      const imageSrc = (match[2] || match[3]).trim();

      return {
        type: "drawingBlock",
        raw: match[0],
        alt,
        src: imageSrc,
      };
    },
  },

  // Parse the token into a drawingBlock node
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  parseMarkdown(token: any, helpers: any) {
    return helpers.createNode("drawingBlock", {
      src: token.src || "",
      alt: token.alt || "",
    });
  },
}).extend({
  // Render back to markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    const alt = node.attrs?.alt ?? "";
    const src = node.attrs?.src ?? "";
    // Wrap src in angle brackets if it contains spaces
    const formattedSrc = src.includes(" ") ? `<${src}>` : src;
    return `![drawing:${alt}](${formattedSrc})\n`;
  },
});
