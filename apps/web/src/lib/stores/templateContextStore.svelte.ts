/**
 * Template context store for resolving template variables in the editor.
 *
 * Holds a flattened key-value map derived from the current entry's frontmatter.
 * TemplateVariable and ConditionalBlock NodeViews subscribe to this store to
 * show live resolved values and active/inactive branch highlighting.
 *
 * Follows the same singleton pattern as other lib stores.
 */

export type TemplateContext = Record<string, unknown>;

function createTemplateContextStore() {
  let context = $state<TemplateContext>({});
  let previewAudience = $state<string | null>(null);

  return {
    get context() {
      return context;
    },

    /** The audience currently being previewed, or null for default (all branches visible). */
    get previewAudience() {
      return previewAudience;
    },

    setContext(newContext: TemplateContext) {
      context = newContext;
    },

    /** Set a specific audience to preview, or null to exit preview mode. */
    setPreviewAudience(audience: string | null) {
      previewAudience = audience;
    },

    /**
     * Resolve a variable name against the current context.
     * Returns the string representation, or null if the variable is missing/null.
     */
    resolve(variableName: string): string | null {
      const value = context[variableName];
      if (value === undefined || value === null) return null;
      if (Array.isArray(value)) return value.join(", ");
      return String(value);
    },

    clear() {
      context = {};
    },
  };
}

let sharedStore: ReturnType<typeof createTemplateContextStore> | null = null;

export function getTemplateContextStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get context(): TemplateContext {
        return {};
      },
      get previewAudience(): string | null {
        return null;
      },
      setContext: (_ctx: TemplateContext) => {},
      setPreviewAudience: (_audience: string | null) => {},
      resolve: (_name: string): string | null => null,
      clear: () => {},
    };
  }

  if (!sharedStore) {
    sharedStore = createTemplateContextStore();
  }
  return sharedStore;
}
