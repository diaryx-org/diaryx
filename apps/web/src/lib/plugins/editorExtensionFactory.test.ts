import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";

import {
  createMarkFromManifest,
  type EditorExtensionManifest,
} from "./editorExtensionFactory";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];

afterEach(() => {
  while (editors.length > 0) {
    editors.pop()?.destroy();
  }
  while (elements.length > 0) {
    elements.pop()?.remove();
  }
});

const spoilerManifest: EditorExtensionManifest = {
  slot: "EditorExtension",
  extension_id: "spoiler",
  node_type: "InlineMark",
  markdown: {
    level: "Inline",
    open: "||",
    close: "||",
  },
  render_export: null,
  edit_mode: null,
  css: `
    .spoiler-mark { border-radius: 4px; }
    .spoiler-hidden { color: transparent; }
    .spoiler-revealed { color: inherit; }
  `,
  click_behavior: {
    ToggleClass: {
      hidden_class: "spoiler-hidden",
      revealed_class: "spoiler-revealed",
    },
  },
  insert_command: null,
  keyboard_shortcut: null,
};

describe("editorExtensionFactory", () => {
  it("toggles spoiler marks on pointer press", () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        createMarkFromManifest(spoilerManifest),
      ],
      content: {
        type: "doc",
        content: [
          {
            type: "paragraph",
            content: [
              {
                type: "text",
                text: "secret",
                marks: [{ type: "spoiler" }],
              },
            ],
          },
        ],
      },
    });

    editors.push(editor);

    const markEl = element.querySelector("[data-spoiler]") as HTMLElement | null;

    expect(markEl).not.toBeNull();
    expect(markEl?.classList.contains("spoiler-hidden")).toBe(true);

    markEl?.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
    );

    expect(markEl?.classList.contains("spoiler-hidden")).toBe(false);
    expect(markEl?.classList.contains("spoiler-revealed")).toBe(true);

    markEl?.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
    );

    expect(markEl?.classList.contains("spoiler-hidden")).toBe(true);
    expect(markEl?.classList.contains("spoiler-revealed")).toBe(false);
  });
});
