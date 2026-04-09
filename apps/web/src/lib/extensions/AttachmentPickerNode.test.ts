import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import { Markdown } from "@tiptap/markdown";
import { afterEach, describe, expect, it, vi } from "vitest";

const { mountSpy, unmountSpy } = vi.hoisted(() => ({
  mountSpy: vi.fn(() => ({})),
  unmountSpy: vi.fn(),
}));

vi.mock("svelte", async () => {
  const actual = await vi.importActual<typeof import("svelte")>("svelte");
  return {
    ...actual,
    mount: mountSpy,
    unmount: unmountSpy,
  };
});

import { AttachmentPickerNode } from "./AttachmentPickerNode";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];

afterEach(() => {
  while (editors.length > 0) {
    editors.pop()?.destroy();
  }
  while (elements.length > 0) {
    elements.pop()?.remove();
  }
  mountSpy.mockClear();
  unmountSpy.mockClear();
});

describe("AttachmentPickerNode", () => {
  it("removes the picker node before running the attachment insert callback", async () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const onAttachmentSelect = vi.fn(() => {
      expect(editor.getJSON().content ?? []).toEqual([{ type: "paragraph" }]);

      editor.commands.insertContent({
        type: "paragraph",
        content: [{ type: "text", text: "attached" }],
      });
    });

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        AttachmentPickerNode.configure({
          entryPath: "entry.md",
          api: null,
          onAttachmentSelect,
        }),
      ],
      content: {
        type: "doc",
        content: [
          { type: "attachmentPickerNode" },
          { type: "paragraph" },
        ],
      },
    });

    editors.push(editor);

    const firstMountCall = mountSpy.mock.calls[0] as unknown[] | undefined;
    const mountOptions = firstMountCall?.[1] as
      | { props?: { onSelect?: (selection: {
        path: string;
        kind: "image" | "video" | "audio" | "file";
        blobUrl?: string;
        sourceEntryPath: string;
      }) => void } }
      | undefined;
    const onSelect = mountOptions?.props?.onSelect;

    expect(typeof onSelect).toBe("function");

    onSelect?.({
      path: "./_attachments/widget.html",
      kind: "file",
      sourceEntryPath: "entry.md",
    });

    expect(onAttachmentSelect).not.toHaveBeenCalled();

    await Promise.resolve();

    expect(onAttachmentSelect).toHaveBeenCalledTimes(1);
    expect(editor.getJSON().content ?? []).toEqual([
      {
        type: "paragraph",
        content: [{ type: "text", text: "attached" }],
      },
    ]);
  });

  it("keeps the document schema-valid when HTML attachments use the picker insert callback", async () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    let editor!: Editor;
    editor = new Editor({
      element,
      extensions: [
        StarterKit,
        Image.configure({
          inline: true,
          allowBase64: true,
        }),
        AttachmentPickerNode.configure({
          entryPath: "entry.md",
          api: null,
          onAttachmentSelect: ({ path }) => {
            editor.commands.setImage({
              src: path,
              alt: path.split("/").pop() || path,
            });
          },
        }),
      ],
      content: {
        type: "doc",
        content: [
          { type: "attachmentPickerNode" },
          { type: "paragraph" },
        ],
      },
    });

    editors.push(editor);

    const firstMountCall = mountSpy.mock.calls[0] as unknown[] | undefined;
    const mountOptions = firstMountCall?.[1] as
      | { props?: { onSelect?: (selection: {
        path: string;
        kind: "image" | "video" | "audio" | "file";
        blobUrl?: string;
        sourceEntryPath: string;
      }) => void } }
      | undefined;
    const onSelect = mountOptions?.props?.onSelect;

    expect(typeof onSelect).toBe("function");

    onSelect?.({
      path: "./_attachments/widget.html",
      kind: "file",
      sourceEntryPath: "entry.md",
    });

    await Promise.resolve();

    expect(() => editor.state.doc.check()).not.toThrow();
    expect(editor.getJSON().content ?? []).toEqual([
      {
        type: "paragraph",
        content: [
          {
            type: "image",
            attrs: {
              src: "./_attachments/widget.html",
              alt: "widget.html",
              title: null,
              width: null,
              height: null,
            },
          },
        ],
      },
    ]);
  });

  it("restores a text selection before running the live focus-plus-image insert path", async () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    let editor!: Editor;
    editor = new Editor({
      element,
      extensions: [
        StarterKit,
        Image.configure({
          inline: true,
          allowBase64: true,
        }),
        AttachmentPickerNode.configure({
          entryPath: "entry.md",
          api: null,
          onAttachmentSelect: ({ path }) => {
            expect(editor.state.selection.$from.parent.type.name).toBe("paragraph");

            editor.chain().focus().setImage({
              src: path,
              alt: path.split("/").pop() || path,
            }).run();
          },
        }),
      ],
      content: {
        type: "doc",
        content: [{ type: "attachmentPickerNode" }],
      },
    });

    editors.push(editor);

    const firstMountCall = mountSpy.mock.calls[0] as unknown[] | undefined;
    const mountOptions = firstMountCall?.[1] as
      | { props?: { onSelect?: (selection: {
        path: string;
        kind: "image" | "video" | "audio" | "file";
        blobUrl?: string;
        sourceEntryPath: string;
      }) => void } }
      | undefined;
    const onSelect = mountOptions?.props?.onSelect;

    expect(typeof onSelect).toBe("function");

    onSelect?.({
      path: "./_attachments/widget.html",
      kind: "file",
      sourceEntryPath: "entry.md",
    });

    await Promise.resolve();

    expect(() => editor.state.doc.check()).not.toThrow();
    expect(editor.getJSON().content ?? []).toEqual([
      {
        type: "paragraph",
        content: [
          {
            type: "image",
            attrs: {
              src: "./_attachments/widget.html",
              alt: "widget.html",
              title: null,
              width: null,
              height: null,
            },
          },
        ],
      },
    ]);
  });

  it("stays schema-valid if external markdown content sync lands before the deferred HTML insert", async () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    let editor!: Editor;
    editor = new Editor({
      element,
      extensions: [
        StarterKit,
        Markdown.configure({ markedOptions: { gfm: true } }),
        Image.configure({
          inline: true,
          allowBase64: true,
        }),
        AttachmentPickerNode.configure({
          entryPath: "entry.md",
          api: null,
          onAttachmentSelect: ({ path }) => {
            editor.commands.setImage({
              src: path,
              alt: path.split("/").pop() || path,
            });
          },
        }),
      ],
      content: {
        type: "doc",
        content: [
          { type: "attachmentPickerNode" },
          { type: "paragraph" },
        ],
      },
    });

    editors.push(editor);

    const firstMountCall = mountSpy.mock.calls[0] as unknown[] | undefined;
    const mountOptions = firstMountCall?.[1] as
      | { props?: { onSelect?: (selection: {
        path: string;
        kind: "image" | "video" | "audio" | "file";
        blobUrl?: string;
        sourceEntryPath: string;
      }) => void } }
      | undefined;
    const onSelect = mountOptions?.props?.onSelect;

    expect(typeof onSelect).toBe("function");

    onSelect?.({
      path: "./_attachments/widget.html",
      kind: "file",
      sourceEntryPath: "entry.md",
    });

    editor.commands.setContent("", { contentType: "markdown" });

    await Promise.resolve();

    expect(() => editor.state.doc.check()).not.toThrow();
  });
});
