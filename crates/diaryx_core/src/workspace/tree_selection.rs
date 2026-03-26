//! Tree selection algorithms for multi-select operations.
//!
//! These functions operate on a `TreeNode` tree to support bulk deletion
//! workflows: pruning nested roots, expanding selections to descendants,
//! ordering paths for safe deletion (children before parents), and checking
//! whether a selection includes descendant entries.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::TreeNode;

/// Compare two paths by depth descending (deepest first).
/// Ties are broken by reverse lexicographic order.
fn compare_path_depth_descending(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a_depth = a.components().count();
    let b_depth = b.components().count();
    b_depth.cmp(&a_depth).then_with(|| b.cmp(a))
}

/// Return the renderable children of a node, skipping placeholder nodes
/// and deduplicating by path.
fn renderable_children(node: &TreeNode) -> Vec<&TreeNode> {
    let mut seen = HashSet::new();
    let mut children = Vec::new();

    for child in &node.children {
        if child.name.starts_with("... (") {
            continue;
        }
        if seen.insert(&child.path) {
            children.push(child);
        }
    }

    children
}

/// Find a node in the tree by path (DFS).
pub fn find_tree_node<'a>(tree: &'a TreeNode, path: &Path) -> Option<&'a TreeNode> {
    if tree.path == path {
        return Some(tree);
    }

    for child in renderable_children(tree) {
        if let Some(found) = find_tree_node(child, path) {
            return Some(found);
        }
    }

    None
}

/// Given selected paths, return only the topmost roots (remove descendants
/// whose ancestor is already selected).
///
/// Paths not found in the tree are appended sorted by depth descending
/// (deepest first), ties broken by reverse lexicographic order.
pub fn prune_nested_roots(tree: &TreeNode, paths: &[PathBuf]) -> Vec<PathBuf> {
    let selected: HashSet<&PathBuf> = paths.iter().collect();
    let mut roots = Vec::new();
    let mut covered = HashSet::new();

    fn visit(
        node: &TreeNode,
        ancestor_selected: bool,
        selected: &HashSet<&PathBuf>,
        roots: &mut Vec<PathBuf>,
        covered: &mut HashSet<PathBuf>,
    ) {
        let is_selected = selected.contains(&node.path);
        if is_selected {
            covered.insert(node.path.clone());
        }
        if is_selected && !ancestor_selected {
            roots.push(node.path.clone());
        }

        let next_ancestor_selected = ancestor_selected || is_selected;
        for child in renderable_children(node) {
            visit(child, next_ancestor_selected, selected, roots, covered);
        }
    }

    visit(tree, false, &selected, &mut roots, &mut covered);

    // Append paths not found in the tree, sorted by depth descending
    let mut remaining: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !covered.contains(*p))
        .cloned()
        .collect();
    remaining.sort_by(|a, b| compare_path_depth_descending(a, b));
    roots.extend(remaining);

    roots
}

/// Given root paths, return all descendants (including the roots themselves).
///
/// DFS: if a node is selected or an ancestor is selected, include it.
/// Root paths not found in the tree are also included.
pub fn expand_selection(tree: &TreeNode, root_paths: &[PathBuf]) -> Vec<PathBuf> {
    let roots: HashSet<&PathBuf> = root_paths.iter().collect();
    let mut expanded = HashSet::new();

    fn visit(
        node: &TreeNode,
        ancestor_selected: bool,
        roots: &HashSet<&PathBuf>,
        expanded: &mut HashSet<PathBuf>,
    ) {
        let is_selected = ancestor_selected || roots.contains(&node.path);
        if is_selected {
            expanded.insert(node.path.clone());
        }

        for child in renderable_children(node) {
            visit(child, is_selected, roots, expanded);
        }
    }

    visit(tree, false, &roots, &mut expanded);

    // Also include any root_paths not found in tree
    for path in root_paths {
        expanded.insert(path.clone());
    }

    expanded.into_iter().collect()
}

/// Post-order traversal: children before parents.
/// Only include paths that are in the `paths` set.
/// Paths not found in the tree are appended sorted by depth descending.
pub fn order_delete_paths(tree: &TreeNode, paths: &[PathBuf]) -> Vec<PathBuf> {
    let selected: HashSet<&PathBuf> = paths.iter().collect();
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();

    fn visit(
        node: &TreeNode,
        selected: &HashSet<&PathBuf>,
        ordered: &mut Vec<PathBuf>,
        seen: &mut HashSet<PathBuf>,
    ) {
        for child in renderable_children(node) {
            visit(child, selected, ordered, seen);
        }

        if selected.contains(&node.path) && seen.insert(node.path.clone()) {
            ordered.push(node.path.clone());
        }
    }

    visit(tree, &selected, &mut ordered, &mut seen);

    // Append paths not found in tree
    let mut remaining: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !seen.contains(*p))
        .cloned()
        .collect();
    remaining.sort_by(|a, b| compare_path_depth_descending(a, b));
    ordered.extend(remaining);

    ordered
}

/// Check whether deleting the given paths will also remove descendant entries.
///
/// Returns true if any root path has children in the tree, or if expanding
/// the selection yields more paths than pruning nested roots.
pub fn selection_includes_descendants(tree: &TreeNode, root_paths: &[PathBuf]) -> bool {
    let roots = prune_nested_roots(tree, root_paths);

    for root_path in &roots {
        if let Some(node) = find_tree_node(tree, root_path)
            && !node.children.is_empty()
        {
            return true;
        }
    }

    expand_selection(tree, &roots).len() > roots.len()
}

/// Full deletion pipeline: prune nested roots, expand to descendants,
/// then order children-first for safe deletion.
pub fn prepare_delete_plan(tree: &TreeNode, paths: &[PathBuf]) -> Vec<PathBuf> {
    let roots = prune_nested_roots(tree, paths);
    let expanded = expand_selection(tree, &roots);
    order_delete_paths(tree, &expanded)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn leaf(name: &str, path: &str) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            description: None,
            path: PathBuf::from(path),
            is_index: false,
            children: vec![],
            properties: HashMap::new(),
        }
    }

    fn node(name: &str, path: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            description: None,
            path: PathBuf::from(path),
            is_index: false,
            children,
            properties: HashMap::new(),
        }
    }

    fn test_tree() -> TreeNode {
        node(
            "README.md",
            "README.md",
            vec![
                leaf("alpha.md", "alpha.md"),
                node(
                    "index.md",
                    "section/index.md",
                    vec![
                        leaf("child.md", "section/child.md"),
                        leaf("nested.md", "shared/nested.md"),
                    ],
                ),
                leaf("omega.md", "omega.md"),
            ],
        )
    }

    #[test]
    fn test_find_tree_node_existing() {
        let tree = test_tree();
        let found = find_tree_node(&tree, Path::new("section/child.md"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "child.md");
    }

    #[test]
    fn test_find_tree_node_root() {
        let tree = test_tree();
        let found = find_tree_node(&tree, Path::new("README.md"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "README.md");
    }

    #[test]
    fn test_find_tree_node_missing() {
        let tree = test_tree();
        assert!(find_tree_node(&tree, Path::new("nonexistent.md")).is_none());
    }

    #[test]
    fn test_prune_nested_roots_parent_and_child() {
        let tree = test_tree();
        let result = prune_nested_roots(
            &tree,
            &[
                PathBuf::from("section/index.md"),
                PathBuf::from("section/child.md"),
                PathBuf::from("omega.md"),
            ],
        );
        assert_eq!(
            result,
            vec![PathBuf::from("section/index.md"), PathBuf::from("omega.md"),]
        );
    }

    #[test]
    fn test_prune_nested_roots_only_leaves() {
        let tree = test_tree();
        let result = prune_nested_roots(
            &tree,
            &[PathBuf::from("alpha.md"), PathBuf::from("omega.md")],
        );
        assert_eq!(
            result,
            vec![PathBuf::from("alpha.md"), PathBuf::from("omega.md")]
        );
    }

    #[test]
    fn test_prune_nested_roots_missing_paths_appended() {
        let tree = test_tree();
        let result = prune_nested_roots(
            &tree,
            &[
                PathBuf::from("alpha.md"),
                PathBuf::from("deep/a/b/missing.md"),
                PathBuf::from("missing.md"),
            ],
        );
        // alpha.md is found in tree; then missing paths sorted by depth descending
        assert_eq!(result[0], PathBuf::from("alpha.md"));
        // deep/a/b/missing.md is deeper than missing.md
        assert_eq!(result[1], PathBuf::from("deep/a/b/missing.md"));
        assert_eq!(result[2], PathBuf::from("missing.md"));
    }

    #[test]
    fn test_expand_selection_parent() {
        let tree = test_tree();
        let mut result = expand_selection(&tree, &[PathBuf::from("section/index.md")]);
        result.sort();
        let mut expected = vec![
            PathBuf::from("section/index.md"),
            PathBuf::from("section/child.md"),
            PathBuf::from("shared/nested.md"),
        ];
        expected.sort();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_expand_selection_leaf() {
        let tree = test_tree();
        let mut result = expand_selection(&tree, &[PathBuf::from("omega.md")]);
        result.sort();
        assert_eq!(result, vec![PathBuf::from("omega.md")]);
    }

    #[test]
    fn test_expand_selection_missing_path_included() {
        let tree = test_tree();
        let mut result = expand_selection(
            &tree,
            &[PathBuf::from("omega.md"), PathBuf::from("missing.md")],
        );
        result.sort();
        let mut expected = vec![PathBuf::from("missing.md"), PathBuf::from("omega.md")];
        expected.sort();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_order_delete_paths_post_order() {
        let tree = test_tree();
        let result = order_delete_paths(
            &tree,
            &[
                PathBuf::from("README.md"),
                PathBuf::from("section/index.md"),
                PathBuf::from("section/child.md"),
                PathBuf::from("shared/nested.md"),
            ],
        );
        assert_eq!(
            result,
            vec![
                PathBuf::from("section/child.md"),
                PathBuf::from("shared/nested.md"),
                PathBuf::from("section/index.md"),
                PathBuf::from("README.md"),
            ]
        );
    }

    #[test]
    fn test_order_delete_paths_missing_appended() {
        let tree = test_tree();
        let result = order_delete_paths(
            &tree,
            &[PathBuf::from("alpha.md"), PathBuf::from("deep/missing.md")],
        );
        assert_eq!(result[0], PathBuf::from("alpha.md"));
        assert_eq!(result[1], PathBuf::from("deep/missing.md"));
    }

    #[test]
    fn test_selection_includes_descendants_with_children() {
        let tree = test_tree();
        assert!(selection_includes_descendants(
            &tree,
            &[PathBuf::from("section/index.md")]
        ));
    }

    #[test]
    fn test_selection_includes_descendants_leaf_only() {
        let tree = test_tree();
        assert!(!selection_includes_descendants(
            &tree,
            &[PathBuf::from("omega.md")]
        ));
    }

    #[test]
    fn test_prepare_delete_plan_full_pipeline() {
        let tree = test_tree();
        let result = prepare_delete_plan(
            &tree,
            &[
                PathBuf::from("section/index.md"),
                PathBuf::from("section/child.md"),
            ],
        );
        // Prune: section/index.md (child.md is nested under it)
        // Expand: section/index.md, section/child.md, shared/nested.md
        // Order (post-order): child.md, nested.md, index.md
        assert_eq!(
            result,
            vec![
                PathBuf::from("section/child.md"),
                PathBuf::from("shared/nested.md"),
                PathBuf::from("section/index.md"),
            ]
        );
    }

    #[test]
    fn test_placeholder_nodes_are_skipped() {
        let tree = node(
            "root",
            "root.md",
            vec![
                leaf("alpha.md", "alpha.md"),
                leaf("... (3 more)", "placeholder"),
                leaf("beta.md", "beta.md"),
            ],
        );

        let children = renderable_children(&tree);
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "alpha.md");
        assert_eq!(children[1].name, "beta.md");
    }

    #[test]
    fn test_duplicate_children_are_deduplicated() {
        let tree = node(
            "root",
            "root.md",
            vec![
                leaf("alpha.md", "alpha.md"),
                leaf("alpha.md", "alpha.md"),
                leaf("beta.md", "beta.md"),
            ],
        );

        let children = renderable_children(&tree);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_compare_path_depth_descending() {
        let mut paths = vec![
            PathBuf::from("a.md"),
            PathBuf::from("x/y/z.md"),
            PathBuf::from("b/c.md"),
            PathBuf::from("a/c.md"),
        ];
        paths.sort_by(|a, b| compare_path_depth_descending(a, b));
        // Deepest first (x/y/z.md = 3 components), then b/c.md and a/c.md (2 components, reverse lex)
        assert_eq!(paths[0], PathBuf::from("x/y/z.md"));
        assert_eq!(paths[1], PathBuf::from("b/c.md"));
        assert_eq!(paths[2], PathBuf::from("a/c.md"));
        assert_eq!(paths[3], PathBuf::from("a.md"));
    }
}
