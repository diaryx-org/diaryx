//! Plugin manifest types for declarative plugin metadata and UI contributions.
//!
//! Each plugin declares a [`PluginManifest`] that describes its identity,
//! capabilities, and UI contributions. The frontend reads these manifests
//! to dynamically render settings tabs, sidebar panels, command palette items, etc.
//!
//! # Marketplace Types
//!
//! This module also contains types for the plugin marketplace:
//! - [`PluginArtifact`] — WASM build artifact reference (URL, SHA-256, size)
//! - [`MarketplaceEntry`] — a single plugin listing in the registry
//! - [`MarketplaceRegistry`] — the parsed CDN registry (`registry.md`)
//! - [`PluginWorkspaceMetadata`] — metadata parsed from a plugin workspace root

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
    /// CLI subcommands contributed by this plugin.
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
    /// A dialog that can be triggered by a plugin command.
    ///
    /// The host renders the component as a modal dialog. Plugins use this
    /// for complex multi-step flows (e.g., sync setup wizard).
    Dialog {
        /// Unique identifier for this dialog.
        id: String,
        /// Dialog title / label.
        label: String,
        /// Component reference for rendering the dialog content.
        component: ComponentRef,
        /// Optional plugin command that triggers this dialog.
        /// If set, the host opens the dialog when this command is executed.
        trigger_command: Option<String>,
    },
    /// A workspace provider contributed by a plugin.
    ///
    /// Plugins declaring this slot appear in workspace creation/management UIs
    /// as sync providers. The host queries provider readiness and delegates
    /// link/unlink/download operations to the provider.
    WorkspaceProvider {
        /// Unique provider identifier (usually the plugin ID).
        id: String,
        /// Human-readable label shown in provider dropdowns.
        label: String,
        /// Optional icon name (Lucide kebab-case).
        icon: Option<String>,
    },
    /// A storage provider contributed by a plugin.
    ///
    /// Plugins declaring this slot appear in the storage settings UI
    /// as alternative filesystem backends. The host creates a
    /// `JsFileSystem`-backed `DiaryxBackend` that delegates I/O to the plugin.
    StorageProvider {
        /// Unique provider identifier (usually the plugin ID).
        id: String,
        /// Human-readable label shown in storage picker.
        label: String,
        /// Optional icon name (Lucide kebab-case).
        icon: Option<String>,
        /// Optional description shown below the label.
        description: Option<String>,
    },
    /// An editor extension (TipTap node/mark) contributed by a plugin.
    ///
    /// The host generates a TipTap extension from this declaration and calls
    /// the plugin's `render_export` function to render content (for atom nodes).
    /// For marks (`InlineMark`), no render export is needed — the host wraps
    /// inline content directly.
    EditorExtension {
        /// Unique extension ID (becomes the TipTap node/mark name).
        extension_id: String,
        /// What kind of editor node this creates.
        node_type: EditorNodeType,
        /// Markdown syntax delimiters for parsing and serialization.
        markdown: MarkdownSyntax,
        /// Name of the plugin's WASM export to call for rendering.
        /// Required for atom nodes (`InlineAtom`, `BlockAtom`), unused for marks.
        #[serde(default)]
        render_export: Option<String>,
        /// How the user edits the source content.
        /// Required for atom nodes, unused for marks.
        #[serde(default)]
        edit_mode: Option<EditMode>,
        /// Optional CSS to inject for rendered output.
        css: Option<String>,
        /// Optional insert command for editor menu integration.
        ///
        /// When present, the host adds a button in the appropriate editor menu
        /// (MoreStylesPicker for inline atoms/marks, BlockPicker for block atoms)
        /// to insert or toggle this extension.
        #[serde(default)]
        insert_command: Option<InsertCommand>,
        /// Optional keyboard shortcut (e.g., `"Mod-Shift-s"`).
        /// Used primarily by mark extensions.
        #[serde(default)]
        keyboard_shortcut: Option<String>,
        /// Optional click behavior for mark extensions.
        /// Defines how clicking on the mark toggles visual state.
        #[serde(default)]
        click_behavior: Option<MarkClickBehavior>,
    },
    /// An item in the block picker menu (the "More" submenu).
    ///
    /// Plugins declare these to add custom block types to the editor's
    /// block picker. The host renders them dynamically and calls the
    /// specified editor command with optional params and user prompt.
    BlockPickerItem {
        /// Unique identifier for this item.
        id: String,
        /// Label displayed in the block picker menu.
        label: String,
        /// Optional Lucide icon name (kebab-case).
        icon: Option<String>,
        /// TipTap editor command to execute (e.g., `"insertConditionalBlock"`).
        editor_command: String,
        /// Static params passed to the editor command.
        #[serde(default)]
        params: Option<serde_json::Value>,
        /// Optional prompt shown to collect user input before executing.
        #[serde(default)]
        prompt: Option<BlockPickerPrompt>,
    },
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
    /// Inline mark that wraps rich text (like bold, spoiler).
    InlineMark,
    /// Host-provided extension too complex for declarative manifest.
    ///
    /// The host looks up a pre-registered TypeScript extension by ID.
    /// For `Builtin` type, the `markdown`, `render_export`, `edit_mode`
    /// fields are ignored — the TypeScript extension handles everything.
    Builtin {
        /// ID of the host-side extension factory.
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
}

/// Click behavior for an inline mark extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub enum MarkClickBehavior {
    /// Toggle between two CSS classes on click (e.g., hidden ↔ revealed).
    ToggleClass {
        /// Class applied when the mark is in its default (hidden) state.
        hidden_class: String,
        /// Class applied when the mark has been clicked (revealed) state.
        revealed_class: String,
    },
}

/// A prompt shown to the user before inserting a block picker item.
///
/// When present on a [`BlockPickerItem`](UiContribution::BlockPickerItem),
/// the host shows a `window.prompt()` dialog and merges the result into
/// the editor command params at `param_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct BlockPickerPrompt {
    /// Message shown in the prompt dialog.
    pub message: String,
    /// Default value pre-filled in the prompt input.
    pub default_value: String,
    /// Key in the params object where the user's input is stored.
    pub param_key: String,
}

/// Metadata for an insert button in the editor menus.
///
/// When present on an [`EditorExtension`](UiContribution::EditorExtension),
/// the host renders a button in the appropriate menu (MoreStylesPicker for
/// inline atoms, BlockPicker/BlockStylePicker for block atoms).
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
// CLI extension types
// ============================================================================

fn default_true() -> bool {
    true
}

/// A CLI subcommand declared by a plugin.
///
/// Plugins include these in their manifest to contribute commands to the
/// `diaryx` CLI. The CLI reads cached manifests at startup and builds
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
    pub url: std::string::String,
    /// SHA-256 hash of the WASM file.
    pub sha256: std::string::String,
    /// File size in bytes.
    pub size: u64,
    /// ISO 8601 timestamp of when the artifact was published.
    pub published_at: std::string::String,
}

/// A single plugin listing in the marketplace registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceEntry {
    /// Canonical plugin ID (e.g., `"diaryx.sync"`).
    pub id: std::string::String,
    /// Human-readable name.
    pub name: std::string::String,
    /// SemVer version string.
    pub version: std::string::String,
    /// One-line summary.
    pub summary: std::string::String,
    /// Full description.
    pub description: std::string::String,
    /// Author or organization.
    pub author: std::string::String,
    /// License identifier.
    pub license: std::string::String,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<std::string::String>,
    /// Category tags for discovery.
    #[serde(default)]
    pub categories: Vec<std::string::String>,
    /// Free-form tags for search.
    #[serde(default)]
    pub tags: Vec<std::string::String>,
    /// WASM artifact reference.
    pub artifact: PluginArtifact,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: Vec<std::string::String>,
    /// Icon URL.
    #[serde(default)]
    pub icon: Option<std::string::String>,
    /// Screenshot URLs.
    #[serde(default)]
    pub screenshots: Vec<std::string::String>,
    /// Requested default permissions (opaque JSON).
    #[serde(default)]
    pub requested_permissions: Option<serde_json::Value>,
}

/// The parsed CDN registry (`registry.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceRegistry {
    /// Schema version (must be `2`).
    pub schema_version: u64,
    /// ISO 8601 timestamp of when the registry was generated.
    pub generated_at: std::string::String,
    /// Plugin listings.
    pub plugins: Vec<MarketplaceEntry>,
    /// Markdown body after the frontmatter.
    #[serde(skip)]
    pub body: std::string::String,
}

/// Metadata parsed from a plugin workspace root `README.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginWorkspaceMetadata {
    /// Canonical plugin ID.
    pub id: std::string::String,
    /// Human-readable name (from `title` frontmatter key).
    pub name: std::string::String,
    /// SemVer version.
    pub version: std::string::String,
    /// Short description (from `description` frontmatter key).
    pub summary: std::string::String,
    /// Author or organization.
    #[serde(default)]
    pub author: Option<std::string::String>,
    /// License identifier.
    #[serde(default)]
    pub license: Option<std::string::String>,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<std::string::String>,
    /// Category tags.
    #[serde(default)]
    pub categories: Vec<std::string::String>,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<std::string::String>,
    /// Declared capabilities.
    #[serde(default)]
    pub capabilities: Vec<std::string::String>,
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
    /// Markdown body after the frontmatter.
    #[serde(skip)]
    pub body: std::string::String,
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

        // Extract and validate schema_version.
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

        // Deserialize plugins array.
        let plugins_yaml = parsed
            .frontmatter
            .get("plugins")
            .ok_or_else(|| DiaryxError::Validation("Registry missing plugins array".to_string()))?;

        let plugins_json = yaml_to_json(plugins_yaml)?;
        let plugins: Vec<MarketplaceEntry> = serde_json::from_value(plugins_json)
            .map_err(|e| DiaryxError::Validation(format!("Failed to parse plugins: {e}")))?;

        // Validate each plugin entry.
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

        // Parse artifact.
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
        }
    }
}

/// Extract a string array from an optional YAML value.
fn yaml_string_array(value: Option<&serde_yaml::Value>) -> Vec<std::string::String> {
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
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MarketplaceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }
}
