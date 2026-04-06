import { Editor } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";

import {
  VisibilityBlock,
  canWrapSelectionInVisibilityBlock,
} from "./VisibilityBlock";

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

function createEditor(content: Record<string, unknown>): Editor {
  const element = document.createElement("div");
  document.body.appendChild(element);
  elements.push(element);

  const editor = new Editor({
    element,
    extensions: [StarterKit, VisibilityBlock.configure({ enabled: false })],
    content,
  });

  editors.push(editor);
  return editor;
}

function paragraphTextRange(editor: Editor, index: number): { from: number; to: number } {
  const ranges: { from: number; to: number }[] = [];
  editor.state.doc.descendants((node, pos) => {
    if (node.type.name !== "paragraph") return;
    ranges.push({
      from: pos + 1,
      to: pos + node.nodeSize - 1,
    });
  });
  return ranges[index]!;
}

describe("VisibilityBlock", () => {
  it("wraps a full paragraph selection in one visibility block", () => {
    const editor = createEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "text", text: "First paragraph" }],
        },
        {
          type: "paragraph",
          content: [{ type: "text", text: "Second paragraph" }],
        },
      ],
    });

    const selection = paragraphTextRange(editor, 0);
    editor.view.dispatch(
      editor.state.tr.setSelection(
        TextSelection.create(editor.state.doc, selection.from, selection.to),
      ),
    );

    expect(canWrapSelectionInVisibilityBlock(editor.state)).toBe(true);
    expect(
      editor.commands.wrapVisibilityBlock({ audiences: ["family"] }),
    ).toBe(true);

    expect(editor.getJSON().content).toEqual([
      {
        type: "visBlockMarker",
        attrs: { variant: "open", audiences: ["family"] },
      },
      {
        type: "paragraph",
        content: [{ type: "text", text: "First paragraph" }],
      },
      {
        type: "visBlockMarker",
        attrs: { variant: "close", audiences: [] },
      },
      {
        type: "paragraph",
        content: [{ type: "text", text: "Second paragraph" }],
      },
    ]);
  });

  it("updates the enclosing block audiences from a partial text selection", () => {
    const editor = createEditor({
      type: "doc",
      content: [
        {
          type: "visBlockMarker",
          attrs: { variant: "open", audiences: ["family"] },
        },
        {
          type: "paragraph",
          content: [{ type: "text", text: "Wrapped paragraph" }],
        },
        {
          type: "visBlockMarker",
          attrs: { variant: "close", audiences: [] },
        },
      ],
    });

    const paragraph = paragraphTextRange(editor, 0);
    editor.view.dispatch(
      editor.state.tr.setSelection(
        TextSelection.create(editor.state.doc, paragraph.from, paragraph.from + 4),
      ),
    );

    expect(
      editor.commands.setVisibilityBlock({ audiences: ["team", "public"] }),
    ).toBe(true);

    expect(editor.getJSON().content?.[0]).toEqual({
      type: "visBlockMarker",
      attrs: { variant: "open", audiences: ["team", "public"] },
    });
  });
});
