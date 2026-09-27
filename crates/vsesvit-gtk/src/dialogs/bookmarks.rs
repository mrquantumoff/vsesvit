//! The Bookmarks dialog: the tree (folders expand in place), a flat search, and the edits
//! core supports: new folder, rename, move, delete. Every edit is one core call followed
//! by a rebuild of the tree from the merged records, so the dialog always shows the tree
//! every device would show.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, InsertAt, NodeKind};

use super::{LibraryDialog, confirm, prompt_choice, prompt_text};
use crate::browser::Browser;
use crate::profile::Core;
use crate::tab::display_uri;
use crate::window::{BrowserWindow, Focus};

const SEARCH_LIMIT: usize = 60;

struct State {
    browser: Browser,
    window: glib::WeakRef<BrowserWindow>,
    ui: LibraryDialog,
    root: gio::ListStore,
    tree: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    stack: gtk::Stack,
    results: gtk::ListBox,
    result_rows: RefCell<Vec<gtk::Widget>>,
}

pub(crate) fn present(window: &BrowserWindow) {
    let browser = window.browser().clone();
    let core = browser.core().clone();

    let root = gio::ListStore::new::<glib::BoxedAnyObject>();
    let tree = gtk::TreeListModel::new(root.clone(), false, false, {
        let core = core.clone();
        move |item| children_model(&core, item)
    });
    let selection = gtk::SingleSelection::new(Some(tree.clone()));
    let list = gtk::ListView::new(Some(selection.clone()), Some(row_factory()));
    list.add_css_class("navigation-sidebar");
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .vexpand(true)
        .build();

    let results = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();
    let results_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&results)
        .vexpand(true)
        .build();
    let no_results = adw::StatusPage::builder()
        .icon_name("edit-find-symbolic")
        .title("No Matching Bookmarks")
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&scroller, Some("tree"));
    stack.add_named(&results_scroller, Some("results"));
    stack.add_named(&no_results, Some("empty"));

    let new_folder = tool_button("folder-new-symbolic", "New Folder");
    let rename = tool_button("document-edit-symbolic", "Rename");
    let move_to = tool_button("go-jump-symbolic", "Move To…");
    let delete = tool_button("user-trash-symbolic", "Delete");
    let ui = LibraryDialog::new(
        "Bookmarks",
        "Search bookmarks",
        &[
            delete.upcast_ref(),
            move_to.upcast_ref(),
            rename.upcast_ref(),
            new_folder.upcast_ref(),
        ],
    );
    ui.content.set_child(Some(&stack));

    let state = Rc::new(State {
        browser,
        window: window.downgrade(),
        ui,
        root,
        tree,
        selection,
        stack,
        results,
        result_rows: RefCell::new(Vec::new()),
    });
    state.rebuild();

    list.connect_activate(glib::clone!(
        #[strong]
        state,
        move |_, position| {
            let node = state
                .tree
                .row(position)
                .and_then(|row| row.item())
                .and_then(|item| node_of(&item));
            if let Some(node) = node {
                state.open(&node);
            }
        }
    ));
    state.ui.search.connect_search_changed(glib::clone!(
        #[strong]
        state,
        move |entry| state.search(&entry.text())
    ));
    new_folder.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.new_folder().await })
    ));
    rename.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.rename().await })
    ));
    move_to.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.move_selected().await })
    ));
    delete.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.delete().await })
    ));

    state.ui.dialog.present(Some(window));
}

impl State {
    fn spawn<F, Fut>(self: &Rc<Self>, f: F)
    where
        F: FnOnce(Rc<Self>) -> Fut,
        Fut: Future<Output = ()> + 'static,
    {
        glib::spawn_future_local(f(self.clone()));
    }

    fn core(&self) -> &Core {
        self.browser.core()
    }

    /// Rebuilds the tree from core and expands the three roots.
    fn rebuild(&self) {
        let roots = self.core().borrow_mut().bookmarks().children(BookmarkId::ROOT);
        self.root.remove_all();
        for node in roots {
            self.root.append(&glib::BoxedAnyObject::new(node));
        }
        let mut i = 0;
        while i < self.tree.n_items() {
            if let Some(row) = self.tree.row(i)
                && row.depth() == 0
            {
                row.set_expanded(true);
            }
            i += 1;
        }
    }

    fn selected(&self) -> Option<BookmarkNode> {
        self.selection
            .selected_item()
            .and_downcast::<gtk::TreeListRow>()
            .and_then(|row| row.item())
            .and_then(|item| node_of(&item))
    }

    fn open(&self, node: &BookmarkNode) {
        if let (Some(url), Some(window)) = (&node.url, self.window.upgrade()) {
            window.open_tab(Some(url.as_str()), None, Focus::Foreground);
        }
    }

    fn search(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            self.stack.set_visible_child_name("tree");
            return;
        }
        let found = self.core().borrow_mut().bookmarks().search(text, SEARCH_LIMIT);
        for row in self.result_rows.take() {
            self.results.remove(&row);
        }
        let mut rows = Vec::new();
        for node in found {
            let Some(url) = node.url.clone() else { continue };
            let row = adw::ActionRow::builder()
                .title(&node.title)
                .subtitle(display_uri(url.as_str()))
                .activatable(true)
                .use_markup(false)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("web-browser-symbolic"));
            let window = self.window.clone();
            row.connect_activated(move |_| {
                if let Some(window) = window.upgrade() {
                    window.open_tab(Some(url.as_str()), None, Focus::Foreground);
                }
            });
            self.results.append(&row);
            rows.push(row.upcast());
        }
        let page = if rows.is_empty() { "empty" } else { "results" };
        self.result_rows.replace(rows);
        self.stack.set_visible_child_name(page);
    }

    fn changed(&self) {
        self.browser.bookmarks_changed();
        self.rebuild();
    }

    fn report(&self, result: Result<(), vsesvit_core::Error>) {
        if let Err(e) = result {
            self.ui.toast(&format!("Bookmarks: {e}"));
        }
        self.changed();
    }

    /// Into the selected folder, or next to the selected bookmark; the bar when nothing is selected.
    async fn new_folder(&self) {
        let parent = match self.selected() {
            Some(node) if node.kind == NodeKind::Folder => node.id,
            Some(node) => node.parent,
            None => BookmarkId::TOOLBAR,
        };
        let Some(title) = prompt_text(&self.ui.dialog, "New Folder", "New Folder", "_Create").await else {
            return;
        };
        let result = self
            .core()
            .borrow_mut()
            .bookmarks()
            .add_folder(parent, InsertAt::End, &title)
            .map(drop);
        self.report(result);
    }

    async fn rename(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root() && n.kind != NodeKind::Separator) else {
            self.ui.toast("Select a bookmark or folder to rename");
            return;
        };
        let Some(title) = prompt_text(&self.ui.dialog, "Rename", &node.title, "_Rename").await else {
            return;
        };
        let result = self.core().borrow_mut().bookmarks().rename(node.id, &title);
        self.report(result);
    }

    async fn move_selected(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root()) else {
            self.ui.toast("Select a bookmark or folder to move");
            return;
        };
        let folders = {
            let mut profile = self.core().borrow_mut();
            let bookmarks = profile.bookmarks();
            let mut out = Vec::new();
            for root in bookmarks.children(BookmarkId::ROOT) {
                collect_folders(&bookmarks, &root, node.id, String::new(), &mut out);
            }
            out
        };
        let names: Vec<&str> = folders.iter().map(|(_, path)| path.as_str()).collect();
        let Some(index) =
            prompt_choice(&self.ui.dialog, "Move To", &format!("Move “{}” into:", node.title), &names, "_Move").await
        else {
            return;
        };
        let Some((target, _)) = folders.get(index as usize) else { return };
        let result = self.core().borrow_mut().bookmarks().move_to(node.id, *target, InsertAt::End);
        self.report(result);
    }

    async fn delete(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root()) else {
            self.ui.toast("Select a bookmark or folder to delete");
            return;
        };
        if node.kind == NodeKind::Folder {
            let ok = confirm(
                &self.ui.dialog,
                "Delete Folder?",
                &format!("“{}” and everything in it will be deleted.", node.title),
                "_Delete",
            )
            .await;
            if !ok {
                return;
            }
        }
        let result = self.core().borrow_mut().bookmarks().remove(node.id);
        self.report(result);
    }
}

/// Live folders in tree order as "Bookmarks Bar / Work / …", leaving out `exclude`'s subtree
/// (a folder cannot move into itself).
fn collect_folders(
    bookmarks: &vsesvit_core::bookmarks::Bookmarks<'_>,
    node: &BookmarkNode,
    exclude: BookmarkId,
    prefix: String,
    out: &mut Vec<(BookmarkId, String)>,
) {
    if node.kind != NodeKind::Folder || node.id == exclude {
        return;
    }
    let path = if prefix.is_empty() { node.title.clone() } else { format!("{prefix} / {}", node.title) };
    out.push((node.id, path.clone()));
    for child in bookmarks.children(node.id) {
        collect_folders(bookmarks, &child, exclude, path.clone(), out);
    }
}

fn node_of(item: &glib::Object) -> Option<BookmarkNode> {
    item.downcast_ref::<glib::BoxedAnyObject>()
        .map(|boxed| boxed.borrow::<BookmarkNode>().clone())
}

/// A folder's children, fetched from core when the row is first expanded.
fn children_model(core: &Core, item: &glib::Object) -> Option<gio::ListModel> {
    let node = node_of(item)?;
    if node.kind != NodeKind::Folder {
        return None;
    }
    let children = core.borrow_mut().bookmarks().children(node.id);
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for child in children {
        store.append(&glib::BoxedAnyObject::new(child));
    }
    Some(store.upcast())
}

fn row_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let icon = gtk::Image::new();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let url = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim-label", "caption"])
            .build();
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.append(&title);
        text.append(&url);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        content.append(&icon);
        content.append(&text);
        let expander = gtk::TreeExpander::builder().child(&content).build();
        item.set_child(Some(&expander));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let (Some(expander), Some(row)) = (
            item.child().and_downcast::<gtk::TreeExpander>(),
            item.item().and_downcast::<gtk::TreeListRow>(),
        ) else {
            return;
        };
        expander.set_list_row(Some(&row));
        let Some(node) = row.item().and_then(|i| node_of(&i)) else { return };
        let Some(content) = expander.child().and_downcast::<gtk::Box>() else { return };
        let Some(icon) = content.first_child().and_downcast::<gtk::Image>() else { return };
        let Some(text) = icon.next_sibling().and_downcast::<gtk::Box>() else { return };
        let Some(title) = text.first_child().and_downcast::<gtk::Label>() else { return };
        let Some(url) = title.next_sibling().and_downcast::<gtk::Label>() else { return };
        let (icon_name, label, subtitle) = match node.kind {
            NodeKind::Folder => ("folder-symbolic", node.title.clone(), String::new()),
            NodeKind::Url => (
                "web-browser-symbolic",
                node.title.clone(),
                node.url.as_ref().map(|u| display_uri(u.as_str())).unwrap_or_default(),
            ),
            NodeKind::Separator => ("view-more-horizontal-symbolic", "—".to_owned(), String::new()),
        };
        icon.set_icon_name(Some(icon_name));
        title.set_label(&label);
        url.set_label(&subtitle);
        url.set_visible(!subtitle.is_empty());
    });
    factory.connect_unbind(|_, item| {
        if let Some(expander) = item
            .downcast_ref::<gtk::ListItem>()
            .and_then(|item| item.child())
            .and_downcast::<gtk::TreeExpander>()
        {
            expander.set_list_row(None);
        }
    });
    factory
}

fn tool_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}
