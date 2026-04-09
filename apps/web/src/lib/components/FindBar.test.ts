import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { toastErrorSpy } = vi.hoisted(() => ({
  toastErrorSpy: vi.fn(),
}));

vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("../test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/input", async () => ({
  Input: (await import("../test/InputStub.svelte")).default,
}));

vi.mock("$lib/hooks/useMobile.svelte", () => ({
  getMobileState: () => ({ isMobile: false }),
}));

vi.mock("svelte-sonner", () => ({
  toast: {
    error: toastErrorSpy,
  },
}));

import FindBar from "./FindBar.svelte";

type MockEditor = {
  commands: {
    clearSearch: ReturnType<typeof vi.fn>;
    setSearchTerm: ReturnType<typeof vi.fn>;
    nextSearchResult: ReturnType<typeof vi.fn>;
    previousSearchResult: ReturnType<typeof vi.fn>;
  };
  storage: {
    searchHighlight: {
      results: Array<{ from: number; to: number }>;
      currentIndex: number;
    };
  };
};

function createEditor(): MockEditor {
  return {
    commands: {
      clearSearch: vi.fn(),
      setSearchTerm: vi.fn(),
      nextSearchResult: vi.fn(),
      previousSearchResult: vi.fn(),
    },
    storage: {
      searchHighlight: {
        results: [{ from: 1, to: 4 }],
        currentIndex: 0,
      },
    },
  };
}

describe("FindBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("guards clearSearch failures when the bar closes", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const editor = createEditor();
    editor.commands.clearSearch.mockImplementation(() => {
      throw new Error("Called contentMatchAt on a node with invalid content");
    });

    const { rerender } = render(FindBar, {
      props: {
        open: true,
        editorRef: { getEditor: () => editor },
      },
    });

    await rerender({
      open: false,
      editorRef: { getEditor: () => editor },
    });

    await waitFor(() => {
      expect(toastErrorSpy).toHaveBeenCalledWith(
        "Could not search this entry",
        {
          description: "The entry contains invalid content, so find results are unavailable until you reopen it.",
        },
      );
    });

    expect(consoleErrorSpy).toHaveBeenCalled();
    consoleErrorSpy.mockRestore();
  });

  it("does not retry clearSearch on repeated closed rerenders", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const editor = createEditor();
    editor.commands.clearSearch.mockImplementation(() => {
      throw new Error("Called contentMatchAt on a node with invalid content");
    });

    const { rerender } = render(FindBar, {
      props: {
        open: true,
        editorRef: { getEditor: () => editor },
      },
    });

    await rerender({
      open: false,
      editorRef: { getEditor: () => editor },
    });
    await rerender({
      open: false,
      editorRef: { getEditor: () => editor },
    });

    expect(editor.commands.clearSearch).toHaveBeenCalledTimes(1);
    expect(toastErrorSpy).toHaveBeenCalledTimes(1);
    expect(consoleErrorSpy).toHaveBeenCalledTimes(1);

    consoleErrorSpy.mockRestore();
  });

  it("shows the search error toast only once after search commands start failing", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const editor = createEditor();
    editor.commands.setSearchTerm.mockImplementation(() => {
      throw new Error("Called contentMatchAt on a node with invalid content");
    });

    render(FindBar, {
      props: {
        open: true,
        editorRef: { getEditor: () => editor },
      },
    });

    const input = screen.getByPlaceholderText("Find...");

    await fireEvent.input(input, { target: { value: "bad" } });
    await fireEvent.input(input, { target: { value: "worse" } });

    expect(editor.commands.setSearchTerm).toHaveBeenCalledTimes(1);
    expect(toastErrorSpy).toHaveBeenCalledTimes(1);
    expect(consoleErrorSpy).toHaveBeenCalledTimes(1);

    consoleErrorSpy.mockRestore();
  });
});
