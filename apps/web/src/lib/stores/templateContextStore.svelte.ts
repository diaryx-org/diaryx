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
  let previewAudience = $state<string[] | null>(null);
  let audiencesVersion = $state(0);

  return {
    get context() {
      return context;
    },

    /** The audiences currently being previewed, or null for default (all branches visible). */
    get previewAudience() {
      return previewAudience;
    },

    /**
     * Monotonically increasing counter. Incremented whenever a brand-new audience
     * tag is created so that the audience panel can re-fetch the available list.
     */
    get audiencesVersion() {
      return audiencesVersion;
    },

    setContext(newContext: TemplateContext) {
      context = newContext;
    },

    /** Set specific audiences to preview, or null to exit preview mode. */
    setPreviewAudience(audiences: string[] | null) {
      previewAudience = audiences && audiences.length > 0 ? audiences : null;
    },

    /** Toggle a single audience in/out of the preview set. */
    togglePreviewAudience(name: string) {
      if (!previewAudience) {
        previewAudience = [name];
        return;
      }
      const idx = previewAudience.indexOf(name);
      if (idx >= 0) {
        const next = previewAudience.filter((_, i) => i !== idx);
        previewAudience = next.length > 0 ? next : null;
      } else {
        previewAudience = [...previewAudience, name];
      }
    },

    /** Call after creating a brand-new audience tag to notify the panel to refresh. */
    bumpAudiencesVersion() {
      audiencesVersion += 1;
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
      get previewAudience(): string[] | null {
        return null;
      },
      get audiencesVersion(): number {
        return 0;
      },
      setContext: (_ctx: TemplateContext) => {},
      setPreviewAudience: (_audiences: string[] | null) => {},
      togglePreviewAudience: (_name: string) => {},
      bumpAudiencesVersion: () => {},
      resolve: (_name: string): string | null => null,
      clear: () => {},
    };
  }

  if (!sharedStore) {
    sharedStore = createTemplateContextStore();
  }
  return sharedStore;
}
