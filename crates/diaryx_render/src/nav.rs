//! Site navigation tree construction.
//!
//! [`build_site_nav_tree`] builds the whole-site tree from every page's
//! `contents_links`/`parent_link`; [`nav_for_page`] specializes that tree for a
//! single page (marking current/ancestor nodes and computing breadcrumbs).
//! Both are pure functions over [`PublishedPage`].

use std::collections::{HashMap, HashSet};

use crate::types::{NavLink, PublishedPage, SiteNavNode, SiteNavigation};

/// Build a site navigation tree from all published pages.
///
/// Uses each page's `contents_links` and `parent_link` to build a tree rooted
/// at the page with `is_root == true`. Filters out `hide_from_nav` pages and
/// sorts children by `nav_order` (if present), then by their position in the
/// parent's `contents_links`.
pub fn build_site_nav_tree(pages: &[PublishedPage]) -> Vec<SiteNavNode> {
    // Map dest_filename → page for quick lookup
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    // Find root page
    let root = match pages.iter().find(|p| p.is_root) {
        Some(r) => r,
        None => return vec![],
    };

    // Build children for a page recursively
    fn build_children(
        page: &PublishedPage,
        page_map: &HashMap<&str, &PublishedPage>,
        depth: usize,
    ) -> Vec<SiteNavNode> {
        if depth >= 3 || page.contents_links.is_empty() {
            return vec![];
        }

        let mut children: Vec<(usize, SiteNavNode)> = Vec::new();

        for (idx, link) in page.contents_links.iter().enumerate() {
            let child_page = page_map.get(link.href.as_str());

            // Skip hidden pages
            if let Some(cp) = child_page {
                if cp.hide_from_nav {
                    continue;
                }
            }

            let title = child_page
                .and_then(|cp| cp.nav_title.as_deref())
                .unwrap_or(&link.title)
                .to_string();

            let sub_children = child_page
                .map(|cp| build_children(cp, page_map, depth + 1))
                .unwrap_or_default();

            let nav_order = child_page.and_then(|cp| cp.nav_order);
            let sort_key = nav_order.unwrap_or(idx as i32);

            children.push((
                sort_key as usize,
                SiteNavNode {
                    title,
                    href: link.href.clone(),
                    is_current: false,
                    is_ancestor_of_current: false,
                    children: sub_children,
                },
            ));
        }

        // Sort by nav_order (encoded in sort_key), stable for equal keys
        children.sort_by_key(|(key, _)| *key);
        children.into_iter().map(|(_, node)| node).collect()
    }

    let root_children = build_children(root, &page_map, 0);

    // Build root node
    let root_title = root.nav_title.as_deref().unwrap_or(&root.title).to_string();

    vec![SiteNavNode {
        title: root_title,
        href: root.dest_filename.clone(),
        is_current: false,
        is_ancestor_of_current: false,
        children: root_children,
    }]
}

/// Build navigation context (tree with current-page marking + breadcrumbs) for a specific page.
pub fn nav_for_page(
    tree: &[SiteNavNode],
    current_dest: &str,
    pages: &[PublishedPage],
) -> SiteNavigation {
    // Deep-clone and mark current + ancestors
    fn mark_current(nodes: &[SiteNavNode], target: &str) -> (Vec<SiteNavNode>, bool) {
        let mut result = Vec::with_capacity(nodes.len());
        let mut found = false;

        for node in nodes {
            let (children, child_found) = mark_current(&node.children, target);
            let is_current = node.href == target;
            let is_ancestor = child_found;

            if is_current || is_ancestor {
                found = true;
            }

            result.push(SiteNavNode {
                title: node.title.clone(),
                href: node.href.clone(),
                is_current,
                is_ancestor_of_current: is_ancestor,
                children,
            });
        }

        (result, found)
    }

    let (marked_tree, _) = mark_current(tree, current_dest);

    // Build breadcrumbs by walking parent_link chain
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    let mut breadcrumbs = Vec::new();
    if let Some(current_page) = page_map.get(current_dest) {
        // Walk up parent chain
        let mut chain = vec![NavLink {
            href: current_page.dest_filename.clone(),
            title: current_page
                .nav_title
                .clone()
                .unwrap_or_else(|| current_page.title.clone()),
        }];

        let mut visited = HashSet::new();
        visited.insert(current_dest.to_string());

        let mut cursor = current_page.parent_link.as_ref();
        while let Some(parent) = cursor {
            if !visited.insert(parent.href.clone()) {
                break; // cycle guard
            }
            chain.push(NavLink {
                href: parent.href.clone(),
                title: page_map
                    .get(parent.href.as_str())
                    .and_then(|p| p.nav_title.clone())
                    .unwrap_or_else(|| parent.title.clone()),
            });
            cursor = page_map
                .get(parent.href.as_str())
                .and_then(|p| p.parent_link.as_ref());
        }

        chain.reverse();
        breadcrumbs = chain;
    }

    SiteNavigation {
        tree: marked_tree,
        breadcrumbs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_page(
        dest: &str,
        title: &str,
        is_root: bool,
        contents: Vec<NavLink>,
        parent: Option<NavLink>,
    ) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}", dest.replace(".html", ".md"))),
            dest_filename: dest.to_string(),
            title: title.to_string(),
            rendered_body: String::new(),
            markdown_body: String::new(),
            contents_links: contents,
            parent_link: parent,
            is_root,
            description: None,
            author: None,
            created: None,
            updated: None,
            attachments: vec![],
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
            file_ark: None,
            source_markdown: String::new(),
        }
    }

    #[test]
    fn test_nav_tree_flat_workspace() {
        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            make_page(
                "a.html",
                "A",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            make_page(
                "b.html",
                "B",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].title, "Home");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].title, "A");
        assert_eq!(tree[0].children[1].title, "B");
    }

    #[test]
    fn test_nav_tree_deep_hierarchy() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![NavLink {
                    href: "grandchild.html".into(),
                    title: "Grandchild".into(),
                }],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
            make_page(
                "grandchild.html",
                "Grandchild",
                false,
                vec![],
                Some(NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children.len(), 1); // Parent
        assert_eq!(tree[0].children[0].children.len(), 1); // Child
        assert_eq!(tree[0].children[0].children[0].children.len(), 1); // Grandchild
        // Depth 3: grandchild's children are empty (max depth reached)
        assert_eq!(
            tree[0].children[0].children[0].children[0].children.len(),
            0
        );
    }

    #[test]
    fn test_nav_tree_hide_from_nav() {
        let mut hidden_page = make_page(
            "hidden.html",
            "Hidden",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        hidden_page.hide_from_nav = true;

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "visible.html".into(),
                        title: "Visible".into(),
                    },
                    NavLink {
                        href: "hidden.html".into(),
                        title: "Hidden".into(),
                    },
                ],
                None,
            ),
            make_page(
                "visible.html",
                "Visible",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            hidden_page,
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].title, "Visible");
    }

    #[test]
    fn test_nav_tree_nav_order() {
        let mut page_b = make_page(
            "b.html",
            "B",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_b.nav_order = Some(1);

        let mut page_a = make_page(
            "a.html",
            "A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_order = Some(2);

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            page_a,
            page_b,
        ];

        let tree = build_site_nav_tree(&pages);
        // B has nav_order 1, A has 2 — B should come first
        assert_eq!(tree[0].children[0].title, "B");
        assert_eq!(tree[0].children[1].title, "A");
    }

    #[test]
    fn test_nav_tree_nav_title() {
        let mut page_a = make_page(
            "a.html",
            "Full Title of A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_title = Some("Short A".to_string());

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![NavLink {
                    href: "a.html".into(),
                    title: "Full Title of A".into(),
                }],
                None,
            ),
            page_a,
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children[0].title, "Short A");
    }

    #[test]
    fn test_nav_for_page_marks_current_and_ancestors() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        let nav = nav_for_page(&tree, "child.html", &pages);

        // Root should be ancestor
        assert!(nav.tree[0].is_ancestor_of_current);
        assert!(!nav.tree[0].is_current);

        // Parent should be ancestor
        assert!(nav.tree[0].children[0].is_ancestor_of_current);
        assert!(!nav.tree[0].children[0].is_current);

        // Child should be current
        assert!(nav.tree[0].children[0].children[0].is_current);
        assert!(!nav.tree[0].children[0].children[0].is_ancestor_of_current);

        // Breadcrumbs: Root → Parent → Child
        assert_eq!(nav.breadcrumbs.len(), 3);
        assert_eq!(nav.breadcrumbs[0].title, "Root");
        assert_eq!(nav.breadcrumbs[1].title, "Parent");
        assert_eq!(nav.breadcrumbs[2].title, "Child");
    }
}
