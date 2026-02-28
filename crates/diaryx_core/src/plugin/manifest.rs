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
