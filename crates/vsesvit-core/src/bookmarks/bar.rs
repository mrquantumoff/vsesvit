//! The bookmarks bar's snapshot and what fits on it (docs/design/linux-cef/design.md section
//! 4.3, row 10).
//!
//! A shell copies the bar out of core as [`BarItem`]s, so it builds its widgets without holding
//! a profile borrow, and compares two copies to learn whether only favicons changed
//! ([`same_but_icons`]): rebuilding every button for that would close open menus and drop hover.
//! [`fitting`] decides how many of the items show whole before the rest move behind the chevron.

use super::{BookmarkId, BookmarkNode, MAX_DEPTH, NodeKind};
use crate::{Profile, Url};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarItem {
    Link {
        id: BookmarkId,
        title: String,
        url: String,
        /// The saved favicon (PNG).
        icon: Option<Vec<u8>>,
    },
    Folder {
        id: BookmarkId,
        title: String,
        children: Vec<BarItem>,
        /// Every item directly in the folder, separators too: what deleting it deletes.
        contents: usize,
    },
}

impl BarItem {
    pub fn id(&self) -> BookmarkId {
        match self {
            BarItem::Link { id, .. } | BarItem::Folder { id, .. } => *id,
        }
    }
}

/// The bar's items: the bookmarks bar folder with each link's saved favicon, in display order.
pub fn snapshot(p: &mut Profile) -> Vec<BarItem> {
    let mut items = {
        let bookmarks = p.bookmarks();
        items_from(BookmarkId::TOOLBAR, &|folder| bookmarks.children(folder))
    };
    fill_icons(&mut items, &mut |url| Url::parse(url).ok().and_then(|url| p.favicons().get(&url).ok().flatten()));
    items
}

/// The bar's items for the bookmark folder `folder`, in display order. Separators are dropped,
/// and so is a link without a URL. `children` lists a folder's nodes.
pub fn items_from(folder: BookmarkId, children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>) -> Vec<BarItem> {
    bar_items(children(folder), 0, children)
}

/// The bar's items for the bookmark nodes `nodes`, `depth` folders below the bar.
fn bar_items(nodes: Vec<BookmarkNode>, depth: usize, children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>) -> Vec<BarItem> {
    nodes
        .into_iter()
        .filter_map(|node| match node.kind {
            NodeKind::Url => node.url.map(|url| BarItem::Link {
                id: node.id,
                title: node.title,
                url: url.to_string(),
                icon: None,
            }),
            NodeKind::Folder => {
                let inner = children(node.id);
                Some(BarItem::Folder {
                    id: node.id,
                    contents: inner.len(),
                    children: if depth + 1 < MAX_DEPTH { bar_items(inner, depth + 1, children) } else { Vec::new() },
                    title: node.title,
                })
            }
            NodeKind::Separator => None,
        })
        .collect()
}

/// Gives every link, in folders too, the favicon `favicon` has for its URL.
pub fn fill_icons(items: &mut [BarItem], favicon: &mut dyn FnMut(&str) -> Option<Vec<u8>>) {
    for item in items {
        match item {
            BarItem::Link { url, icon, .. } => *icon = favicon(url),
            BarItem::Folder { children, .. } => fill_icons(children, favicon),
        }
    }
}

/// How many of the items, `widths` wide, fit whole in `available`. When not all of them do,
/// `chevron` is kept free for the button that lists the rest.
pub fn fitting(widths: &[f64], available: f64, chevron: f64) -> usize {
    if widths.iter().sum::<f64>() <= available {
        return widths.len();
    }
    let room = available - chevron;
    widths
        .iter()
        .scan(0.0, |used, width| {
            *used += width;
            Some(*used)
        })
        .take_while(|used| *used <= room)
        .count()
}

/// Whether `new` is `old` with favicons gained or changed and nothing else: no icon lost.
pub fn same_but_icons(old: &BarItem, new: &BarItem) -> bool {
    match (old, new) {
        (
            BarItem::Link { id, title, url, icon },
            BarItem::Link { id: new_id, title: new_title, url: new_url, icon: new_icon },
        ) => id == new_id && title == new_title && url == new_url && (new_icon.is_some() || icon.is_none()),
        (
            BarItem::Folder { id, title, children, contents },
            BarItem::Folder { id: new_id, title: new_title, children: new_children, contents: new_contents },
        ) => {
            id == new_id
                && title == new_title
                && contents == new_contents
                && children.len() == new_children.len()
                && children.iter().zip(new_children).all(|(old, new)| same_but_icons(old, new))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmarks::InsertAt;
    use crate::{OpenOptions, Url};

    fn node(id: BookmarkId, kind: NodeKind, title: &str, url: Option<&str>) -> BookmarkNode {
        BookmarkNode {
            id,
            kind,
            parent: BookmarkId::TOOLBAR,
            index: 0,
            title: title.into(),
            url: url.map(|u| Url::parse(u).unwrap()),
            added_ms: 0,
        }
    }

    #[test]
    fn folders_nest_and_separators_drop() {
        // Root ids stand in for arbitrary node ids here.
        let children = |folder: BookmarkId| match folder {
            f if f == BookmarkId::TOOLBAR => vec![
                node(BookmarkId::MOBILE, NodeKind::Url, "", Some("https://a.test/")),
                node(BookmarkId::ROOT, NodeKind::Separator, "", None),
                node(BookmarkId::OTHER, NodeKind::Folder, "F", None),
            ],
            f if f == BookmarkId::OTHER => {
                vec![node(BookmarkId::MOBILE, NodeKind::Url, "B", Some("https://b.test/"))]
            }
            _ => vec![],
        };
        assert_eq!(
            items_from(BookmarkId::TOOLBAR, &children),
            [
                BarItem::Link { id: BookmarkId::MOBILE, title: "".into(), url: "https://a.test/".into(), icon: None },
                BarItem::Folder {
                    id: BookmarkId::OTHER,
                    title: "F".into(),
                    children: vec![BarItem::Link {
                        id: BookmarkId::MOBILE,
                        title: "B".into(),
                        url: "https://b.test/".into(),
                        icon: None,
                    }],
                    contents: 1,
                },
            ]
        );
    }

    #[test]
    fn folders_nested_without_end_stop_at_the_maximum_depth() {
        // Every folder holds another, as a synced chain thousands deep would.
        let children = |_| vec![node(BookmarkId::OTHER, NodeKind::Folder, "F", None)];
        let items = items_from(BookmarkId::TOOLBAR, &children);
        let mut level = items.as_slice();
        let mut depth = 0;
        while let [BarItem::Folder { children, .. }] = level {
            depth += 1;
            level = children;
        }
        assert!(level.is_empty());
        assert_eq!(depth, MAX_DEPTH);
    }

    #[test]
    fn a_folder_of_only_a_separator_shows_empty_but_counts_it() {
        let children = |folder: BookmarkId| match folder {
            f if f == BookmarkId::TOOLBAR => vec![node(BookmarkId::OTHER, NodeKind::Folder, "F", None)],
            f if f == BookmarkId::OTHER => vec![node(BookmarkId::MOBILE, NodeKind::Separator, "", None)],
            _ => vec![],
        };
        let items = items_from(BookmarkId::TOOLBAR, &children);
        let [BarItem::Folder { children, contents, .. }] = items.as_slice() else { panic!("a folder") };
        assert!(children.is_empty());
        assert_eq!(*contents, 1);
    }

    #[test]
    fn only_whole_items_fit_and_the_chevron_keeps_its_room() {
        assert_eq!(fitting(&[100.0, 100.0, 100.0], 300.0, 30.0), 3, "all fit, no chevron");
        assert_eq!(fitting(&[100.0, 100.0, 100.0], 299.0, 30.0), 2);
        assert_eq!(fitting(&[100.0, 100.0, 100.0], 229.0, 30.0), 1, "room for the chevron");
        assert_eq!(fitting(&[100.0, 100.0], 120.0, 30.0), 0);
        assert_eq!(fitting(&[], 10.0, 30.0), 0);
    }

    #[test]
    fn icons_reach_links_inside_folders() {
        let link = |url: &str| BarItem::Link { id: BookmarkId::MOBILE, title: url.into(), url: url.into(), icon: None };
        let mut items = vec![
            link("https://a.test/"),
            BarItem::Folder {
                id: BookmarkId::OTHER,
                title: "F".into(),
                children: vec![link("https://b.test/")],
                contents: 1,
            },
        ];
        fill_icons(&mut items, &mut |url| url.contains("b.test").then(|| vec![1]));
        assert!(matches!(&items[0], BarItem::Link { icon: None, .. }));
        let BarItem::Folder { children, .. } = &items[1] else { panic!("a folder") };
        assert!(matches!(&children[0], BarItem::Link { icon: Some(_), .. }));
    }

    #[test]
    fn only_new_icons_keep_the_entries() {
        let link = |title: &str, icon: Option<u8>| BarItem::Link {
            id: BookmarkId::MOBILE,
            title: title.into(),
            url: "https://a.test/".into(),
            icon: icon.map(|b| vec![b]),
        };
        let folder = |child| BarItem::Folder {
            id: BookmarkId::OTHER,
            title: "F".into(),
            children: vec![child],
            contents: 1,
        };
        assert!(same_but_icons(&link("A", None), &link("A", Some(1))));
        assert!(same_but_icons(&link("A", Some(1)), &link("A", Some(2))));
        assert!(same_but_icons(&folder(link("A", None)), &folder(link("A", Some(1)))));
        assert!(!same_but_icons(&link("A", Some(1)), &link("A", None)));
        assert!(!same_but_icons(&link("A", None), &link("B", None)));
        assert!(!same_but_icons(&folder(link("A", None)), &folder(link("B", Some(1)))));
        assert!(!same_but_icons(&link("A", None), &folder(link("A", None))));
    }

    #[test]
    fn the_snapshot_holds_the_bar_with_each_links_saved_icon() {
        let dir = std::env::temp_dir().join(format!("vsesvit-bar-snapshot-{}", uuid::Uuid::new_v4().simple()));
        let mut p = Profile::open(&dir, OpenOptions::default()).unwrap();
        let (a, b) = (Url::parse("https://a.test/").unwrap(), Url::parse("https://b.test/").unwrap());
        let folder = p.bookmarks().add_folder(BookmarkId::TOOLBAR, InsertAt::End, "F").unwrap();
        p.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "A", &a).unwrap();
        let inner = p.bookmarks().add_url(folder, InsertAt::End, "B", &b).unwrap();
        let elsewhere = Url::parse("https://c.test/").unwrap();
        p.bookmarks().add_url(BookmarkId::OTHER, InsertAt::End, "Elsewhere", &elsewhere).unwrap();
        assert!(p.favicons().record(&b, &[1, 2, 3]).unwrap());
        let items = snapshot(&mut p);
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
        let [BarItem::Folder { children, contents: 1, .. }, BarItem::Link { url, icon: None, .. }] = items.as_slice()
        else {
            panic!("the folder and the link: {items:?}")
        };
        assert_eq!(url, "https://a.test/");
        assert!(matches!(children.as_slice(), [BarItem::Link { id, icon: Some(png), .. }] if *id == inner && png == &[1, 2, 3]));
    }
}
