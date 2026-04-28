import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  setPluginMetadata: vi.fn(),
  getPrimaryWorkspaceProviderLink: vi.fn(),
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  setPluginMetadata: mocks.setPluginMetadata,
  getPrimaryWorkspaceProviderLink: mocks.getPrimaryWorkspaceProviderLink,
}));

import { hydrateProviderLinksFromFrontmatter } from "./hydrateProviderLinks";

function makeApi(rootIndex: string | null, frontmatter: Record<string, unknown>) {
  return {
    findRootIndex: vi.fn(async () => rootIndex ?? ""),
    getFrontmatter: vi.fn(async () => frontmatter),
  } as never;
}

describe("hydrateProviderLinksFromFrontmatter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getPrimaryWorkspaceProviderLink.mockReturnValue(null);
  });

  it("seeds the registry from a sync workspace_id in root frontmatter", async () => {
    const api = makeApi("/ws/index.md", {
      plugins: { "diaryx.sync": { workspace_id: "workspace:abc123" } },
    });
    const backend = { getWorkspacePath: () => "/ws" };

    await hydrateProviderLinksFromFrontmatter("local-1", api, backend);

    expect(mocks.setPluginMetadata).toHaveBeenCalledWith("local-1", "diaryx.sync", {
      remoteWorkspaceId: "workspace:abc123",
      syncEnabled: true,
    });
  });

  it("skips when the existing link already matches the frontmatter", async () => {
    mocks.getPrimaryWorkspaceProviderLink.mockReturnValue({
      pluginId: "diaryx.sync",
      remoteWorkspaceId: "workspace:abc123",
      syncEnabled: true,
    });
    const api = makeApi("/ws/index.md", {
      plugins: { "diaryx.sync": { workspace_id: "workspace:abc123" } },
    });
    const backend = { getWorkspacePath: () => "/ws" };

    await hydrateProviderLinksFromFrontmatter("local-1", api, backend);

    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });

  it("does nothing when frontmatter has no plugins entry", async () => {
    const api = makeApi("/ws/index.md", { title: "Hello" });
    const backend = { getWorkspacePath: () => "/ws" };

    await hydrateProviderLinksFromFrontmatter("local-1", api, backend);

    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });

  it("ignores plugin entries without a workspace_id", async () => {
    const api = makeApi("/ws/index.md", {
      plugins: { "diaryx.sync": { other: "value" } },
    });
    const backend = { getWorkspacePath: () => "/ws" };

    await hydrateProviderLinksFromFrontmatter("local-1", api, backend);

    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });

  it("swallows errors from frontmatter reads", async () => {
    const api = {
      findRootIndex: vi.fn(async () => {
        throw new Error("boom");
      }),
      getFrontmatter: vi.fn(),
    } as never;
    const backend = { getWorkspacePath: () => "/ws" };

    await expect(
      hydrateProviderLinksFromFrontmatter("local-1", api, backend),
    ).resolves.toBeUndefined();
    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });
});
