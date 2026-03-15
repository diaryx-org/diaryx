/**
 * TipTap extension for search highlighting with match navigation.
 *
 * Provides:
 * - setSearchTerm: set the search query and highlight all matches
 * - nextSearchResult / previousSearchResult: cycle through matches
 * - clearSearch: remove all highlights
 *
 * Uses ProseMirror Decorations to highlight matches without modifying the document.
 */

import { Extension } from "@tiptap/core";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

const searchPluginKey = new PluginKey("searchHighlight");

export interface SearchHighlightStorage {
  searchTerm: string;
  caseSensitive: boolean;
  results: { from: number; to: number }[];
  currentIndex: number;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    searchHighlight: {
      setSearchTerm: (term: string, caseSensitive?: boolean) => ReturnType;
      nextSearchResult: () => ReturnType;
      previousSearchResult: () => ReturnType;
      clearSearch: () => ReturnType;
    };
  }
}

function findMatches(
  doc: any,
  searchTerm: string,
  caseSensitive: boolean,
): { from: number; to: number }[] {
  if (!searchTerm) return [];

  const results: { from: number; to: number }[] = [];
  const term = caseSensitive ? searchTerm : searchTerm.toLowerCase();

  doc.descendants((node: any, pos: number) => {
    if (!node.isText) return;
    const text = caseSensitive ? node.text! : node.text!.toLowerCase();
    let index = text.indexOf(term);
    while (index !== -1) {
      results.push({
        from: pos + index,
        to: pos + index + searchTerm.length,
      });
      index = text.indexOf(term, index + 1);
    }
  });

  return results;
}


export const SearchHighlight = Extension.create<
  Record<string, never>,
  SearchHighlightStorage
>({
  name: "searchHighlight",

  addStorage() {
    return {
      searchTerm: "",
      caseSensitive: false,
      results: [],
      currentIndex: -1,
    };
  },

  addCommands() {
    return {
      setSearchTerm:
        (term: string, caseSensitive = false) =>
        ({ editor, tr, dispatch }) => {
          const storage = (editor.storage as any)
            .searchHighlight as SearchHighlightStorage;
          storage.searchTerm = term;
          storage.caseSensitive = caseSensitive;
          storage.results = findMatches(
            editor.state.doc,
            term,
            caseSensitive,
          );
          storage.currentIndex = storage.results.length > 0 ? 0 : -1;

          if (dispatch) {
            tr.setMeta(searchPluginKey, { updated: true });
            dispatch(tr);
          }

          // Scroll to first match
          if (storage.currentIndex >= 0) {
            const match = storage.results[storage.currentIndex];
            editor.commands.setTextSelection(match.from);
            editor.commands.scrollIntoView();
          }

          return true;
        },

      nextSearchResult:
        () =>
        ({ editor, tr, dispatch }) => {
          const storage = (editor.storage as any)
            .searchHighlight as SearchHighlightStorage;
          if (storage.results.length === 0) return false;

          storage.currentIndex =
            (storage.currentIndex + 1) % storage.results.length;

          if (dispatch) {
            tr.setMeta(searchPluginKey, { updated: true });
            dispatch(tr);
          }

          const match = storage.results[storage.currentIndex];
          editor.commands.setTextSelection(match.from);
          editor.commands.scrollIntoView();

          return true;
        },

      previousSearchResult:
        () =>
        ({ editor, tr, dispatch }) => {
          const storage = (editor.storage as any)
            .searchHighlight as SearchHighlightStorage;
          if (storage.results.length === 0) return false;

          storage.currentIndex =
            (storage.currentIndex - 1 + storage.results.length) %
            storage.results.length;

          if (dispatch) {
            tr.setMeta(searchPluginKey, { updated: true });
            dispatch(tr);
          }

          const match = storage.results[storage.currentIndex];
          editor.commands.setTextSelection(match.from);
          editor.commands.scrollIntoView();

          return true;
        },

      clearSearch:
        () =>
        ({ editor, tr, dispatch }) => {
          const storage = (editor.storage as any)
            .searchHighlight as SearchHighlightStorage;
          storage.searchTerm = "";
          storage.results = [];
          storage.currentIndex = -1;

          if (dispatch) {
            tr.setMeta(searchPluginKey, { updated: true });
            dispatch(tr);
          }

          return true;
        },
    };
  },

  addProseMirrorPlugins() {
    const extensionThis = this;

    return [
      new Plugin({
        key: searchPluginKey,
        state: {
          init() {
            return DecorationSet.empty;
          },
          apply(tr, oldDecorations) {
            const meta = tr.getMeta(searchPluginKey);
            if (meta?.updated || tr.docChanged) {
              const storage = extensionThis.storage as SearchHighlightStorage;
              // Re-run search if document changed
              if (tr.docChanged && storage.searchTerm) {
                storage.results = findMatches(
                  tr.doc,
                  storage.searchTerm,
                  storage.caseSensitive,
                );
                if (storage.currentIndex >= storage.results.length) {
                  storage.currentIndex = storage.results.length > 0 ? 0 : -1;
                }
              }

              if (storage.results.length === 0) {
                return DecorationSet.empty;
              }

              const decorations = storage.results.map((result, i) =>
                Decoration.inline(result.from, result.to, {
                  class:
                    i === storage.currentIndex
                      ? "search-highlight search-highlight--current"
                      : "search-highlight",
                }),
              );

              return DecorationSet.create(tr.doc, decorations);
            }

            if (tr.docChanged) {
              return oldDecorations.map(tr.mapping, tr.doc);
            }

            return oldDecorations;
          },
        },
        props: {
          decorations(state) {
            return this.getState(state) ?? DecorationSet.empty;
          },
        },
      }),
    ];
  },
});
