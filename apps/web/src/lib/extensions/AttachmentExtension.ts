/**
 * TipTap extension for attachment picker integration.
 *
 * This extension adds a command to open the attachment picker dialog.
 * The actual picker UI is handled by the AttachmentPicker component.
 */

import { Extension } from "@tiptap/core";

export interface AttachmentExtensionOptions {
  /** Callback to open the attachment picker dialog */
  onOpenPicker?: () => void;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    attachmentPicker: {
      /** Opens the attachment picker dialog */
      openAttachmentPicker: () => ReturnType;
    };
  }
}

export const AttachmentExtension = Extension.create<AttachmentExtensionOptions>(
  {
    name: "attachmentPicker",

    addOptions() {
      return {
        onOpenPicker: undefined,
      };
    },

    addCommands() {
      return {
        openAttachmentPicker:
          () =>
          () => {
            this.options.onOpenPicker?.();
            return true;
          },
      };
    },
  }
);
