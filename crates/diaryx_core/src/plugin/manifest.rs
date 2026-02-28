//! Plugin manifest types for declarative plugin metadata and UI contributions.
//!
//! Each plugin declares a [`PluginManifest`] that describes its identity,
//! capabilities, and UI contributions. The frontend reads these manifests
//! to dynamically render settings tabs, sidebar panels, command palette items, etc.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::PluginId;

/// Declarative metadata for a plugin.
///
/// Returned by [`Plugin::manifest()`](super::Plugin::manifest) and consumed
/// by the frontend to build extension-point UI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
}

/// A capability that a plugin can declare.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
        fields: Vec<SettingsField>,
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
    /// An editor extension (TipTap node/mark) contributed by a plugin.
    ///
    /// The host generates a TipTap extension from this declaration and calls
    /// the plugin's `render_export` function to render content.
    EditorExtension {
        /// Unique extension ID (becomes the TipTap node name).
        extension_id: String,
        /// What kind of editor node this creates.
        node_type: EditorNodeType,
        /// Markdown syntax delimiters for parsing and serialization.
        markdown: MarkdownSyntax,
        /// Name of the plugin's WASM export to call for rendering.
        render_export: String,
        /// How the user edits the source content.
        edit_mode: EditMode,
        /// Optional CSS to inject for rendered output.
        css: Option<String>,
        /// Optional insert command for editor menu integration.
        ///
        /// When present, the host adds a button in the appropriate editor menu
        /// (MoreStylesPicker for inline atoms, BlockPicker for block atoms)
        /// to insert an empty node of this type.
        #[serde(default)]
        insert_command: Option<InsertCommand>,
    },
}

/// Which sidebar a tab appears in.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum SidebarSide {
    /// Left sidebar.
    Left,
    /// Right sidebar.
    Right,
}

/// Where a status bar item appears.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum StatusBarPosition {
    /// Left-aligned.
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
}

/// Which host context menu surface a plugin contribution targets.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum ContextMenuTarget {
    /// Context menu for entry nodes in the left sidebar file tree.
    LeftSidebarTree,
}

/// The kind of TipTap node an [`EditorExtension`](UiContribution::EditorExtension) creates.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum EditorNodeType {
    /// Inline atom node (like a footnote reference).
    InlineAtom,
    /// Block atom node (like an HTML block).
    BlockAtom,
}

/// Markdown syntax delimiters for an editor extension.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct MarkdownSyntax {
    /// Whether this is an inline or block-level syntax.
    pub level: MarkdownLevel,
    /// Opening delimiter (e.g., `"$"` or `"$$"`).
    pub open: String,
    /// Closing delimiter (e.g., `"$"` or `"$$"`).
    pub close: String,
}

/// Whether a markdown syntax is inline or block-level.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum MarkdownLevel {
    /// Inline-level (within a paragraph).
    Inline,
    /// Block-level (standalone paragraph).
    Block,
}

/// How the user edits the source content of an editor extension node.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum EditMode {
    /// Click opens a popover with a source text input (for inline nodes).
    Popover,
    /// Click toggles between source textarea and rendered preview (for block nodes).
    SourceToggle,
}

/// Metadata for an insert button in the editor menus.
///
/// When present on an [`EditorExtension`](UiContribution::EditorExtension),
/// the host renders a button in the appropriate menu (MoreStylesPicker for
/// inline atoms, BlockPicker/BlockStylePicker for block atoms).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
}

/// A select dropdown option.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct SelectOption {
    /// The value stored when selected.
    pub value: String,
    /// The label displayed to the user.
    pub label: String,
}
