import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

vi.stubGlobal("__APP_VERSION__", "0.0.0-test");

// ---------------------------------------------------------------------------
// Stub loader helpers
// ---------------------------------------------------------------------------

async function loadNoopStub() {
  return (await import("./test/NoopStub.svelte")).default;
}

async function loadSnippetChildStub() {
  return (await import("./test/SnippetChildStub.svelte")).default;
}

// ---------------------------------------------------------------------------
// Mocks – must be hoisted before the component import
// ---------------------------------------------------------------------------

// --- Backend & window drag ---
vi.mock("./backend", () => ({
  maybeStartWindowDrag: vi.fn(),
}));

vi.mock("./windowDrag", () => ({
  maybeStartWindowDrag: vi.fn(),
}));

vi.mock("$lib/backend/api", () => ({}));

// --- UI component stubs ---
vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("./test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/input", async () => ({
  Input: (await import("./test/InputStub.svelte")).default,
}));

vi.mock("$lib/components/ui/alert", async () => {
  const Stub = await loadNoopStub();
  return { Root: Stub, Description: Stub };
});

vi.mock("$lib/components/ui/dropdown-menu", async () => {
  const Stub = await loadNoopStub();
  const SnippetStub = await loadSnippetChildStub();
  return {
    Root: Stub,
    Trigger: SnippetStub,
    Content: Stub,
    Item: Stub,
    Separator: Stub,
  };
});

vi.mock("$lib/components/ui/tooltip", async () => {
  const Stub = await loadNoopStub();
  return { Root: Stub, Trigger: Stub, Content: Stub };
});

vi.mock("$lib/components/ui/kbd", async () => {
  const Stub = await loadNoopStub();
  return { Root: Stub, Group: Stub };
});

// --- Component stubs ---
vi.mock("$lib/components/FilePickerPopover.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("$lib/components/DocumentAudiencePill.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("$lib/components/NestedObjectDisplay.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("$lib/components/PluginSidebarPanel.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("$lib/components/UpgradeBanner.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("$lib/components/MoveConfigDialog.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

vi.mock("./history/VersionDiff.svelte", async () => ({
  default: (await import("./test/NoopStub.svelte")).default,
}));

// --- Hooks / stores ---
vi.mock("$lib/hooks/useMobile.svelte", () => ({
  getMobileState: () => ({ isMobile: false }),
}));

vi.mock("@/models/stores/workspaceStore.svelte", () => ({
  workspaceStore: { tree: null },
}));

vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: () => ({ rightSidebarTabs: [] }),
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  getPlugin: () => null,
}));

vi.mock("$lib/auth", () => ({
  getAuthState: () => ({ isAuthenticated: false, user: null }),
}));

// --- Services ---
vi.mock("$lib/sync/attachmentSyncService", () => ({
  getAttachmentMetadata: vi.fn(() => null),
  enqueueAttachmentDownload: vi.fn(),
  isAttachmentSyncEnabled: vi.fn(() => false),
}));

vi.mock("@/models/services/attachmentService", () => ({
  getAttachmentAvailability: vi.fn(async () => "unknown"),
  getAttachmentMediaKind: vi.fn(() => "other"),
  getAttachmentThumbnailUrl: vi.fn(async () => null),
  isPreviewableAttachmentKind: vi.fn(() => false),
}));

vi.mock("$lib/utils/linkParser", () => ({
  parseLinkDisplay: vi.fn(() => null),
}));

vi.mock("$lib/backend/generated/serde_json/JsonValue", () => ({}));

// --- Lucide icons: proxy that returns a stub for every named export ---
vi.mock("@lucide/svelte", async () => {
  const Stub = (await import("./test/IconStub.svelte")).default;
  const cache: Record<string, unknown> = {};
  return new Proxy(
    {} as Record<string, unknown>,
    {
      get(_target, prop) {
        if (typeof prop !== "string") return undefined;
        // Skip special module-interop keys
        if (prop === "__esModule" || prop === "default" || prop === "then") return undefined;
        if (!cache[prop]) cache[prop] = Stub;
        return cache[prop];
      },
      has(_target, prop) {
        return typeof prop === "string";
      },
    },
  );
});

// ---------------------------------------------------------------------------
// Import the component AFTER all mocks are set up
// ---------------------------------------------------------------------------
import RightSidebar from "./RightSidebar.svelte";

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------
function createMockEntry(overrides: Partial<any> = {}) {
  return {
    path: "workspace/test-entry.md",
    title: "Test Entry",
    content: "# Test\nSome content.",
    frontmatter: {
      title: "Test Entry",
      tags: ["test", "example"],
      ...overrides.frontmatter,
    },
    attachments: [],
    ...overrides,
  };
}

function renderSidebar(propOverrides: Record<string, unknown> = {}) {
  const onToggleCollapse = vi.fn();
  const props = {
    entry: null,
    collapsed: false,
    onToggleCollapse,
    ...propOverrides,
  };
  const result = render(RightSidebar, { props });
  return { ...result, onToggleCollapse };
}

function padDatePart(value: number): string {
  return value.toString().padStart(2, "0");
}

function formatLocalDateTimeInput(date: Date): string {
  const year = date.getFullYear();
  const month = padDatePart(date.getMonth() + 1);
  const day = padDatePart(date.getDate());
  const hour = padDatePart(date.getHours());
  const minute = padDatePart(date.getMinutes());
  return `${year}-${month}-${day}T${hour}:${minute}`;
}

function formatLocalRfc3339(date: Date): string {
  const localDateTime = `${formatLocalDateTimeInput(date)}:${padDatePart(date.getSeconds())}`;
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const offsetHours = padDatePart(Math.floor(absoluteOffset / 60));
  const offsetRemainder = padDatePart(absoluteOffset % 60);
  return `${localDateTime}${sign}${offsetHours}:${offsetRemainder}`;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
describe("RightSidebar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders empty state when entry is null", () => {
    renderSidebar({ entry: null });

    expect(screen.getByText("No entry selected")).toBeInTheDocument();
    expect(screen.getByText("Select an entry to view its properties")).toBeInTheDocument();
  });

  it("renders entry properties when entry is provided", () => {
    const entry = createMockEntry();
    renderSidebar({ entry });

    // The frontmatter key "title" should appear (formatted as "Title")
    expect(screen.getByText("Title")).toBeInTheDocument();
    // The "tags" key should appear (formatted as "Tags")
    expect(screen.getByText("Tags")).toBeInTheDocument();
  });

  it("calls onToggleCollapse when collapse button is clicked", async () => {
    const { onToggleCollapse } = renderSidebar({ entry: createMockEntry() });

    const collapseBtn = screen.getByRole("button", { name: "Collapse panel" });
    await fireEvent.click(collapseBtn);

    expect(onToggleCollapse).toHaveBeenCalledTimes(1);
  });

  it("displays title error when titleError prop is set", () => {
    const entry = createMockEntry();
    renderSidebar({
      entry,
      titleError: "Title already exists",
    });

    expect(screen.getByText("Title already exists")).toBeInTheDocument();
  });

  it("shows attachment list when entry has attachments", () => {
    const entry = createMockEntry({
      frontmatter: {
        title: "Test Entry",
        attachments: ["photos/image.png", "docs/readme.pdf"],
      },
    });
    renderSidebar({ entry });

    // Attachment filenames should appear
    expect(screen.getByText("image.png")).toBeInTheDocument();
    expect(screen.getByText("readme.pdf")).toBeInTheDocument();
  });

  it("calls onDeleteAttachment when remove button is clicked", async () => {
    const onDeleteAttachment = vi.fn();
    const entry = createMockEntry({
      frontmatter: {
        title: "Test Entry",
        attachments: ["photos/image.png"],
      },
    });
    renderSidebar({ entry, onDeleteAttachment });

    const removeBtn = screen.getByRole("button", { name: "Remove attachment" });
    await fireEvent.click(removeBtn);

    expect(onDeleteAttachment).toHaveBeenCalledWith("photos/image.png");
  });

  it("calls onPropertyChange when a property is edited", async () => {
    const onPropertyChange = vi.fn();
    const entry = createMockEntry({
      frontmatter: {
        title: "Test Entry",
      },
    });
    renderSidebar({ entry, onPropertyChange });

    // The title input should be rendered with the current value
    const titleInput = screen.getByDisplayValue("Test Entry");
    expect(titleInput).toBeInTheDocument();

    // Simulate editing the title
    await fireEvent.input(titleInput, { target: { value: "New Title" } });
    await fireEvent.blur(titleInput);

    expect(onPropertyChange).toHaveBeenCalledWith("title", "New Title");
  });

  it("preserves local wall time for datetime frontmatter fields", async () => {
    const onPropertyChange = vi.fn();
    const initialDate = new Date(2026, 2, 29, 19, 0, 0);
    const updatedDate = new Date(2026, 2, 29, 20, 15, 0);
    const entry = createMockEntry({
      frontmatter: {
        title: "Test Entry",
        updated: formatLocalRfc3339(initialDate),
      },
    });
    const { container } = renderSidebar({ entry, onPropertyChange });

    const updatedInput = container.querySelector('input[type="datetime-local"]') as HTMLInputElement;
    expect(updatedInput).not.toBeNull();
    expect(updatedInput.value).toBe(formatLocalDateTimeInput(initialDate));

    await fireEvent.change(updatedInput, {
      target: { value: formatLocalDateTimeInput(updatedDate) },
    });

    expect(onPropertyChange).toHaveBeenCalledWith("updated", formatLocalRfc3339(updatedDate));
  });

  it("displays attachment error message", () => {
    const entry = createMockEntry({
      frontmatter: {
        title: "Test Entry",
        attachments: ["file.txt"],
      },
    });
    renderSidebar({
      entry,
      attachmentError: "Failed to upload attachment",
    });

    expect(screen.getByText("Failed to upload attachment")).toBeInTheDocument();
  });
});
