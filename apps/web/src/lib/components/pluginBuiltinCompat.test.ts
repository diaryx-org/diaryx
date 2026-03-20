import { describe, expect, it } from "vitest";

import { getLegacyBuiltinFields } from "./pluginBuiltinCompat";

describe("pluginBuiltinCompat", () => {
  it("maps legacy publish builtin to declarative fields", () => {
    const fields = getLegacyBuiltinFields("publish.panel");

    expect(fields).toEqual([
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.guard" }),
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.site-url" }),
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.subdomain" }),
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.custom-domains" }),
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.audiences" }),
      expect.objectContaining({ type: "HostWidget", widget_id: "namespace.publish-button" }),
      expect.objectContaining({ type: "Section", label: "Export" }),
      expect.objectContaining({
        type: "HostActionButton",
        action_type: "open-export-dialog",
      }),
    ]);
  });

  it("returns null for unknown builtins", () => {
    expect(getLegacyBuiltinFields("unknown.component")).toBeNull();
  });
});
