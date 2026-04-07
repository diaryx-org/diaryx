import { Editor } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import StarterKit from "@tiptap/starter-kit";
import { render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Api } from "$lib/backend/api";
import { VisibilityBlock } from "$lib/extensions/VisibilityBlock";
import { VisibilityMark } from "$lib/extensions/VisibilityMark";

import VisibilityPicker from "./VisibilityPicker.svelte";

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
    extensions: [StarterKit, VisibilityMark, VisibilityBlock],
    content,
  });

  editors.push(editor);
  return editor;
}

function selectText(editor: Editor, text: string) {
  let range: { from: number; to: number } | null = null;

  editor.state.doc.descendants((node, pos) => {
    if (range) return false;
    if (!node.isText) return;

    const index = node.text?.indexOf(text) ?? -1;
    if (index < 0) return;

    range = {
      from: pos + index,
      to: pos + index + text.length,
    };
    return false;
  });

  if (!range) throw new Error(`Could not find text: ${text}`);
  const selection = range as { from: number; to: number };

  editor.view.dispatch(
    editor.state.tr.setSelection(
      TextSelection.create(editor.state.doc, selection.from, selection.to),
    ),
  );
}

function createApi(): Api {
  return {
    getAvailableAudiences: vi.fn(async () => ["family", "team"]),
  } as unknown as Api;
}

describe("VisibilityPicker", () => {
  it("refreshes selected audiences after the editor selection changes", async () => {
    const editor = createEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "plain " },
            {
              type: "text",
              text: "marked",
              marks: [
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
          ],
        },
      ],
    });
    const api = createApi();

    selectText(editor, "plain");
    const { rerender } = render(VisibilityPicker, {
      editor,
      api,
      rootPath: "README.md",
      open: true,
    });

    expect(await screen.findByRole("option", { name: /family/ }))
      .toHaveAttribute("aria-selected", "false");

    await rerender({
      editor,
      api,
      rootPath: "README.md",
      open: false,
    });

    selectText(editor, "marked");

    await rerender({
      editor,
      api,
      rootPath: "README.md",
      open: true,
    });

    await waitFor(() => {
      expect(screen.getByRole("option", { name: /family/ }))
        .toHaveAttribute("aria-selected", "true");
    });
  });

  it("prefers an existing inline audience over block wrapping for a full paragraph selection", async () => {
    const editor = createEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "marked",
              marks: [
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
          ],
        },
      ],
    });

    selectText(editor, "marked");
    render(VisibilityPicker, {
      editor,
      api: createApi(),
      rootPath: "README.md",
      open: true,
    });

    expect(await screen.findByText("Apply inline visibility")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /family/ }))
      .toHaveAttribute("aria-selected", "true");
  });
});
