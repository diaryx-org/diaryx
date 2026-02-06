//! Convert diaryx TreeNode to tui-tree-widget TreeItem

use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::text::Text;
use tui_tree_widget::TreeItem;

use diaryx_core::workspace::TreeNode;

/// Convert a diaryx TreeNode to a tui-tree-widget TreeItem.
///
/// The display format is "title - description" or just "title" if no description.
/// Deduplicates children by path since tui-tree-widget requires unique identifiers.
pub fn tree_node_to_item(node: &TreeNode) -> TreeItem<'static, PathBuf> {
    // Format display text: "title - description" or just "title"
    let display = match &node.description {
        Some(desc) if !desc.is_empty() => format!("{} - {}", node.name, desc),
        _ => node.name.clone(),
    };

    // Recursively convert children, deduplicating by path
    let mut seen = HashSet::new();
    let children: Vec<TreeItem<'static, PathBuf>> = node
        .children
        .iter()
        .filter(|child| seen.insert(child.path.clone()))
        .map(tree_node_to_item)
        .collect();

    // TreeItem::new takes (identifier, text, children)
    TreeItem::new(node.path.clone(), Text::raw(display), children)
        .expect("TreeItem creation should not fail")
}
