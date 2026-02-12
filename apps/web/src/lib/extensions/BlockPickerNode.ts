/**
 * TipTap Node extension for inline block picker.
 *
 * This extension renders the block formatting picker as an inline block node
 * in the editor. When a block type is selected, the node is replaced with the
 * chosen block. This ensures the picker scrolls naturally with the document
 * content, unlike the floating menu approach.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import BlockPickerNodeView from "../components/BlockPickerNodeView.svelte";
import { mount, unmount } from "svelte";

export interface BlockPickerNodeOptions {
  onInsertAttachment?: () => void;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    blockPickerNode: {
      /** Inserts a block picker node */
      insertBlockPicker: () => ReturnType;
    };
  }
}

export const BlockPickerNode = Node.create<BlockPickerNodeOptions>({
  name: "blockPickerNode",

  group: "block",

  atom: true,

  draggable: false,

  selectable: true,

  addOptions() {
    return {
      onInsertAttachment: undefined,
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-block-picker]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ['div', mergeAttributes(HTMLAttributes, { 'data-block-picker': '' })];
  },

  addCommands() {
    return {
      insertBlockPicker:
        () =>
        ({ commands }) => {
          return commands.insertContent({
            type: this.name,
          });
        },
    };
  },

  addNodeView() {
    return ({ node, getPos, editor }) => {
      const dom = document.createElement("div");
      dom.classList.add("block-picker-node-wrapper");
      dom.setAttribute("contenteditable", "false");

      let svelteComponent: Record<string, unknown> | null = null;

      const deleteNode = () => {
        const pos = getPos();
        if (typeof pos === "number") {
          editor.commands.deleteRange({
            from: pos,
            to: pos + node.nodeSize,
          });
        }
      };

      const onInsertAttachment = this.options.onInsertAttachment;

      svelteComponent = mount(BlockPickerNodeView, {
        target: dom,
        props: {
          editor,
          showAttachment: !!onInsertAttachment,
          onSelect: (action: () => void) => {
            deleteNode();
            action();
          },
          onInsertAttachment: onInsertAttachment
            ? () => {
                deleteNode();
                onInsertAttachment();
              }
            : undefined,
          onCancel: () => {
            deleteNode();
            editor.commands.focus();
          },
        },
      });

      return {
        dom,
        destroy() {
          if (svelteComponent) {
            unmount(svelteComponent);
            svelteComponent = null;
          }
        },
      };
    };
  },
});
