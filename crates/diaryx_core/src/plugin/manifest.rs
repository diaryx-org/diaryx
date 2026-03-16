//! Plugin manifest types for declarative plugin metadata and UI contributions.
//!
//! Each plugin declares a [`PluginManifest`] that describes its identity,
//! capabilities, and UI contributions. The frontend reads these manifests
//! to dynamically render settings tabs, sidebar panels, command palette items, etc.

use serde::{Deserialize, Serialize};

use super::PluginId;
use crate::error::DiaryxError;
use crate::frontmatter;

/// Declarative metadata for a plugin.
///
/// Returned by [`Plugin::manifest()`](super::Plugin::manifest) and consumed
/// by the frontend to build extension-point UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct PluginManifest {
    /// Unique plugin identifier.
    pub id: PluginId,
    /// Human-readable name.
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// Short description of what this plugin does.
    pub description: String,
    /// Capabilities this plugin provides.
    pub capabilities: Vec<PluginCapability>,
    /// UI extension points contributed by this plugin.
    pub ui: Vec<UiContribution>,
    /// CLI commands contributed by this plugin.
    #[serde(default)]
    pub cli: Vec<CliCommand>,
}

/// A capability that a plugin can declare.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum PluginCapability {
    /// Listens to file lifecycle events (create, save, delete, move).
    FileEvents,
    /// Listens to workspace lifecycle events (open, close, change, commit).
    WorkspaceEvents,
    /// Handles CRDT-related commands (sync, body docs, etc.).
    CrdtCommands,
    /// Provides sync transport (WebSocket, etc.).
    SyncTransport,
    /// Provides custom commands.
    CustomCommands {
        /// Names of the custom commands this plugin handles.
        commands: Vec<String>,
    },
    /// Contributes editor extensions (TipTap nodes/marks).
    EditorExtension,
    /// Provides media format transcoding (e.g. HEIC→JPEG).
    MediaTranscoder {
        /// Supported conversion pairs as `"source:target"` strings.
        conversions: Vec<String>,
    },
}

/// A UI extension point contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "slot")]
pub enum UiContribution {
    /// A tab in the settings dialog.
    SettingsTab {
        /// Unique identifier for this tab.
        id: String,
        /// Tab label displayed in the settings sidebar.
        label: String,
        /// Optional icon name.
        icon: Option<String>,
        /// How to render this tab's content.
        ///
        /// If `component` is set, the host renders that component and ignores `fields`.
        /// Otherwise, the host renders a declarative form from `fields`.
        fields: Vec<SettingsField>,
        /// Optional component reference for rendering the tab's content.
        ///
        /// When present, the host renders this component instead of a declarative
        /// form from `fields`. Use `ComponentRef::Builtin` to reference a host-provided
        /// component (e.g., `"sync.settings"`).
        #[serde(default)]
        component: Option<ComponentRef>,
    },
    /// A tab in one of the sidebars.
    SidebarTab {
        /// Unique identifier for this tab.
        id: String,
        /// Tab label.
        label: String,
        /// Optional icon name.
        icon: Option<String>,
        /// Which sidebar this tab appears in.
        side: SidebarSide,
        /// Component reference for rendering.
        component: ComponentRef,
    },
    /// An item in the command palette.
    CommandPaletteItem {
        /// Unique identifier for this item.
        id: String,
        /// Label displayed in the palette.
        label: String,
        /// Optional group name for categorization.
        group: Option<String>,
        /// Plugin command to execute when selected.
        plugin_command: String,
    },
    /// A plugin-owned command palette surface.
    ///
    /// When present, the host renders this component instead of the built-in
    /// command palette command list.
    CommandPalette {
        /// Unique identifier for this contribution.
        id: String,
        /// Optional label shown by host UIs.
        label: Option<String>,
        /// Component reference for rendering.
        component: ComponentRef,
    },
    /// A plugin-owned context menu surface.
    ///
    /// When present, the host renders this component for the target context
    /// menu surface instead of built-in menu items.
    ContextMenu {
        /// Unique identifier for this contribution.
        id: String,
        /// Optional label shown by host UIs.
        label: Option<String>,
        /// Target menu surface this contribution owns.
        target: ContextMenuTarget,
        /// Component reference for rendering.
        component: ComponentRef,
    },
    /// A button in the editor toolbar.
    ToolbarButton {
        /// Unique identifier for this button.
        id: String,
        /// Button label / tooltip.
        label: String,
        /// Optional icon name.
        icon: Option<String>,
        /// Plugin command to execute on click.
        plugin_command: String,
    },
    /// An item in the status bar.
    StatusBarItem {
        /// Unique identifier for this item.
        id: String,
        /// Label displayed in the status bar.
        label: String,
        /// Where in the status bar this item appears.
        position: StatusBarPosition,
        /// Optional plugin command to execute on click.
        plugin_command: Option<String>,
    },
    /// A workspace provider surfaced in workspace-creation/linking flows.
    WorkspaceProvider {
        /// Unique identifier for this provider contribution.
        id: String,
        /// Label shown in provider picker UIs.
        label: String,
        /// Optional description shown in provider picker UIs.
        description: Option<String>,
    },
    /// An editor extension (TipTap node/mark) contributed by a plugin.
    ///
    /// The host generates a TipTap extension from this declaration. Atom nodes
    /// can call a guest `render_export`; inline marks and host-builtins do not
    /// require guest rendering.
    EditorExtension {
        /// Unique extension ID (becomes the TipTap node name).
        extension_id: String,
        /// What kind of editor node this creates.
        node_type: EditorNodeType,
        /// Markdown syntax delimiters for parsing and serialization.
        markdown: Box<MarkdownSyntax>,
        /// Name of the plugin's WASM export to call for rendering.
        ///
        /// Only required for atom nodes rendered by guest code.
        render_export: Option<String>,
        /// How the user edits the source content.
        ///
        /// Only required for atom nodes that expose source editing.
        edit_mode: Option<EditMode>,
        /// Component ID for `get_component_html` when `edit_mode` is `Iframe`.
        ///
        /// The host calls the plugin's `get_component_html` command with this ID
        /// to load the iframe editor HTML.
        #[serde(default)]
        iframe_component_id: Option<String>,
        /// Optional CSS to inject for rendered output.
        css: Option<String>,
        /// Optional insert command for editor menu integration.
        ///
        /// When present, the host adds a button in the appropriate editor menu
        /// (MoreStylesPicker for inline atoms/marks, BlockPicker for block
        /// atoms) to insert an empty node of this type.
        #[serde(default)]
        insert_command: Box<Option<InsertCommand>>,
        /// Optional keyboard shortcut string (e.g., `"Mod-Shift-s"`).
        #[serde(default)]
        keyboard_shortcut: Option<String>,
        /// Optional click behavior for marks (e.g., spoiler reveal/hide).
        #[serde(default)]
        click_behavior: Option<ClickBehavior>,
        /// HTML tag to render (defaults to "span" if absent).
        #[serde(default)]
        html_tag: Option<String>,
        /// Base CSS class always applied to the rendered element.
        #[serde(default)]
        base_css_class: Option<String>,
        /// Typed attributes for InlineMark extensions.
        #[serde(default)]
        attributes: Option<Vec<MarkAttribute>>,
        /// Optional toolbar configuration for InlineMark extensions.
        ///
        /// When present, the host renders a toolbar button in the bubble menu
        /// (web) and iOS keyboard toolbar with an optional attribute picker.
        #[serde(default)]
        toolbar: Box<Option<MarkToolbar>>,
    },
}

/// Toolbar configuration for InlineMark extensions.
///
/// When present, the host renders a toolbar button in the bubble menu
/// (web) and iOS keyboard toolbar. For marks with attributes that have
/// `valid_values`, the button opens a picker dropdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct MarkToolbar {
    /// Lucide icon name for the toolbar button (e.g., "highlighter").
    pub icon: String,
    /// Tooltip / accessibility label for the button.
    pub label: String,
}

/// Which sidebar a tab appears in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum SidebarSide {
    /// Left sidebar.
    Left,
    /// Right sidebar.
    Right,
}

/// Where a status bar item appears.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum StatusBarPosition {
    /// Left-aligned.
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
}

/// Which host context menu surface a plugin contribution targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum ContextMenuTarget {
    /// Context menu for entry nodes in the left sidebar file tree.
    LeftSidebarTree,
}

/// The kind of TipTap node an [`EditorExtension`](UiContribution::EditorExtension) creates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum EditorNodeType {
    /// Inline atom node (like a footnote reference).
    InlineAtom,
    /// Block atom node (like an HTML block).
    BlockAtom,
    /// Inline mark wrapping rich text (like spoiler formatting).
    InlineMark,
    /// A host-registered builtin extension too complex for declarative guest rendering.
    Builtin {
        /// Host-managed extension identifier.
        host_extension_id: String,
    },
}

/// Markdown syntax delimiters for an editor extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct MarkdownSyntax {
    /// Whether this is an inline or block-level syntax.
    pub level: MarkdownLevel,
    /// Opening delimiter (e.g., `"$"` or `"$$"`).
    pub open: String,
    /// Closing delimiter (e.g., `"$"` or `"$$"`).
    pub close: String,
    /// Optional attribute embedded in the markdown syntax.
    #[serde(default)]
    pub attribute_syntax: Option<MarkdownAttributeSyntax>,
    /// When true, the block syntax is single-line (no newlines between
    /// open/close delimiters). Used for markdown patterns like
    /// `![drawing:alt](path)` where the entire block is one line.
    #[serde(default)]
    pub single_line: bool,
}

/// How a mark attribute is embedded in the markdown delimiters.
///
/// For highlight: `=={color}text==` means the attribute appears as `{value}`
/// immediately after the opening delimiter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct MarkdownAttributeSyntax {
    /// Which attribute this syntax maps to (must match a [`MarkAttribute::name`]).
    pub attribute: String,
    /// Opening wrapper around the value in the markdown (e.g., `"{"` for `{color}`).
    pub open: String,
    /// Closing wrapper around the value in the markdown (e.g., `"}"` for `{color}`).
    pub close: String,
    /// Where the attribute appears relative to the content.
    /// `"after_open"` means between the opening delimiter and the content.
    #[serde(default = "default_after_open")]
    pub position: String,
}

fn default_after_open() -> String {
    "after_open".into()
}

/// A typed attribute on an InlineMark extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct MarkAttribute {
    /// Attribute name (becomes the TipTap attribute key, e.g., "color").
    pub name: String,
    /// Default value when not explicitly set.
    pub default: String,
    /// HTML attribute name for storage (e.g., "data-highlight-color").
    pub html_attribute: String,
    /// Allowed values. If non-empty, input/paste rules reject unknown values.
    #[serde(default)]
    pub valid_values: Vec<String>,
    /// If set, the factory generates `class="{prefix}{value}"` on rendered HTML.
    #[serde(default)]
    pub css_class_prefix: Option<String>,
}

/// Whether a markdown syntax is inline or block-level.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum MarkdownLevel {
    /// Inline-level (within a paragraph).
    Inline,
    /// Block-level (standalone paragraph).
    Block,
}

/// How the user edits the source content of an editor extension node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum EditMode {
    /// Click opens a popover with a source text input (for inline nodes).
    Popover,
    /// Click toggles between source textarea and rendered preview (for block nodes).
    SourceToggle,
    /// Content is edited inside a sandboxed iframe loaded from the plugin's
    /// `get_component_html` command. Used for rich interactive editors like drawing.
    Iframe,
}

/// Where an insert command should appear in the editor UI.
///
/// Controls which menus/surfaces render the insert button. Defaults to
/// `BlockPicker` so that block atoms only appear in the block picker ("+"
/// menu on web, block picker dropdown on iOS) and not in the block style
/// picker or as a standalone toolbar button.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum InsertCommandPlacement {
    /// Only appear in the block/inline picker menu (default for block atoms).
    #[default]
    Picker,
    /// Appear in both the picker and the block style picker (BubbleMenu dropdown).
    PickerAndStylePicker,
    /// Appear everywhere: picker, style picker, and as a standalone toolbar button.
    All,
}

/// Metadata for an insert button in the editor menus.
///
/// When present on an [`EditorExtension`](UiContribution::EditorExtension),
/// the host renders a button in the appropriate menu (MoreStylesPicker for
/// inline atoms/marks, BlockPicker for block atoms).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct InsertCommand {
    /// Button label shown in the menu.
    pub label: String,
    /// Lucide icon name in kebab-case (e.g., `"sigma"`, `"square-sigma"`).
    /// Falls back to a generic plugin icon if unrecognized.
    pub icon: Option<String>,
    /// Tooltip / description for the button.
    pub description: Option<String>,
    /// Where this insert command should appear in the UI.
    ///
    /// Defaults to [`InsertCommandPlacement::Picker`] so that block atoms
    /// only show in the block picker and not duplicated in other surfaces.
    #[serde(default)]
    pub placement: InsertCommandPlacement,
}

/// Click behavior for inline mark extensions.
///
/// When present, the host registers a ProseMirror plugin that toggles CSS
/// classes on click (e.g., spoiler reveal/hide).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum ClickBehavior {
    /// Toggle between a hidden and revealed CSS class on click.
    ToggleClass {
        /// CSS class applied when the mark content is hidden.
        hidden_class: String,
        /// CSS class applied when the mark content is revealed.
        revealed_class: String,
    },
}

/// How to render a plugin-contributed UI panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type")]
pub enum ComponentRef {
    /// Use an existing built-in component by ID.
    Builtin {
        /// ID of the built-in component to render.
        component_id: String,
    },
    /// Render a form from declarative field definitions.
    Declarative {
        /// Fields to render as form controls.
        fields: Vec<SettingsField>,
    },
    /// Render plugin-provided HTML in a sandboxed iframe.
    ///
    /// The host calls the guest's `get_component_html` export with the
    /// given `component_id` to obtain the HTML content.
    Iframe {
        /// Identifier passed to the guest export to retrieve the HTML.
        component_id: String,
    },
}

/// A declarative settings field rendered as a form control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type")]
pub enum SettingsField {
    /// Text input.
    Text {
        /// Config key this field writes to.
        key: String,
        /// Label displayed next to the input.
        label: String,
        /// Optional description / help text.
        description: Option<String>,
        /// Optional placeholder text.
        #[serde(default)]
        placeholder: Option<String>,
    },
    /// Password input (rendered as `type="password"`).
    Password {
        /// Config key this field writes to.
        key: String,
        /// Label displayed next to the input.
        label: String,
        /// Optional description / help text.
        description: Option<String>,
        /// Optional placeholder text.
        #[serde(default)]
        placeholder: Option<String>,
    },
    /// Boolean toggle.
    Toggle {
        /// Config key this field writes to.
        key: String,
        /// Label displayed next to the toggle.
        label: String,
        /// Optional description / help text.
        description: Option<String>,
    },
    /// Dropdown select.
    Select {
        /// Config key this field writes to.
        key: String,
        /// Label displayed above the select.
        label: String,
        /// Available options.
        options: Vec<SelectOption>,
        /// Optional description / help text.
        description: Option<String>,
    },
    /// Numeric input.
    Number {
        /// Config key this field writes to.
        key: String,
        /// Label displayed next to the input.
        label: String,
        /// Optional minimum value.
        min: Option<f64>,
        /// Optional maximum value.
        max: Option<f64>,
    },
    /// Section header (non-interactive).
    Section {
        /// Section heading text.
        label: String,
        /// Optional description.
        description: Option<String>,
    },
    /// Action button that dispatches a plugin command.
    Button {
        /// Button label.
        label: String,
        /// Plugin command to dispatch on click.
        command: String,
        /// Button style variant: `"default"`, `"outline"`, or `"destructive"`.
        #[serde(default)]
        variant: Option<String>,
    },
    /// Action button that triggers a host-managed action instead of a plugin
    /// command.
    HostActionButton {
        /// Button label.
        label: String,
        /// Host action type to invoke.
        action_type: String,
        /// Button style variant: `"default"`, `"outline"`, or `"destructive"`.
        #[serde(default)]
        variant: Option<String>,
    },
    /// Host sign-in form. Renders inline email/magic-link sign-in when not
    /// authenticated, or account status when authenticated.
    AuthStatus {
        /// Label displayed above the sign-in form.
        label: String,
        /// Optional description / help text.
        description: Option<String>,
    },
    /// Host upgrade banner. Only visible when the user is not on Plus tier.
    /// Renders sign-in prompt if not authenticated, or purchase button if
    /// authenticated but free tier.
    UpgradeBanner {
        /// Feature name requiring Plus.
        feature: String,
        /// Optional description for the upgrade banner.
        description: Option<String>,
    },
    /// Conditional field group. Shows nested fields only when a host
    /// condition is met. Supported conditions include auth checks like
    /// `"authenticated"`, `"plus"`, `"not_authenticated"`,
    /// `"not_plus"`, plus config comparisons like
    /// `"config:import_format=dayone"`.
    Conditional {
        /// Condition string to evaluate.
        condition: String,
        /// Nested fields to render when condition is met.
        fields: Vec<SettingsField>,
    },
    /// Host-managed widget embedded within a declarative panel.
    HostWidget {
        /// Identifier of the host widget to render.
        widget_id: String,
    },
}

/// A select dropdown option.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct SelectOption {
    /// The value stored when selected.
    pub value: String,
    /// The label displayed to the user.
    pub label: String,
}

// ============================================================================
// CLI command types
// ============================================================================

fn default_true() -> bool {
    true
}

/// A CLI subcommand declared by a plugin. The CLI host builds
/// dynamic clap commands from these declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct CliCommand {
    /// Subcommand name (e.g., `"publish"`).
    pub name: String,
    /// Short help text shown in `--help`.
    pub about: String,
    /// Longer help text (shown with `--help` on the subcommand itself).
    #[serde(default)]
    pub long_about: Option<String>,
    /// Alternative names for this command (e.g., `["pub"]`).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Positional and named arguments.
    #[serde(default)]
    pub args: Vec<CliArg>,
    /// Nested subcommands.
    #[serde(default)]
    pub subcommands: Vec<CliCommand>,
    /// Internal command name sent to `handle_command`.
    /// Defaults to PascalCase of `name` if absent.
    #[serde(default)]
    pub command_name: Option<String>,
    /// If `true`, the CLI resolves the workspace root and passes it.
    #[serde(default = "default_true")]
    pub requires_workspace: bool,
    /// Use a native CLI handler instead of WASM dispatch.
    /// Value is the handler ID (e.g., `"sync_start"`, `"preview"`).
    #[serde(default)]
    pub native_handler: Option<String>,
}

/// A CLI argument declared by a plugin command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct CliArg {
    /// Argument name (used as the clap ID).
    pub name: String,
    /// Help text.
    pub help: String,
    /// Value type for parsing.
    #[serde(default)]
    pub value_type: CliArgType,
    /// Whether this argument is required.
    #[serde(default)]
    pub required: bool,
    /// Default value as a string.
    #[serde(default)]
    pub default_value: Option<String>,
    /// Single-character short flag (e.g., `'p'` for `-p`).
    #[serde(default)]
    pub short: Option<char>,
    /// Long flag name (e.g., `"port"` for `--port`).
    #[serde(default)]
    pub long: Option<String>,
    /// If `true`, this is a boolean flag (no value needed).
    #[serde(default)]
    pub is_flag: bool,
}

/// Value types for CLI arguments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum CliArgType {
    /// String value (default).
    #[default]
    String,
    /// Integer value.
    Integer,
    /// Floating-point value.
    Float,
    /// Boolean value.
    Boolean,
    /// Filesystem path.
    Path,
}

// ============================================================================
// Marketplace types
// ============================================================================

/// Reference to a WASM build artifact on the CDN.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginArtifact {
    /// CDN URL for the WASM file.
    pub url: String,
    /// SHA-256 hash of the WASM file.
    pub sha256: String,
    /// File size in bytes.
    pub size: u64,
    /// ISO 8601 timestamp of when the artifact was published.
    pub published_at: String,
}

/// A single plugin listing in the marketplace registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceEntry {
    /// Canonical plugin ID (e.g., `"diaryx.sync"`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// One-line summary.
    pub summary: String,
    /// Full description.
    pub description: String,
    /// Author or organization.
    pub author: String,
    /// License identifier.
    pub license: String,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Category tags for discovery.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Free-form tags for search.
    #[serde(default)]
    pub tags: Vec<String>,
    /// WASM artifact reference.
    pub artifact: PluginArtifact,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Icon URL.
    #[serde(default)]
    pub icon: Option<String>,
    /// Screenshot URLs.
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// Requested default permissions (opaque JSON).
    #[serde(default)]
    pub requested_permissions: Option<serde_json::Value>,
    /// Protocol version this plugin was built against.
    #[serde(default)]
    pub protocol_version: Option<u32>,
}

/// The parsed CDN registry (`registry.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceRegistry {
    /// Schema version (must be `2`).
    pub schema_version: u64,
    /// ISO 8601 timestamp of when the registry was generated.
    pub generated_at: String,
    /// Plugin listings.
    pub plugins: Vec<MarketplaceEntry>,
    /// Markdown body after the frontmatter.
    #[serde(skip)]
    pub body: String,
}

/// Metadata parsed from a plugin workspace root `README.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginWorkspaceMetadata {
    /// Canonical plugin ID.
    pub id: String,
    /// Human-readable name (from `title` frontmatter key).
    pub name: String,
    /// SemVer version.
    pub version: String,
    /// Short description (from `description` frontmatter key).
    pub summary: String,
    /// Author or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// License identifier.
    #[serde(default)]
    pub license: Option<String>,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Category tags.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// WASM artifact reference.
    pub artifact: PluginArtifact,
    /// UI contributions (opaque JSON, preserved from frontmatter).
    #[serde(default)]
    pub ui: Option<serde_json::Value>,
    /// CLI commands (opaque JSON, preserved from frontmatter).
    #[serde(default)]
    pub cli: Option<serde_json::Value>,
    /// Requested permissions (opaque JSON).
    #[serde(default)]
    pub requested_permissions: Option<serde_json::Value>,
    /// Protocol version this plugin was built against.
    #[serde(default)]
    pub protocol_version: Option<u32>,
    /// Markdown body after the frontmatter.
    #[serde(skip)]
    pub body: String,
}

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
fn yaml_to_json(yaml: &serde_yaml::Value) -> Result<serde_json::Value, DiaryxError> {
    let json_str = serde_json::to_string(
        &serde_yaml::from_value::<serde_json::Value>(yaml.clone())
            .map_err(|e| DiaryxError::Validation(format!("YAML→JSON conversion failed: {e}")))?,
    )
    .map_err(|e| DiaryxError::Validation(format!("JSON serialization failed: {e}")))?;
    serde_json::from_str(&json_str)
        .map_err(|e| DiaryxError::Validation(format!("JSON round-trip failed: {e}")))
}

impl MarketplaceRegistry {
    /// Parse a `registry.md` file (YAML frontmatter + markdown body).
    pub fn from_markdown(content: &str) -> Result<Self, DiaryxError> {
        let parsed = frontmatter::parse(content)?;

        let schema_version = parsed
            .frontmatter
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                DiaryxError::Validation(
                    "Registry missing or invalid schema_version (expected 2)".to_string(),
                )
            })?;

        if schema_version != 2 {
            return Err(DiaryxError::Validation(format!(
                "Unsupported registry schema_version: {schema_version} (expected 2)"
            )));
        }

        let generated_at = parsed
            .frontmatter
            .get("generated_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DiaryxError::Validation("Registry missing generated_at".to_string()))?
            .to_string();

        let plugins_yaml = parsed
            .frontmatter
            .get("plugins")
            .ok_or_else(|| DiaryxError::Validation("Registry missing plugins array".to_string()))?;

        let plugins_json = yaml_to_json(plugins_yaml)?;
        let plugins: Vec<MarketplaceEntry> = serde_json::from_value(plugins_json)
            .map_err(|e| DiaryxError::Validation(format!("Failed to parse plugins: {e}")))?;

        for plugin in &plugins {
            validate_marketplace_entry(plugin)?;
        }

        Ok(MarketplaceRegistry {
            schema_version,
            generated_at,
            plugins,
            body: parsed.body,
        })
    }
}

impl PluginWorkspaceMetadata {
    /// Parse a plugin workspace root `README.md` (YAML frontmatter + markdown body).
    pub fn from_markdown(content: &str) -> Result<Self, DiaryxError> {
        let parsed = frontmatter::parse(content)?;
        let fm = &parsed.frontmatter;

        let id = fm
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DiaryxError::Validation("Plugin workspace missing 'id'".to_string()))?
            .to_string();

        let name = fm
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DiaryxError::Validation("Plugin workspace missing 'title'".to_string()))?
            .to_string();

        let version = fm
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DiaryxError::Validation("Plugin workspace missing 'version'".to_string())
            })?
            .to_string();

        let summary = fm
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let author = fm
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let license = fm
            .get("license")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let repository = fm
            .get("repository")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let categories = yaml_string_array(fm.get("categories"));
        let tags = yaml_string_array(fm.get("tags"));
        let capabilities = yaml_string_array(fm.get("capabilities"));

        let artifact_yaml = fm.get("artifact").ok_or_else(|| {
            DiaryxError::Validation("Plugin workspace missing 'artifact'".to_string())
        })?;
        let artifact_json = yaml_to_json(artifact_yaml)?;
        let artifact: PluginArtifact = serde_json::from_value(artifact_json)
            .map_err(|e| DiaryxError::Validation(format!("Failed to parse artifact: {e}")))?;

        let ui = fm.get("ui").map(yaml_to_json).transpose()?;
        let cli = fm.get("cli").map(yaml_to_json).transpose()?;
        let requested_permissions = fm
            .get("requested_permissions")
            .map(yaml_to_json)
            .transpose()?;

        let protocol_version = fm
            .get("protocol_version")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        Ok(PluginWorkspaceMetadata {
            id,
            name,
            version,
            summary,
            author,
            license,
            repository,
            categories,
            tags,
            capabilities,
            artifact,
            ui,
            cli,
            requested_permissions,
            protocol_version,
            body: parsed.body,
        })
    }

    /// Convert to a [`MarketplaceEntry`] for registry assembly.
    pub fn to_marketplace_entry(&self) -> MarketplaceEntry {
        MarketplaceEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            summary: self.summary.clone(),
            description: self.body.trim().to_string(),
            author: self.author.clone().unwrap_or_default(),
            license: self.license.clone().unwrap_or_default(),
            repository: self.repository.clone(),
            categories: self.categories.clone(),
            tags: self.tags.clone(),
            artifact: self.artifact.clone(),
            capabilities: self.capabilities.clone(),
            icon: None,
            screenshots: Vec::new(),
            requested_permissions: self.requested_permissions.clone(),
            protocol_version: self.protocol_version,
        }
    }
}

/// Extract a string array from an optional YAML value.
fn yaml_string_array(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Validate a marketplace entry has required fields.
fn validate_marketplace_entry(entry: &MarketplaceEntry) -> Result<(), DiaryxError> {
    if entry.id.trim().is_empty() {
        return Err(DiaryxError::Validation(
            "Marketplace entry has empty id".to_string(),
        ));
    }
    if entry.version.trim().is_empty() {
        return Err(DiaryxError::Validation(format!(
            "Marketplace entry '{}' has empty version",
            entry.id
        )));
    }
    if entry.artifact.url.trim().is_empty() {
        return Err(DiaryxError::Validation(format!(
            "Marketplace entry '{}' has empty artifact.url",
            entry.id
        )));
    }
    if entry.artifact.sha256.trim().is_empty() {
        return Err(DiaryxError::Validation(format!(
            "Marketplace entry '{}' has empty artifact.sha256",
            entry.id
        )));
    }
    if entry.artifact.size == 0 {
        return Err(DiaryxError::Validation(format!(
            "Marketplace entry '{}' has artifact.size=0",
            entry.id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REGISTRY_MD: &str = r#"---
title: "Diaryx Plugin Registry"
description: "Official plugin directory"
generated_at: "2026-03-03T00:00:00Z"
schema_version: 2
plugins:
  - id: "diaryx.sync"
    name: "Sync"
    version: "1.2.3"
    summary: "Realtime multi-device sync"
    description: "Full description of sync plugin"
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx-sync"
    categories: ["sync", "collaboration"]
    tags: ["sync", "crdt", "realtime"]
    artifact:
      url: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc123.wasm"
      sha256: "abc123"
      size: 2048000
      published_at: "2026-03-03T00:00:00Z"
    capabilities: ["sync_transport"]
    icon: null
    screenshots: []
    requested_permissions: null
---
# Diaryx Plugin Registry
Browse and install plugins for Diaryx.
"#;

    const SAMPLE_PLUGIN_README: &str = r#"---
title: "Sync"
description: "Realtime multi-device sync"
id: "diaryx.sync"
version: "1.2.3"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx-sync"
categories: ["sync", "collaboration"]
tags: ["sync", "crdt", "realtime"]
capabilities: ["sync_transport", "crdt_commands"]
artifact:
  url: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc123.wasm"
  sha256: "abc123"
  size: 2048000
  published_at: "2026-03-03T00:00:00Z"
ui:
  - slot: WorkspaceProvider
    id: diaryx.sync
    label: "Diaryx Cloud"
cli:
  - name: sync
    about: "Sync workspace"
requested_permissions:
  defaults:
    http_requests:
      include: ["api.diaryx.org"]
  reasons:
    http_requests: "Connect to sync server"
---
# Sync Plugin
Full description in markdown body...
"#;

    #[test]
    fn parse_registry_md() {
        let registry = MarketplaceRegistry::from_markdown(SAMPLE_REGISTRY_MD).unwrap();
        assert_eq!(registry.schema_version, 2);
        assert_eq!(registry.plugins.len(), 1);
        assert_eq!(registry.plugins[0].id, "diaryx.sync");
        assert_eq!(registry.plugins[0].name, "Sync");
        assert_eq!(registry.plugins[0].version, "1.2.3");
        assert_eq!(registry.plugins[0].author, "Diaryx Team");
        assert_eq!(registry.plugins[0].artifact.size, 2048000);
        assert!(registry.body.contains("Browse and install"));
    }

    #[test]
    fn parse_plugin_workspace_readme() {
        let meta = PluginWorkspaceMetadata::from_markdown(SAMPLE_PLUGIN_README).unwrap();
        assert_eq!(meta.id, "diaryx.sync");
        assert_eq!(meta.name, "Sync");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.summary, "Realtime multi-device sync");
        assert_eq!(meta.author.as_deref(), Some("Diaryx Team"));
        assert_eq!(meta.categories, vec!["sync", "collaboration"]);
        assert_eq!(meta.artifact.sha256, "abc123");
        assert!(meta.ui.is_some());
        assert!(meta.cli.is_some());
        assert!(meta.requested_permissions.is_some());
        assert!(meta.body.contains("Full description"));
    }

    #[test]
    fn plugin_workspace_to_marketplace_entry() {
        let meta = PluginWorkspaceMetadata::from_markdown(SAMPLE_PLUGIN_README).unwrap();
        let entry = meta.to_marketplace_entry();
        assert_eq!(entry.id, "diaryx.sync");
        assert_eq!(entry.name, "Sync");
        assert_eq!(entry.author, "Diaryx Team");
        assert_eq!(entry.artifact.url, meta.artifact.url);
        assert!(entry.description.contains("Full description"));
    }

    #[test]
    fn reject_wrong_schema_version() {
        let content = "---\nschema_version: 1\ngenerated_at: \"2026-01-01\"\nplugins: []\n---\n";
        let err = MarketplaceRegistry::from_markdown(content).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected 2"), "got: {msg}");
    }

    #[test]
    fn reject_missing_id() {
        let content = r#"---
schema_version: 2
generated_at: "2026-01-01"
plugins:
  - name: "Test"
    version: "1.0.0"
    summary: "Test"
    description: "Test"
    author: "Test"
    license: "MIT"
    artifact:
      url: "https://example.com/test.wasm"
      sha256: "abc"
      size: 100
      published_at: "2026-01-01"
---
"#;
        let err = MarketplaceRegistry::from_markdown(content).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing") || msg.contains("id"), "got: {msg}");
    }

    #[test]
    fn reject_missing_artifact_in_workspace() {
        let content = "---\ntitle: Test\nid: test.plugin\nversion: \"1.0.0\"\n---\nBody\n";
        let err = PluginWorkspaceMetadata::from_markdown(content).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("artifact"), "got: {msg}");
    }

    #[test]
    fn roundtrip_marketplace_entry() {
        let entry = MarketplaceEntry {
            id: "test.plugin".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            summary: "A test plugin".to_string(),
            description: "Longer description".to_string(),
            author: "Tester".to_string(),
            license: "MIT".to_string(),
            repository: Some("https://example.com".to_string()),
            categories: vec!["test".to_string()],
            tags: vec!["example".to_string()],
            artifact: PluginArtifact {
                url: "https://example.com/test.wasm".to_string(),
                sha256: "abc123".to_string(),
                size: 1024,
                published_at: "2026-03-03T00:00:00Z".to_string(),
            },
            capabilities: vec!["custom".to_string()],
            icon: None,
            screenshots: vec![],
            requested_permissions: None,
            protocol_version: Some(1),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MarketplaceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn parse_inline_mark_editor_extension_contribution() {
        let contribution: UiContribution = serde_json::from_value(serde_json::json!({
            "slot": "EditorExtension",
            "extension_id": "spoiler",
            "node_type": "InlineMark",
            "markdown": {
                "level": "Inline",
                "open": "||",
                "close": "||"
            },
            "render_export": null,
            "edit_mode": null,
            "css": ".spoiler { color: transparent; }",
            "keyboard_shortcut": "Mod-Shift-s",
            "click_behavior": {
                "ToggleClass": {
                    "hidden_class": "spoiler-hidden",
                    "revealed_class": "spoiler-revealed"
                }
            },
            "insert_command": {
                "label": "Spoiler",
                "icon": "eye-off",
                "description": "Hide text behind a spoiler"
            }
        }))
        .unwrap();

        match contribution {
            UiContribution::EditorExtension {
                extension_id,
                node_type,
                render_export,
                edit_mode,
                insert_command,
                keyboard_shortcut,
                click_behavior,
                ..
            } => {
                assert_eq!(extension_id, "spoiler");
                assert!(matches!(node_type, EditorNodeType::InlineMark));
                assert_eq!(render_export, None);
                assert!(edit_mode.is_none());
                assert_eq!(insert_command.unwrap().label, "Spoiler");
                assert_eq!(keyboard_shortcut.as_deref(), Some("Mod-Shift-s"));
                assert!(matches!(
                    click_behavior,
                    Some(ClickBehavior::ToggleClass { .. })
                ));
            }
            other => panic!("expected editor extension, got {other:?}"),
        }
    }

    #[test]
    fn parse_builtin_editor_node_type() {
        let node_type: EditorNodeType = serde_json::from_value(serde_json::json!({
            "Builtin": {
                "host_extension_id": "templateVariable"
            }
        }))
        .unwrap();

        match node_type {
            EditorNodeType::Builtin { host_extension_id } => {
                assert_eq!(host_extension_id, "templateVariable");
            }
            other => panic!("expected builtin node type, got {other:?}"),
        }
    }
}
