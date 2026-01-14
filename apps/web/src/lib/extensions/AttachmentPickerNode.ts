/**
 * TipTap Node extension for inline attachment picker.
 *
 * This extension renders the attachment picker as an inline block node
 * in the editor. When an attachment is selected, the node is replaced
 * with the actual embed content.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import type { Api } from "$lib/backend/api";
import AttachmentPickerNodeView from "../components/AttachmentPickerNodeView.svelte";
import { mount, unmount } from "svelte";

export interface AttachmentPickerNodeOptions {
  entryPath: string;
  api: Api | null;
  onAttachmentSelect: (selection: {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    sourceEntryPath: string;
  }) => void;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    attachmentPickerNode: {
      /** Inserts an attachment picker block node */
      insertAttachmentPicker: () => ReturnType;
    };
  }
}

export const AttachmentPickerNode = Node.create<AttachmentPickerNodeOptions>({
  name: "attachmentPickerNode",

  group: "block",

  atom: true, // Non-editable, treated as single unit

  draggable: false,

  selectable: true,

  addOptions() {
    return {
      entryPath: "",
      api: null,
      onAttachmentSelect: () => {},
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-attachment-picker]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ['div', mergeAttributes(HTMLAttributes, { 'data-attachment-picker': '' })];
  },

  addCommands() {
    return {
      insertAttachmentPicker:
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
      dom.classList.add("attachment-picker-node-wrapper");
      dom.setAttribute("contenteditable", "false");

      // Track Svelte component for cleanup
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

      // Mount the Svelte component
      svelteComponent = mount(AttachmentPickerNodeView, {
        target: dom,
        props: {
          entryPath: this.options.entryPath,
          api: this.options.api,
          onSelect: (selection: {
            path: string;
            isImage: boolean;
            blobUrl?: string;
            sourceEntryPath: string;
          }) => {
            this.options.onAttachmentSelect(selection);
            deleteNode();
          },
          onCancel: () => {
            deleteNode();
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
