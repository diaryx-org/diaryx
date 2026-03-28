import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Mock state & fns (declared before vi.mock calls)
// ---------------------------------------------------------------------------

const getLocalWorkspacesMock = vi.fn(() => [] as Array<{ id: string; name: string; path?: string }>);
const createLocalWorkspaceMock = vi.fn(() => ({
  id: "ws-1",
  name: "New Workspace",
  path: undefined,
}));

const switchWorkspaceMock = vi.fn(async () => {});
const getBackendMock = vi.fn(async () => ({
  getWorkspacePath: () => "/mock/workspace/index.md",
  importFromZip: vi.fn(async () => ({ success: true, files_imported: 3 })),
}));
const createApiMock = vi.fn(() => ({
  findRootIndex: vi.fn(async () => "/mock/workspace/index.md"),
  createWorkspace: vi.fn(async () => {}),
}));

const fetchStarterWorkspaceRegistryMock = vi.fn(async () => ({ starters: [] }));
const fetchStarterWorkspaceZipMock = vi.fn(async () => new Blob());
const isTierLimitErrorMock = vi.fn(() => false);
const resolveStorageTypeMock = vi.fn(async () => "opfs" as const);
const isStorageTypeSupportedMock = vi.fn(() => false);
const storeWorkspaceFileSystemHandleMock = vi.fn(async () => {});
const authorizeWorkspacePathMock = vi.fn(async (p: string) => p);
const pickAuthorizedWorkspaceFolderMock = vi.fn(async () => null);
const getProviderStatusMock = vi.fn(async () => ({ ready: true, message: null }));
const linkWorkspaceMock = vi.fn(async () => {});
const listRemoteWorkspacesMock = vi.fn(async () => []);
const downloadWorkspaceMock = vi.fn(async () => ({ filesImported: 5 }));
const captureProviderPluginForTransferMock = vi.fn(async () => null);
const installCapturedProviderPluginMock = vi.fn(async () => {});

const pluginStore = {
  workspaceProviders: [] as Array<{ contribution: { id: string; label: string } }>,
};

// ---------------------------------------------------------------------------
// vi.mock — Dialog stubs that render children
// ---------------------------------------------------------------------------

// Dialog sub-components need to render their children/snippets for content
// to be visible in jsdom. ButtonStub renders its children snippet, so it
// works as a passthrough wrapper for all Dialog parts.
vi.mock("$lib/components/ui/dialog", async () => {
  const ButtonStub = (await import("./test/ButtonStub.svelte")).default;

  return {
    Root: ButtonStub,
    Content: ButtonStub,
    Header: ButtonStub,
    Title: ButtonStub,
    Description: ButtonStub,
  };
});

vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("./test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/input", async () => ({
  Input: (await import("./test/InputStub.svelte")).default,
}));

vi.mock("$lib/components/ui/label", async () => ({
  Label: (await import("./test/LabelStub.svelte")).default,
}));

vi.mock("$lib/components/ui/progress", () => ({
  Progress: function MockProgress() {},
}));

vi.mock("@lucide/svelte", () => {
  const stub = function MockIcon() {};
  return {
    Loader2: stub,
    AlertCircle: stub,
    Upload: stub,
    Plus: stub,
    FolderOpen: stub,
    FolderTree: stub,
    Cloud: stub,
    CloudDownload: stub,
  };
});

vi.mock("./backend", () => ({
  getBackend: (...a: Parameters<typeof getBackendMock>) => getBackendMock(...a),
  createApi: (...a: Parameters<typeof createApiMock>) => createApiMock(...a),
}));

vi.mock("$lib/backend/interface", () => ({
  isTauri: () => false,
}));

vi.mock("$lib/backend/workspaceAccess", () => ({
  authorizeWorkspacePath: (...a: Parameters<typeof authorizeWorkspacePathMock>) => authorizeWorkspacePathMock(...a),
  pickAuthorizedWorkspaceFolder: (...a: Parameters<typeof pickAuthorizedWorkspaceFolderMock>) => pickAuthorizedWorkspaceFolderMock(...a),
}));

vi.mock("$lib/billing", () => ({
  isTierLimitError: (...a: Parameters<typeof isTierLimitErrorMock>) => isTierLimitErrorMock(...a),
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  getLocalWorkspaces: (...a: Parameters<typeof getLocalWorkspacesMock>) => getLocalWorkspacesMock(...a),
  createLocalWorkspace: (...a: Parameters<typeof createLocalWorkspaceMock>) => createLocalWorkspaceMock(...a),
}));

vi.mock("$lib/backend/storageType", () => ({
  isStorageTypeSupported: (...a: Parameters<typeof isStorageTypeSupportedMock>) => isStorageTypeSupportedMock(...a),
  resolveStorageType: (...a: Parameters<typeof resolveStorageTypeMock>) => resolveStorageTypeMock(...a),
  storeWorkspaceFileSystemHandle: (...a: Parameters<typeof storeWorkspaceFileSystemHandleMock>) => storeWorkspaceFileSystemHandleMock(...a),
}));

vi.mock("$lib/workspace/switchWorkspace", () => ({
  switchWorkspace: (...a: Parameters<typeof switchWorkspaceMock>) => switchWorkspaceMock(...a),
}));

vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: () => pluginStore,
}));

vi.mock("$lib/sync/workspaceProviderService", () => ({
  getProviderStatus: (...a: Parameters<typeof getProviderStatusMock>) => getProviderStatusMock(...a),
  linkWorkspace: (...a: Parameters<typeof linkWorkspaceMock>) => linkWorkspaceMock(...a),
  listRemoteWorkspaces: (...a: Parameters<typeof listRemoteWorkspacesMock>) => listRemoteWorkspacesMock(...a),
  downloadWorkspace: (...a: Parameters<typeof downloadWorkspaceMock>) => downloadWorkspaceMock(...a),
}));

vi.mock("$lib/sync/browserProviderBootstrap", () => ({
  captureProviderPluginForTransfer: (...a: Parameters<typeof captureProviderPluginForTransferMock>) => captureProviderPluginForTransferMock(...a),
  installCapturedProviderPlugin: (...a: Parameters<typeof installCapturedProviderPluginMock>) => installCapturedProviderPluginMock(...a),
}));

vi.mock("$lib/marketplace/starterWorkspaceRegistry", () => ({
  fetchStarterWorkspaceRegistry: (...a: Parameters<typeof fetchStarterWorkspaceRegistryMock>) => fetchStarterWorkspaceRegistryMock(...a),
}));

vi.mock("$lib/marketplace/starterWorkspaceApply", () => ({
  fetchStarterWorkspaceZip: (...a: Parameters<typeof fetchStarterWorkspaceZipMock>) => fetchStarterWorkspaceZipMock(...a),
}));

// ---------------------------------------------------------------------------
// Import component AFTER all mocks
// ---------------------------------------------------------------------------

import AddWorkspaceDialog from "./AddWorkspaceDialog.svelte";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("AddWorkspaceDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getLocalWorkspacesMock.mockReturnValue([]);
    createLocalWorkspaceMock.mockReturnValue({
      id: "ws-1",
      name: "New Workspace",
      path: undefined,
    });
    fetchStarterWorkspaceRegistryMock.mockResolvedValue({ starters: [] });
    pluginStore.workspaceProviders = [];
  });

  it("renders dialog title 'Add Workspace' when open", async () => {
    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      expect(screen.getByText("Add Workspace")).toBeInTheDocument();
    });
  });

  it("shows workspace name input with default name", async () => {
    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      const input = screen.getByPlaceholderText("My Workspace");
      expect(input).toBeInTheDocument();
      expect((input as HTMLInputElement).value).toBe("New Workspace");
    });
  });

  it("renders content source options (Import from ZIP, Start fresh)", async () => {
    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      expect(screen.getByText("Import from ZIP")).toBeInTheDocument();
      expect(screen.getByText("Start fresh")).toBeInTheDocument();
    });
  });

  it("submit button is disabled when workspace name is empty", async () => {
    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      expect(screen.getByPlaceholderText("My Workspace")).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText("My Workspace");
    await fireEvent.input(input, { target: { value: "" } });

    await waitFor(() => {
      const submitButton = screen.getByText("Create Workspace");
      expect(submitButton.closest("button")).toBeDisabled();
    });
  });

  it("calls onComplete after successful fresh workspace creation", async () => {
    const onComplete = vi.fn();

    render(AddWorkspaceDialog, { open: true, onComplete });

    await waitFor(() => {
      expect(screen.getByText("Create Workspace")).toBeInTheDocument();
    });

    const submitButton = screen.getByText("Create Workspace");
    await fireEvent.click(submitButton.closest("button")!);

    await waitFor(() => {
      expect(createLocalWorkspaceMock).toHaveBeenCalled();
      expect(switchWorkspaceMock).toHaveBeenCalledWith("ws-1", "New Workspace");
      expect(onComplete).toHaveBeenCalledWith(null);
    });
  });

  it("shows error when initialization fails", async () => {
    createLocalWorkspaceMock.mockImplementation(() => {
      throw new Error("Disk full");
    });

    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      expect(screen.getByText("Create Workspace")).toBeInTheDocument();
    });

    const submitButton = screen.getByText("Create Workspace");
    await fireEvent.click(submitButton.closest("button")!);

    await waitFor(() => {
      expect(screen.getByText("Disk full")).toBeInTheDocument();
    });
  });

  it("'Download from Cloud' option not shown when no providers", async () => {
    pluginStore.workspaceProviders = [];

    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      expect(screen.getByText("Start fresh")).toBeInTheDocument();
    });

    expect(screen.queryByText("Download from Cloud")).not.toBeInTheDocument();
  });

  it("calls onOpenChange(false) when dialog closes", async () => {
    const onOpenChange = vi.fn();

    render(AddWorkspaceDialog, { open: true, onOpenChange });

    await waitFor(() => {
      expect(screen.getByText("Create Workspace")).toBeInTheDocument();
    });

    // Trigger a successful submission which calls handleClose -> onOpenChange(false)
    const submitButton = screen.getByText("Create Workspace");
    await fireEvent.click(submitButton.closest("button")!);

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  it("generates incremented workspace name when existing names collide", async () => {
    getLocalWorkspacesMock.mockReturnValue([
      { id: "ws-0", name: "New Workspace", path: undefined },
    ]);

    render(AddWorkspaceDialog, { open: true });

    await waitFor(() => {
      const input = screen.getByPlaceholderText("My Workspace") as HTMLInputElement;
      expect(input.value).toBe("New Workspace 2");
    });
  });
});
