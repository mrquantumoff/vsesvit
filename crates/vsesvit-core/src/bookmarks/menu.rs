//! The bookmark menus' model: what a bar item's context menu holds, what the bar asks its
//! window to do, and how a link or folder is named in a menu or a tooltip
//! (docs/design/linux-cef/design.md section 4.3, row 10). A shell turns [`MenuEntry`]s into its
//! own menu widgets and runs the [`BarCommand`] of the one picked.

use super::BookmarkId;
use super::bar::BarItem;

/// Menu labels longer than this are cut, as Chrome caps its menu width.
pub const MENU_LABEL_CHARS: usize = 50;

/// Where a bookmark opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    CurrentTab,
    BackgroundTab,
    NewWindow,
}

/// What the bar asks its window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarCommand {
    Open(String, Disposition),
    /// The links directly in a folder, each in a background tab.
    OpenAll(Vec<String>),
    /// The bookmark editor for a bookmark or folder.
    Edit(BookmarkId),
    CopyLink(String),
    /// Deletes a bookmark; a folder with `contents` items in it only once the user confirms.
    Delete { id: BookmarkId, title: String, contents: usize },
    AddPage,
    AddFolder,
    ToggleBar,
    Manager,
}

/// An entry of a bookmark context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Command(String, BarCommand),
    /// A command with a check mark, checked or not.
    Toggle(String, bool, BarCommand),
    Separator,
}

/// The context menu of a bar item, or of the bar's empty space (`None`), as Chrome has them.
pub fn context_entries(item: Option<&BarItem>) -> Vec<MenuEntry> {
    use MenuEntry::{Command, Separator, Toggle};
    let command = |label: &str, command| Command(label.to_owned(), command);
    match item {
        Some(BarItem::Link { id, title, url, .. }) => vec![
            command("Open in new tab", BarCommand::Open(url.clone(), Disposition::BackgroundTab)),
            command("Open in new window", BarCommand::Open(url.clone(), Disposition::NewWindow)),
            Separator,
            command("Edit\u{2026}", BarCommand::Edit(*id)),
            command("Copy link", BarCommand::CopyLink(url.clone())),
            Separator,
            command("Delete", BarCommand::Delete { id: *id, title: title.clone(), contents: 0 }),
        ],
        Some(BarItem::Folder { id, title, children, contents }) => {
            let links: Vec<String> = children
                .iter()
                .filter_map(|child| match child {
                    BarItem::Link { url, .. } => Some(url.clone()),
                    BarItem::Folder { .. } => None,
                })
                .collect();
            vec![
                Command(format!("Open all ({}) in new tabs", links.len()), BarCommand::OpenAll(links)),
                Separator,
                command("Rename\u{2026}", BarCommand::Edit(*id)),
                Separator,
                command("Delete", BarCommand::Delete { id: *id, title: title.clone(), contents: *contents }),
            ]
        }
        None => vec![
            command("Add page\u{2026}", BarCommand::AddPage),
            command("Add folder\u{2026}", BarCommand::AddFolder),
            Separator,
            Toggle("Show bookmarks bar".to_owned(), true, BarCommand::ToggleBar),
            command("Bookmark manager", BarCommand::Manager),
        ],
    }
}

/// Whether a link has no title. On the bar it then shows only its favicon, as in Chrome.
pub fn untitled(title: &str) -> bool {
    title.trim().is_empty()
}

/// What a link goes by in a menu and for a screen reader: its title, or its URL if untitled.
pub fn link_name<'a>(title: &'a str, url: &'a str) -> &'a str {
    if untitled(title) { url } else { title }
}

/// A title as a menu shows it: cut to [`MENU_LABEL_CHARS`] with an ellipsis.
pub fn menu_label(title: &str) -> String {
    if title.chars().count() <= MENU_LABEL_CHARS {
        return title.to_owned();
    }
    let cut: String = title.chars().take(MENU_LABEL_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The lines of an item's tooltip: a link's title over its URL, or just the URL if untitled; a
/// folder's title.
pub fn tip_lines(item: &BarItem) -> Vec<&str> {
    match item {
        BarItem::Link { title, url, .. } if untitled(title) => vec![url],
        BarItem::Link { title, url, .. } => vec![title, url],
        BarItem::Folder { title, .. } => vec![title],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(title: &str) -> BarItem {
        BarItem::Link { id: BookmarkId::MOBILE, title: title.into(), url: "https://a.test/".into(), icon: None }
    }

    #[test]
    fn long_menu_labels_are_cut_with_an_ellipsis() {
        assert_eq!(menu_label("Short"), "Short");
        let long = "word ".repeat(20);
        let label = menu_label(&long);
        assert!(label.chars().count() <= MENU_LABEL_CHARS, "{label}");
        assert!(label.ends_with("word\u{2026}"), "{label}");
    }

    #[test]
    fn untitled_links_go_by_their_url() {
        // As in Chrome, an untitled link on the bar shows only its favicon.
        assert_eq!(link_name("", "https://a.test/"), "https://a.test/");
        assert_eq!(menu_label(link_name(" ", "https://b.test/")), "https://b.test/");
        assert_eq!(link_name("A", "https://a.test/"), "A");
        assert_eq!(tip_lines(&link("")), ["https://a.test/"]);
        assert_eq!(tip_lines(&link("A")), ["A", "https://a.test/"]);
        let folder = BarItem::Folder { id: BookmarkId::OTHER, title: "F".into(), children: vec![], contents: 0 };
        assert_eq!(tip_lines(&folder), ["F"]);
    }

    #[test]
    fn separators_count_toward_a_folders_delete() {
        // A folder holding only a separator: its menu is empty, but deleting it still asks.
        let folder = BarItem::Folder { id: BookmarkId::OTHER, title: "F".into(), children: vec![], contents: 1 };
        assert!(context_entries(Some(&folder)).contains(&MenuEntry::Command(
            "Delete".into(),
            BarCommand::Delete { id: BookmarkId::OTHER, title: "F".into(), contents: 1 }
        )));
    }

    #[test]
    fn context_menus_match_chromes() {
        let labels = |item: Option<&BarItem>| -> Vec<String> {
            context_entries(item)
                .into_iter()
                .map(|e| match e {
                    MenuEntry::Command(label, _) | MenuEntry::Toggle(label, _, _) => label,
                    MenuEntry::Separator => "-".into(),
                })
                .collect()
        };
        let link = link("A");
        assert_eq!(
            labels(Some(&link)),
            ["Open in new tab", "Open in new window", "-", "Edit\u{2026}", "Copy link", "-", "Delete"]
        );
        let folder = BarItem::Folder {
            id: BookmarkId::OTHER,
            title: "F".into(),
            children: vec![link.clone(), link.clone()],
            contents: 2,
        };
        assert_eq!(labels(Some(&folder)), ["Open all (2) in new tabs", "-", "Rename\u{2026}", "-", "Delete"]);
        assert!(context_entries(Some(&folder)).contains(&MenuEntry::Command(
            "Delete".into(),
            BarCommand::Delete { id: BookmarkId::OTHER, title: "F".into(), contents: 2 }
        )));
        assert_eq!(
            labels(None),
            ["Add page\u{2026}", "Add folder\u{2026}", "-", "Show bookmarks bar", "Bookmark manager"]
        );
    }
}
