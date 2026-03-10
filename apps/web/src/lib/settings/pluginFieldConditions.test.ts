import { describe, expect, it } from "vitest";

import { evaluateFieldCondition } from "./pluginFieldConditions";

const freeAuthState = {
  isAuthenticated: false,
  tier: "free",
};

describe("evaluateFieldCondition", () => {
  it("supports config equality checks", () => {
    expect(
      evaluateFieldCondition("config:import_format=dayone", freeAuthState, {
        import_format: "dayone",
      }),
    ).toBe(true);
    expect(
      evaluateFieldCondition("config:import_format=markdown", freeAuthState, {
        import_format: "dayone",
      }),
    ).toBe(false);
  });

  it("supports config inequality checks", () => {
    expect(
      evaluateFieldCondition("config:markdown_destination!=root", freeAuthState, {
        markdown_destination: "subfolder",
      }),
    ).toBe(true);
  });

  it("preserves existing auth-based conditions", () => {
    expect(evaluateFieldCondition("not_plus", freeAuthState, {})).toBe(true);
    expect(
      evaluateFieldCondition("authenticated", { isAuthenticated: true, tier: "plus" }, {}),
    ).toBe(true);
  });
});
