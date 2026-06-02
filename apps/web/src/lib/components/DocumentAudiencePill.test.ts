import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";

vi.mock("@lucide/svelte", async () => {
  const Stub = (await import("../test/IconStub.svelte")).default;
  return {
    Plus: Stub,
    Lock: Stub,
    ArrowUpRight: Stub,
    X: Stub,
  };
});

vi.mock("$lib/stores/audienceColorStore.svelte", () => ({
  getAudienceColorStore: () => ({
    audienceColors: {},
  }),
}));

vi.mock("$lib/stores/audiencePanelStore.svelte", () => ({
  getAudiencePanelStore: () => ({
    openPanel: vi.fn(),
  }),
}));

import DocumentAudiencePill from "./DocumentAudiencePill.svelte";

describe("DocumentAudiencePill", () => {
  it("renders a legacy scalar audience value as a single tag", () => {
    render(DocumentAudiencePill, {
      props: {
        audience: "family",
        entryPath: "entry.md",
        api: null,
        onChange: vi.fn(),
      },
    });

    expect(screen.getByRole("button", { name: /Audience: family/i })).toBeInTheDocument();
    expect(screen.getByText("family")).toBeInTheDocument();
  });

  it("renders malformed audience values without crashing", () => {
    render(DocumentAudiencePill, {
      props: {
        audience: { name: "family" },
        entryPath: "entry.md",
        api: null,
        onChange: vi.fn(),
      },
    });

    expect(screen.getByRole("button", { name: /Audience: Private/i })).toBeInTheDocument();
    expect(screen.getByText("Private")).toBeInTheDocument();
  });
});
