import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ============================================================================
// Stub global
// ============================================================================

vi.stubGlobal("__APP_VERSION__", "0.0.0-test");

// ============================================================================
// Mock functions
// ============================================================================

const onOpenEntry = vi.fn();
const onToggleNode = vi.fn();
const onToggleCollapse = vi.fn();
const onOpenSettings = vi.fn();
const onOpenMarketplace = vi.fn();
const onOpenAccountSettings = vi.fn();
const onAddWorkspace = vi.fn();
const onMoveEntry = vi.fn();
const onCreateChildEntry = vi.fn();
const onDeleteEntry = vi.fn();
const onExport = vi.fn();
const onAddAttachment = vi.fn();

// ============================================================================
// Mocks – MUST come before the component import
// ============================================================================

vi.mock("$lib/backend", () => ({
  isTauri: () => false,
}));

vi.mock("$lib/windowDrag", () => ({
  maybeStartWindowDrag: vi.fn(),
}));

vi.mock("$lib/backend/generated", () => ({}));

vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("./test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/tooltip", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Trigger: S, Content: S };
});

vi.mock("$lib/components/ui/kbd", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Group: S };
});

vi.mock("$lib/components/ui/context-menu", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Trigger: S, Content: S, Item: S, Separator: S, Sub: S, SubTrigger: S, SubContent: S };
});

vi.mock("$lib/components/ui/dropdown-menu", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Trigger: S, Content: S, Item: S, Separator: S };
});

vi.mock("$lib/components/ui/popover", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Trigger: S, Content: S };
});

vi.mock("./SignInDialog.svelte", async () => ({
  default: (await import("./test/PassthroughStub.svelte")).default,
}));
vi.mock("../views/sidebar/MobileActionSheet.svelte", async () => ({
  default: (await import("./test/PassthroughStub.svelte")).default,
}));

vi.mock("./hooks/useContextMenu.svelte", () => ({
  createContextMenuState: () => ({
    useBottomSheet: false,
    bottomSheetOpen: false,
    targetData: null,
    openMenu: vi.fn(),
    closeMenu: vi.fn(),
  }),
}));

vi.mock("./hooks/useMobile.svelte", () => ({
  getMobileState: () => ({ isMobile: false, isTouchDevice: false }),
}));

vi.mock("../models/stores/workspaceStore.svelte", () => ({
  workspaceStore: {
    backend: null,
    expandNode: vi.fn(),
  },
}));

vi.mock("@lucide/svelte", () => {
  function MockIcon() {}
  return {
    ChevronRight: MockIcon,
    ChevronDown: MockIcon,
    FileText: MockIcon,
    Folder: MockIcon,
    FolderClosed: MockIcon,
    FolderMinus: MockIcon,
    FolderPlus: MockIcon,
    Loader2: MockIcon,
    PanelLeftClose: MockIcon,
    AlertCircle: MockIcon,
    AlertTriangle: MockIcon,
    Plus: MockIcon,
    Settings: MockIcon,
    Store: MockIcon,
    Wrench: MockIcon,
    Eye: MockIcon,
    X: MockIcon,
    MoreVertical: MockIcon,
    FolderInput: MockIcon,
    FolderOpen: MockIcon,
    CircleUser: MockIcon,
    Download: MockIcon,
    SearchCheck: MockIcon,
    Trash2: MockIcon,
    Pencil: MockIcon,
    Copy: MockIcon,
    CircleHelp: MockIcon,
    Share2: MockIcon,
    History: MockIcon,
    FolderTree: MockIcon,
    Globe: MockIcon,
  };
});

vi.mock("./auth", () => ({
  getAuthState: () => ({ isAuthenticated: false, user: null }),
}));

vi.mock("@/models/stores/collaborationStore.svelte", () => ({
  collaborationStore: { serverOffline: false },
}));

vi.mock("./WorkspaceSelector.svelte", async () => ({
  default: (await import("./test/PassthroughStub.svelte")).default,
}));
vi.mock("$lib/stores/audiencePanelStore.svelte", () => ({
  getAudiencePanelStore: () => ({
    panelOpen: false,
    mode: "view" as const,
    paintBrushes: [] as readonly string[],
    transientAudience: null,
    hasEditorSelection: false,
    openPanel: () => {},
    closePanel: () => {},
    setMode: () => {},
    toggleBrush: () => {},
    setBrushes: () => {},
    createTransientBrush: () => {},
    notePainted: () => {},
    confirmTransientPersisted: () => {},
    setHasEditorSelection: () => {},
    registerApplyPaintBrush: () => {},
    applyBrushToSelection: () => false,
  }),
  CLEAR_BRUSH: "__clear__",
}));
vi.mock("./components/PluginSidebarPanel.svelte", async () => ({
  default: (await import("./test/PassthroughStub.svelte")).default,
}));

vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: () => ({
    leftSidebarTabs: [],
    leftSidebarContextMenuOwner: null,
  }),
}));

vi.mock("./leftSidebarSelection", () => ({
  collectTreePaths: (tree: any) => {
    if (!tree) return [];
    const paths: string[] = [tree.path];
    for (const child of tree.children ?? []) {
      paths.push(child.path);
    }
    return paths;
  },
  collectVisibleTreePaths: () => [],
  getRenderableSidebarChildren: (node: any) =>
    (node.children ?? []).filter((c: any) => !c.name.startsWith("... (")),
  getTreeSelectionRange: () => [],
}));

// ============================================================================
// Import the component under test AFTER all mocks
// ============================================================================

import LeftSidebar from "./LeftSidebar.svelte";

// ============================================================================
// Helpers
// ============================================================================

function createTree(overrides: Record<string, unknown> = {}) {
  return {
    path: "workspace",
    name: "workspace",
    title: "Workspace",
    description: null,
    is_index: true,
    children: [],
    ...overrides,
  };
}

function defaultProps(overrides: Record<string, unknown> = {}) {
  return {
    tree: null as any,
    currentEntry: null,
    isLoading: false,
    expandedNodes: new Set<string>(),
    validationResult: null,
    collapsed: false,
    showUnlinkedFiles: false,
    api: null,
    onOpenEntry,
    onToggleNode,
    onToggleCollapse,
    onOpenSettings,
    onOpenMarketplace,
    onOpenAccountSettings,
    onAddWorkspace,
    onMoveEntry,
    onCreateChildEntry,
    onDeleteEntry,
    onExport,
    onAddAttachment,
    ...overrides,
  };
}

// ============================================================================
// Tests
// ============================================================================

describe("LeftSidebar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders sidebar global controls", () => {
    render(LeftSidebar, defaultProps());
    expect(screen.getByLabelText("Collapse sidebar")).toBeInTheDocument();
    expect(screen.getByLabelText("Open marketplace")).toBeInTheDocument();
  });

  it("groups marketplace with workspace controls and collapse with footer actions", () => {
    render(LeftSidebar, defaultProps());
    const marketplaceButton = screen.getByLabelText("Open marketplace");
    const signInLabel = screen.getByText("Sign in");
    const settingsButton = screen.getByLabelText("Open settings");
    const collapseButton = screen.getByLabelText("Collapse sidebar");

    expect(marketplaceButton.compareDocumentPosition(signInLabel) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(signInLabel.compareDocumentPosition(settingsButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(settingsButton.compareDocumentPosition(collapseButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders loading state when isLoading is true and tree is null", () => {
    render(
      LeftSidebar,
      defaultProps({ isLoading: true, tree: null }),
    );
    // The Loader2 icon is rendered as a MockIcon function; check the loading container exists
    // The loading state renders a div with py-8 and an icon inside
    // Since icons are mocked, we verify that neither the tree view nor the empty/no-workspace
    // messages are shown
    expect(screen.queryByText("No workspace found")).not.toBeInTheDocument();
    expect(screen.queryByText("This workspace is empty")).not.toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  });

  it("renders empty workspace message when tree has no children", () => {
    const tree = createTree({ path: ".", children: [] });
    render(LeftSidebar, defaultProps({ tree }));
    expect(screen.getByText("This workspace is empty")).toBeInTheDocument();
  });

  it('renders "No workspace found" when tree is null and not loading', () => {
    render(LeftSidebar, defaultProps({ tree: null, isLoading: false }));
    expect(screen.getByText("No workspace found")).toBeInTheDocument();
  });

  it("renders workspace missing state with workspace name", () => {
    render(
      LeftSidebar,
      defaultProps({
        workspaceMissing: { id: "ws-1", name: "My Workspace" },
      }),
    );
    expect(screen.getByText("Not found")).toBeInTheDocument();
    expect(
      screen.getByText(/My Workspace.*may have been moved or deleted/),
    ).toBeInTheDocument();
  });

  it("renders tree nodes from tree data", () => {
    const tree = createTree({
      children: [
        {
          path: "workspace/intro.md",
          name: "intro.md",
          title: "Intro",
          description: null,
          is_index: false,
          children: [],
        },
        {
          path: "workspace/guide.md",
          name: "guide.md",
          title: "Guide",
          description: null,
          is_index: false,
          children: [],
        },
      ],
    });

    render(LeftSidebar, defaultProps({ tree, expandedNodes: new Set(["workspace"]) }));
    // .md suffix is stripped in display
    expect(screen.getByText("intro")).toBeInTheDocument();
    expect(screen.getByText("guide")).toBeInTheDocument();
  });

  it("calls onOpenEntry when a tree node is clicked", async () => {
    const tree = createTree({
      children: [
        {
          path: "workspace/intro.md",
          name: "intro.md",
          title: "Intro",
          description: null,
          is_index: false,
          children: [],
        },
      ],
    });

    render(LeftSidebar, defaultProps({ tree, expandedNodes: new Set(["workspace"]) }));

    await fireEvent.click(screen.getByText("intro"));
    expect(onOpenEntry).toHaveBeenCalledWith("workspace/intro.md");
  });

  it("calls onToggleNode when folder toggle button is clicked", async () => {
    const tree = createTree({
      children: [
        {
          path: "workspace/docs/README.md",
          name: "docs",
          title: "Docs",
          description: null,
          is_index: true,
          children: [
            {
              path: "workspace/docs/getting-started.md",
              name: "getting-started.md",
              title: "Getting Started",
              description: null,
              is_index: false,
              children: [],
            },
          ],
        },
      ],
    });

    render(LeftSidebar, defaultProps({ tree }));

    const toggleButtons = screen.getAllByLabelText("Toggle folder");
    // The first toggle is for the root tree node (workspace), second for docs
    // Click the one for the docs folder
    await fireEvent.click(toggleButtons[toggleButtons.length - 1]);
    expect(onToggleNode).toHaveBeenCalled();
  });

  it("calls onToggleCollapse when collapse button is clicked", async () => {
    render(LeftSidebar, defaultProps());

    await fireEvent.click(screen.getByLabelText("Collapse sidebar"));
    expect(onToggleCollapse).toHaveBeenCalledTimes(1);
  });

  it("shows problems panel when validation errors exist", () => {
    const validationResult = {
      errors: [
        {
          type: "BrokenPartOf",
          file: "workspace/broken.md",
          target: "missing.md",
          description: "Broken part-of reference",
          can_auto_fix: true,
          is_viewable: true,
        },
      ],
      warnings: [],
    };

    render(LeftSidebar, defaultProps({ validationResult }));
    expect(screen.getByLabelText("Workspace problems (1)")).toBeInTheDocument();
    expect(screen.getByText(/1 problem/i)).toBeInTheDocument();
  });
});
