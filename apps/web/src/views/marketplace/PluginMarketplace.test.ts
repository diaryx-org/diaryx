import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

const fetchPluginRegistryMock = vi.fn();
const browserInstallPluginMock = vi.fn();
const browserUninstallPluginMock = vi.fn();
const inspectPluginWasmMock = vi.fn();
const proxyFetchMock = vi.fn();
const openExternalUrlMock = vi.fn();

const pluginStore = {
  allManifests: [] as Array<{ id: string; name: string; description?: string }>,
  isPluginEnabled: vi.fn(() => true),
  setPluginEnabled: vi.fn(),
};

const mobileState = { isMobile: false };

vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("../../lib/test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/input", async () => ({
  Input: (await import("../../lib/test/InputStub.svelte")).default,
}));

vi.mock("$lib/components/ui/switch", async () => ({
  Switch: (await import("../../lib/test/SwitchStub.svelte")).default,
}));

vi.mock("$lib/components/ui/badge", async () => ({
  Badge: (await import("../../lib/test/BadgeStub.svelte")).default,
}));

vi.mock("$lib/components/ui/separator", async () => ({
  Separator: (await import("../../lib/test/SeparatorStub.svelte")).default,
}));

vi.mock("$lib/plugins/pluginRegistry", () => ({
  fetchPluginRegistry: (...args: unknown[]) => fetchPluginRegistryMock(...args),
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  getBrowserPluginSupport: () => ({ supported: true, reason: null }),
  getBrowserPluginSupportError: () => null,
  installPlugin: (...args: unknown[]) => browserInstallPluginMock(...args),
  uninstallPlugin: (...args: unknown[]) => browserUninstallPluginMock(...args),
  inspectPluginWasm: (...args: unknown[]) => inspectPluginWasmMock(...args),
}));

vi.mock("$lib/backend", () => ({
  getBackend: async () => ({}),
  isTauri: () => false,
}));

vi.mock("$lib/backend/api", () => ({
  createApi: () => ({
    resolveWorkspaceRootIndexPath: vi.fn(async () => null),
    getFrontmatter: vi.fn(),
    setFrontmatterProperty: vi.fn(),
  }),
}));

vi.mock("$lib/backend/proxyFetch", () => ({
  proxyFetch: (...args: unknown[]) => proxyFetchMock(...args),
}));

vi.mock("@/models/stores/workspaceStore.svelte", () => ({
  workspaceStore: { tree: null },
}));

vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: () => pluginStore,
}));

vi.mock("$lib/hooks/useMobile.svelte", () => ({
  getMobileState: () => mobileState,
}));

vi.mock("$lib/billing", () => ({
  openExternalUrl: (...args: unknown[]) => openExternalUrlMock(...args),
}));

import PluginMarketplace from "./PluginMarketplace.svelte";

const registryPlugin = {
  id: "diaryx.publish",
  name: "Publish",
  version: "1.2.0",
  summary: "Ship a site",
  description: "Publish your workspace",
  author: "Diaryx",
  license: "MIT",
  artifact: {
    url: "https://example.com/publish.wasm",
    sha256: `sha256:${"00".repeat(32)}`,
    size: 2048,
    published_at: "2026-03-05T00:00:00Z",
  },
  repository: "https://example.com/repo",
  categories: ["sharing"],
  tags: ["publish"],
  icon: null,
  screenshots: [],
  capabilities: ["publish"],
  requested_permissions: null,
};

describe("PluginMarketplace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    pluginStore.allManifests = [];
    fetchPluginRegistryMock.mockResolvedValue({ plugins: [registryPlugin] });
    proxyFetchMock.mockResolvedValue(
      new Response(new Uint8Array([104, 101, 108, 108, 111]), { status: 200 }),
    );
    inspectPluginWasmMock.mockResolvedValue({
      pluginId: "diaryx.publish",
      pluginName: "Publish",
      requestedPermissions: null,
    });
    vi.stubGlobal("crypto", {
      subtle: {
        digest: vi.fn(async () => new Uint8Array(32).buffer),
      },
    });
    vi.stubGlobal("confirm", vi.fn(() => true));
  });

  it("loads the registry and filters plugins by search text", async () => {
    render(PluginMarketplace, { onClose: vi.fn() });

    expect(await screen.findAllByText("Publish")).toHaveLength(2);
    await fireEvent.input(screen.getByPlaceholderText("Search plugins"), {
      target: { value: "missing" },
    });

    await waitFor(() => {
      expect(screen.getByText("No plugins match your filters.")).toBeInTheDocument();
    });
  });

  it("installs a registry plugin and shows it as installed on rerender", async () => {
    const { rerender } = render(PluginMarketplace, { onClose: vi.fn() });

    const installButtons = await screen.findAllByRole("button", { name: "Install" });
    await fireEvent.click(installButtons[0]!);

    await waitFor(() => {
      expect(proxyFetchMock).toHaveBeenCalledWith("https://example.com/publish.wasm");
      expect(browserInstallPluginMock).toHaveBeenCalled();
    });

    pluginStore.allManifests = [{ id: "diaryx.publish", name: "Publish" }];
    await rerender({ onClose: vi.fn() });

    expect(await screen.findByText("Installed")).toBeInTheDocument();
  });
});
