/**
 * TipTap Node extension for inline attachment picker.
 *
 * This extension renders the attachment picker as an inline block node
 * in the editor. When an attachment is selected, the node is replaced
 * with the actual embed content.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import type { Api } from "$lib/backend/api";
import AttachmentPickerNodeView from "../components/AttachmentPickerNodeView.svelte";
import { mount, unmount } from "svelte";
import type { AttachmentMediaKind } from "@/models/services/attachmentService";

export interface AttachmentPickerNodeOptions {
  entryPath: string;
  api: Api | null;
  onAttachmentSelect: (selection: {
    path: string;
    kind: AttachmentMediaKind;
    blobUrl?: string;
    filename?: string;
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

      const restoreTextSelectionNear = (anchor: number) => {
        const state = editor.state;
        const boundedAnchor = Math.max(0, Math.min(anchor, state.doc.content.size));

        try {
          const selection = TextSelection.near(state.doc.resolve(boundedAnchor), 1);
          editor.view.dispatch(state.tr.setSelection(selection));
          return;
        } catch {
          // Fall through and create a paragraph if the document no longer has
          // a textblock near the removed picker node.
        }

        const paragraphType = editor.schema.nodes.paragraph;
        if (!paragraphType) return;

        const insertPos = Math.max(0, Math.min(anchor, state.doc.content.size));
        const tr = state.tr.insert(insertPos, paragraphType.create());
        const selection = TextSelection.near(
          tr.doc.resolve(Math.min(insertPos + 1, tr.doc.content.size)),
          1,
        );
        editor.view.dispatch(tr.setSelection(selection));
      };

      const deleteNodeAndThen = (action?: () => void) => {
        const deletedPos = getPos();
        deleteNode();
        if (!action) return;
        // Defer follow-up inserts until the picker atom is gone so inline
        // media embeds are created against the current document state.
        queueMicrotask(() => {
          if (typeof deletedPos === "number") {
            restoreTextSelectionNear(deletedPos);
          }
          action();
        });
      };

      // Mount the Svelte component
      svelteComponent = mount(AttachmentPickerNodeView, {
        target: dom,
        props: {
          entryPath: this.options.entryPath,
          api: this.options.api,
          onSelect: (selection: {
            path: string;
            kind: AttachmentMediaKind;
            blobUrl?: string;
            filename?: string;
            sourceEntryPath: string;
          }) => {
            deleteNodeAndThen(() => {
              this.options.onAttachmentSelect(selection);
            });
          },
          onCancel: () => {
            deleteNodeAndThen(() => editor.commands.focus());
          },
        },
      });

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
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
});
