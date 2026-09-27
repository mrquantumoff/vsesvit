use adw::prelude::*;

/// Accelerators come from the actions themselves (set in `app.rs`), so this never drifts.
pub(crate) fn present(parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::ShortcutsDialog::new();
    for (title, items) in SECTIONS {
        let section = adw::ShortcutsSection::new(Some(title));
        for item in *items {
            section.add(match item {
                Item::Action(title, action) => adw::ShortcutsItem::from_action(title, action),
                Item::Keys(title, accelerator) => adw::ShortcutsItem::new(title, accelerator),
            });
        }
        dialog.add(section);
    }
    dialog.present(Some(parent));
}

enum Item {
    Action(&'static str, &'static str),
    /// Shortcuts built into widgets rather than bound to actions.
    Keys(&'static str, &'static str),
}

const SECTIONS: &[(&str, &[Item])] = &[
    (
        "Tabs and Windows",
        &[
            Item::Action("New tab", "win.new-tab"),
            Item::Action("Close tab", "win.close-tab"),
            Item::Action("Reopen closed tab", "win.reopen-closed-tab"),
            Item::Keys("Next tab", "<Control>Tab"),
            Item::Keys("Previous tab", "<Control><Shift>Tab"),
            Item::Action("New window", "app.new-window"),
            Item::Action("Quit", "app.quit"),
        ],
    ),
    (
        "Navigation",
        &[
            Item::Action("Focus the address bar", "win.focus-location"),
            Item::Action("Back", "win.back"),
            Item::Action("Forward", "win.forward"),
            Item::Action("Reload", "win.reload"),
            Item::Action("Reload, ignoring the cache", "win.reload-bypass-cache"),
        ],
    ),
    (
        "Page",
        &[
            Item::Action("Find", "win.find"),
            Item::Action("Next match", "win.find-next"),
            Item::Action("Previous match", "win.find-previous"),
            Item::Action("Bookmark this page", "win.bookmark-page"),
            Item::Action("Zoom in", "win.zoom-in"),
            Item::Action("Zoom out", "win.zoom-out"),
            Item::Action("Reset zoom", "win.zoom-reset"),
            Item::Action("Fullscreen", "win.fullscreen"),
        ],
    ),
    (
        "General",
        &[
            Item::Action("Show or hide the bookmarks bar", "win.show-bookmarks-bar"),
            Item::Action("Bookmarks", "win.show-bookmarks"),
            Item::Action("History", "win.show-history"),
            Item::Action("Settings", "win.show-settings"),
            Item::Action("Keyboard shortcuts", "app.shortcuts"),
        ],
    ),
];
