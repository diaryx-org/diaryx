import { Editor } from "@tiptap/core";
import { NodeSelection } from "@tiptap/pm/state";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("svelte", async () => {
  const actual = await vi.importActual<typeof import("svelte")>("svelte");
  return {
    ...actual,
    mount: vi.fn(() => ({})),
    unmount: vi.fn(),
  };
});

import { ConditionalBlock } from "./ConditionalBlock";

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

describe("ConditionalBlock", () => {
  it("inserts a block after a top-level node selection", () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        ConditionalBlock.configure({ enabled: false }),
      ],
      content: {
        type: "doc",
        content: [
          {
            type: "conditionalMarker",
            attrs: {
              variant: "open",
              helperType: "if",
              condition: "draft",
            },
          },
          { type: "paragraph" },
        ],
      },
    });

    editors.push(editor);

    editor.view.dispatch(
      editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 0)),
    );

    expect(
      editor.commands.insertConditionalBlock({
        helperType: "for-audience",
        condition: "public",
      }),
    ).toBe(true);

    const content = editor.getJSON().content ?? [];
    expect(content).toEqual([
      {
        type: "conditionalMarker",
        attrs: {
          variant: "open",
          helperType: "if",
          condition: "draft",
        },
      },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "open",
          helperType: "for-audience",
          condition: "public",
        },
      },
      { type: "paragraph" },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "else",
          helperType: "",
          condition: "",
        },
      },
      { type: "paragraph" },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "close",
          helperType: "for-audience",
          condition: "",
        },
      },
      { type: "paragraph" },
    ]);

    expect(editor.state.selection.$from.parent.type.name).toBe("paragraph");
  });
});
