//! The bookmark editor's model: what it works on, what its URL field accepts, the folders it
//! offers, what Done saves, and the star that opens it (docs/design/linux-cef/design.md section
//! 4.3, row 10). A shell draws the form and hands the fields back as an [`Edit`].

use super::{BookmarkId, BookmarkNode, Bookmarks, InsertAt, MAX_DEPTH, NodeKind};
use crate::search::classify_url;
use crate::{Error, Url};

/// What the editor works on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A bookmark the star has just added.
    Added(BookmarkId),
    /// A bookmark or folder that already exists.
    Existing(BookmarkId),
    /// A bookmark to add, filled in with the current page.
    NewPage { title: String, url: String },
    NewFolder,
}

/// How the editor looks for a target: its title and which parts it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub title: &'static str,
    pub url: bool,
    pub remove: bool,
}

impl Target {
    /// `kind` is the kind of the bookmark an `Added` or `Existing` target names.
    pub fn look(&self, kind: NodeKind) -> Look {
        match (self, kind) {
            (Target::Added(_), _) => Look { title: "Bookmark added", url: true, remove: true },
            (Target::Existing(_), NodeKind::Folder) => Look { title: "Rename folder", url: false, remove: false },
            (Target::Existing(_), _) => Look { title: "Edit bookmark", url: true, remove: true },
            (Target::NewPage { .. }, _) => Look { title: "Add page", url: true, remove: false },
            (Target::NewFolder, _) => Look { title: "Add folder", url: false, remove: false },
        }
    }

    /// The bookmark this target edits, if it exists already.
    pub fn existing(&self) -> Option<BookmarkId> {
        match self {
            Target::Added(id) | Target::Existing(id) => Some(*id),
            Target::NewPage { .. } | Target::NewFolder => None,
        }
    }
}

/// What Done saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub target: Target,
    pub name: String,
    /// For links; `None` for folders.
    pub url: Option<Url>,
    pub folder: BookmarkId,
}

/// The URL field's text as a bookmark URL, if it is one.
pub fn parse_url(text: &str) -> Option<Url> {
    classify_url(text).map(|target| target.url().clone())
}

/// A folder the editor offers: its id and its label, indented by depth.
pub type FolderChoice = (BookmarkId, String);

/// Every folder under `roots`, depth first and at most `MAX_DEPTH` deep, except `exclude` and
/// what is inside it (a folder cannot move into itself).
pub fn folder_choices(
    roots: &[BookmarkNode],
    children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>,
    exclude: Option<BookmarkId>,
) -> Vec<FolderChoice> {
    fn walk(
        node: &BookmarkNode,
        depth: usize,
        children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>,
        exclude: Option<BookmarkId>,
        out: &mut Vec<FolderChoice>,
    ) {
        if node.kind != NodeKind::Folder || Some(node.id) == exclude {
            return;
        }
        out.push((node.id, format!("{}{}", "\u{2003}".repeat(depth), node.title)));
        if depth < MAX_DEPTH {
            for child in children(node.id) {
                walk(&child, depth + 1, children, exclude, out);
            }
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, 0, children, exclude, &mut out);
    }
    out
}

/// The folders a bookmark can go to: the bookmarks bar, Other bookmarks, and Mobile bookmarks
/// when it has any, indented by depth, without `exclude` and its subfolders.
pub fn folders(bookmarks: &Bookmarks<'_>, exclude: Option<BookmarkId>) -> Vec<FolderChoice> {
    let children = |id| bookmarks.children(id);
    let roots: Vec<BookmarkNode> = [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE]
        .into_iter()
        .filter(|&id| id != BookmarkId::MOBILE || !children(id).is_empty())
        .filter_map(|id| bookmarks.get(id))
        .collect();
    folder_choices(&roots, &children, exclude)
}

/// Saves what the editor holds. Returns the bookmark it saved.
pub fn save(bookmarks: &mut Bookmarks<'_>, edit: &Edit) -> Result<BookmarkId, Error> {
    match &edit.target {
        Target::Added(id) | Target::Existing(id) => {
            bookmarks.rename(*id, &edit.name)?;
            if let Some(url) = &edit.url {
                bookmarks.set_url(*id, url)?;
            }
            if bookmarks.get(*id).is_some_and(|n| n.parent != edit.folder) {
                bookmarks.move_to(*id, edit.folder, InsertAt::End)?;
            }
            Ok(*id)
        }
        Target::NewPage { .. } => match &edit.url {
            Some(url) => bookmarks.add_url(edit.folder, InsertAt::End, &edit.name, url),
            None => bookmarks.add_folder(edit.folder, InsertAt::End, &edit.name),
        },
        Target::NewFolder => bookmarks.add_folder(edit.folder, InsertAt::End, &edit.name),
    }
}

/// The star button and its shortcut on the page at `url`, titled `title`: the editor on the
/// page's first bookmark, or, when it has none, on a new one at the end of the bookmarks bar.
/// `None` when `url` is not a URL.
pub fn star(bookmarks: &mut Bookmarks<'_>, url: &str, title: &str) -> Result<Option<Target>, Error> {
    let Ok(url) = Url::parse(url) else {
        return Ok(None);
    };
    if let Some(first) = bookmarks.find_by_url(&url).first() {
        return Ok(Some(Target::Existing(first.id)));
    }
    let id = bookmarks.add_url(BookmarkId::TOOLBAR, InsertAt::End, title, &url)?;
    Ok(Some(Target::Added(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpenOptions, Profile};

    fn folder(id: BookmarkId, title: &str) -> BookmarkNode {
        BookmarkNode {
            id,
            kind: NodeKind::Folder,
            parent: BookmarkId::ROOT,
            index: 0,
            title: title.into(),
            url: None,
            added_ms: 0,
        }
    }

    fn open(name: &str) -> (Profile, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("vsesvit-editor-{name}-{}", uuid::Uuid::new_v4().simple()));
        (Profile::open(&dir, OpenOptions::default()).unwrap(), dir)
    }

    #[test]
    fn folders_are_listed_depth_first_and_a_folder_not_inside_itself() {
        // Root ids stand in for arbitrary folder ids here.
        let (bar, a, a1, other) = (BookmarkId::TOOLBAR, BookmarkId::MOBILE, BookmarkId::ROOT, BookmarkId::OTHER);
        let children = move |id: BookmarkId| {
            if id == bar {
                vec![folder(a, "A")]
            } else if id == a {
                vec![folder(a1, "A1")]
            } else {
                vec![]
            }
        };
        let roots = [folder(bar, "Bookmarks bar"), folder(other, "Other bookmarks")];
        let labels = |exclude| {
            folder_choices(&roots, &children, exclude).into_iter().map(|(_, label)| label).collect::<Vec<_>>()
        };
        assert_eq!(labels(None), ["Bookmarks bar", "\u{2003}A", "\u{2003}\u{2003}A1", "Other bookmarks"]);
        assert_eq!(labels(Some(a)), ["Bookmarks bar", "Other bookmarks"]);
    }

    #[test]
    fn folders_nested_without_end_are_listed_to_the_maximum_depth() {
        // Every folder holds another, as a synced chain thousands deep would.
        let children = |_| vec![folder(BookmarkId::OTHER, "F")];
        let choices = folder_choices(&[folder(BookmarkId::TOOLBAR, "Bar")], &children, None);
        assert_eq!(choices.len(), MAX_DEPTH + 1);
    }

    #[test]
    fn each_target_has_its_look() {
        let id = BookmarkId::TOOLBAR;
        assert_eq!(Target::Added(id).look(NodeKind::Url).title, "Bookmark added");
        let edit = Target::Existing(id).look(NodeKind::Url);
        assert!(edit.url && edit.remove);
        let folder = Target::Existing(id).look(NodeKind::Folder);
        assert!(!folder.url && !folder.remove);
        assert!(!Target::NewFolder.look(NodeKind::Folder).url);
        let page = Target::NewPage { title: String::new(), url: String::new() };
        assert!(page.look(NodeKind::Url).url && !page.look(NodeKind::Url).remove);
    }

    #[test]
    fn only_web_addresses_are_bookmark_urls() {
        assert!(parse_url("https://example.com/a").is_some());
        assert!(parse_url("example.com").is_some());
        assert!(parse_url("not a url").is_none());
        assert!(parse_url("").is_none());
    }

    #[test]
    fn mobile_bookmarks_are_offered_only_once_they_hold_something() {
        let (mut p, dir) = open("folders");
        let offered = |p: &mut Profile| folders(&p.bookmarks(), None).into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        let before = offered(&mut p);
        let url = Url::parse("https://a.test/").unwrap();
        p.bookmarks().add_url(BookmarkId::MOBILE, InsertAt::End, "A", &url).unwrap();
        let after = offered(&mut p);
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(before, [BookmarkId::TOOLBAR, BookmarkId::OTHER]);
        assert_eq!(after, [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE]);
    }

    #[test]
    fn the_form_saves_name_url_and_folder_through_core() {
        let (mut p, dir) = open("save");
        let url = |s: &str| Url::parse(s).unwrap();
        let mut bookmarks = p.bookmarks();
        let id = bookmarks.add_url(BookmarkId::TOOLBAR, InsertAt::End, "Old", &url("https://a.test/")).unwrap();
        let edit = Edit {
            target: Target::Existing(id),
            name: "New".into(),
            url: Some(url("https://b.test/")),
            folder: BookmarkId::OTHER,
        };
        assert_eq!(save(&mut bookmarks, &edit).unwrap(), id);
        let saved = bookmarks.get(id).unwrap();
        let page = Edit {
            target: Target::NewPage { title: String::new(), url: String::new() },
            name: "Page".into(),
            url: Some(url("https://c.test/")),
            folder: BookmarkId::TOOLBAR,
        };
        let page_id = save(&mut bookmarks, &page).unwrap();
        let made = Edit { target: Target::NewFolder, name: "Dir".into(), url: None, folder: BookmarkId::OTHER };
        let folder_id = save(&mut bookmarks, &made).unwrap();
        let (page, dir_node) = (bookmarks.get(page_id).unwrap(), bookmarks.get(folder_id).unwrap());
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((saved.title.as_str(), saved.url, saved.parent), ("New", Some(url("https://b.test/")), BookmarkId::OTHER));
        assert_eq!((page.title.as_str(), page.parent), ("Page", BookmarkId::TOOLBAR));
        assert_eq!((dir_node.kind, dir_node.title.as_str(), dir_node.parent), (NodeKind::Folder, "Dir", BookmarkId::OTHER));
    }

    #[test]
    fn the_star_bookmarks_a_page_once_then_edits_its_bookmark() {
        let (mut p, dir) = open("star");
        let mut bookmarks = p.bookmarks();
        let first = star(&mut bookmarks, "https://a.test/", "A").unwrap();
        let again = star(&mut bookmarks, "https://a.test/", "A").unwrap();
        let not_a_url = star(&mut bookmarks, "no url", "x").unwrap();
        let on_bar = bookmarks.children(BookmarkId::TOOLBAR);
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
        let [saved] = on_bar.as_slice() else { panic!("one bookmark: {on_bar:?}") };
        assert_eq!(saved.title, "A");
        assert_eq!(first, Some(Target::Added(saved.id)));
        assert_eq!(again, Some(Target::Existing(saved.id)));
        assert_eq!(not_a_url, None);
    }
}
