//! The Bookmarks window: the tree (folders expand in place), a flat search, the edits core
//! supports (new folder, edit name, URL and folder, move by menu or by dragging rows,
//! delete) and import from another browser or a bookmarks file. Every edit is one core
//! call followed by a rebuild of the tree from the merged records, so the window always
//! shows the tree every device would show.

use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::Profile;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, InsertAt, NodeKind};
use vsesvit_core::import::{self, Source};

use super::{LibraryWindow, Windowed, confirm, prompt_choice, prompt_text};
use crate::bookmark_drag::{self, Zone};
use crate::bookmark_editor::{self, Subject};
use crate::browser::Browser;
use crate::favicons;
use crate::profile::Core;
use crate::tab::display_uri;
use crate::window::{BrowserWindow, Focus};

const SEARCH_LIMIT: usize = 60;

/// Holds no browser or profile of its own: the widgets' handlers keep this state alive
/// for as long as the window's widgets exist, which must not keep the profile open.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    ui: LibraryWindow,
    root: gio::ListStore,
    tree: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    stack: gtk::Stack,
    results: gtk::ListBox,
    result_rows: RefCell<Vec<gtk::Widget>>,
    /// Rebuilds the tree after any bookmark change (an edit on the bar, icons that
    /// arrived), for as long as the window exists.
    watch: OnceCell<Rc<dyn Fn()>>,
}

/// What a tree row holds: the node, and its stored favicon, fetched with the folder's
/// children rather than on every bind.
struct Entry {
    node: BookmarkNode,
    icon: Option<gdk::Texture>,
}

pub(crate) fn present(window: &BrowserWindow) {
    super::present_window(window, Windowed::Bookmarks, || build(window).ui.window.clone());
}

/// Bookmarks to bring in from another browser's profile or a file, into a new folder on
/// the bookmarks bar.
#[derive(Clone)]
pub(crate) struct Import {
    pub(crate) folder: String,
    /// The browser profile or file name, for messages.
    pub(crate) from: String,
    source: Source,
}

impl From<import::Found> for Import {
    fn from(found: import::Found) -> Self {
        Import { folder: found.folder_title(), from: found.name, source: found.source }
    }
}

impl Import {
    pub(crate) fn file(path: PathBuf) -> Self {
        let from = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        Import { folder: import::FILE_FOLDER_TITLE.to_owned(), from, source: Source::File(path) }
    }

    /// Reads the source on a worker thread, then adds what it holds. How many items were
    /// added, or the message for the user.
    pub(crate) async fn run(self, browser: Browser) -> Result<usize, String> {
        let Import { folder, from, source } = self;
        let items = match gio::spawn_blocking(move || source.read()).await {
            Ok(Ok(items)) => items,
            Ok(Err(e)) => return Err(format!("Could not read {from}: {e}")),
            Err(_) => return Err(format!("Could not read {from}")),
        };
        let added = browser.core().borrow_mut().bookmarks().import_folder(&folder, items);
        browser.bookmarks_changed();
        added.map_err(|e| format!("Bookmarks: {e}"))
    }
}

pub(crate) async fn pick_bookmarks_file(parent: Option<&gtk::Window>) -> Option<PathBuf> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Bookmarks (.html, .json)"));
    for suffix in ["html", "htm", "json"] {
        filter.add_suffix(suffix);
    }
    // Chromium's own file has no extension.
    filter.add_pattern("Bookmarks");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let chooser = gtk::FileDialog::builder()
        .title("Import Bookmarks File")
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .build();
    chooser.open_future(parent).await.ok()?.path()
}

fn build(window: &BrowserWindow) -> Rc<State> {
    let root = gio::ListStore::new::<glib::BoxedAnyObject>();
    let tree = gtk::TreeListModel::new(root.clone(), false, false, {
        let core = Rc::downgrade(window.browser().core());
        move |item| children_model(&core.upgrade()?, item)
    });
    let selection = gtk::SingleSelection::new(Some(tree.clone()));
    let list = gtk::ListView::new(Some(selection.clone()), None::<gtk::ListItemFactory>);
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
    let edit = tool_button("document-edit-symbolic", "Edit…");
    let move_to = tool_button("go-jump-symbolic", "Move To…");
    let delete = tool_button("user-trash-symbolic", "Delete");
    let import = tool_button("document-open-symbolic", "Import Bookmarks…");
    let ui = LibraryWindow::new(
        "Bookmarks",
        "Search bookmarks",
        &[
            import.upcast_ref(),
            delete.upcast_ref(),
            move_to.upcast_ref(),
            edit.upcast_ref(),
            new_folder.upcast_ref(),
        ],
    );
    ui.content.set_child(Some(&stack));

    let state = Rc::new(State {
        window: window.downgrade(),
        ui,
        root,
        tree,
        selection,
        stack,
        results,
        result_rows: RefCell::new(Vec::new()),
        watch: OnceCell::new(),
    });
    state.rebuild();
    let weak = Rc::downgrade(&state);
    let watch: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(state) = weak.upgrade() {
            state.rebuild();
        }
    });
    window.browser().watch_bookmarks(&watch);
    let _ = state.watch.set(watch);
    list.set_factory(Some(&row_factory(Rc::downgrade(&state))));

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
    edit.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.edit().await })
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
    import.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.spawn(|s| async move { s.import().await })
    ));
    state
}

impl State {
    fn spawn<F, Fut>(self: &Rc<Self>, f: F)
    where
        F: FnOnce(Rc<Self>) -> Fut,
        Fut: Future<Output = ()> + 'static,
    {
        glib::spawn_future_local(f(self.clone()));
    }

    fn core(&self) -> Option<Core> {
        self.window.upgrade().map(|window| window.browser().core().clone())
    }

    /// Rebuilds the tree from core. The first build expands the three roots; later ones
    /// keep the folders that were open.
    fn rebuild(&self) {
        let Some(core) = self.core() else { return };
        let first = self.tree.n_items() == 0;
        let expanded: HashSet<BookmarkId> = (0..self.tree.n_items())
            .filter_map(|i| self.tree.row(i))
            .filter(|row| row.is_expanded())
            .filter_map(|row| row.item().and_then(|item| node_of(&item)))
            .map(|node| node.id)
            .collect();
        let roots = core.borrow_mut().bookmarks().children(BookmarkId::ROOT);
        self.root.remove_all();
        for node in roots {
            self.root.append(&glib::BoxedAnyObject::new(Entry { node, icon: None }));
        }
        let mut i = 0;
        while i < self.tree.n_items() {
            if let Some(row) = self.tree.row(i) {
                let open = row.item().and_then(|item| node_of(&item)).is_some_and(|node| expanded.contains(&node.id));
                if open || (first && row.depth() == 0) {
                    row.set_expanded(true);
                }
            }
            i += 1;
        }
    }

    /// A row dropped on `zone` of `target`: one core move, then the tree and the bars.
    fn drop_on(&self, id: BookmarkId, target: &BookmarkNode, zone: Zone) {
        let Some(core) = self.core() else { return };
        match bookmark_drag::apply(&core, id, target, zone) {
            Ok(false) => {}
            result => self.report(result.map(drop)),
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
        let Some(core) = self.core() else { return };
        let found = {
            let mut profile = core.borrow_mut();
            let found = profile.bookmarks().search(text, SEARCH_LIMIT);
            with_icons(&mut profile, found)
        };
        for row in self.result_rows.take() {
            self.results.remove(&row);
        }
        let mut rows = Vec::new();
        for Entry { node, icon } in found {
            let Some(url) = node.url.clone() else { continue };
            let row = adw::ActionRow::builder()
                .title(&node.title)
                .subtitle(display_uri(url.as_str()))
                .activatable(true)
                .use_markup(false)
                .build();
            row.add_prefix(&favicons::image(icon.as_ref(), "web-browser-symbolic"));
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

    /// The watch rebuilds the tree along with the bars.
    fn changed(&self) {
        if let Some(window) = self.window.upgrade() {
            window.browser().bookmarks_changed();
        }
    }

    fn report(&self, result: Result<(), vsesvit_core::Error>) {
        if let Err(e) = result {
            self.ui.toast(&format!("Bookmarks: {e}"));
        }
        self.changed();
    }

    /// Asks for a browser profile found on this machine or a bookmarks file, and adds its
    /// bookmarks to a new folder on the bookmarks bar.
    async fn import(&self) {
        let found = import::installed_browsers();
        let mut names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        names.push("Bookmarks File (HTML)…");
        let Some(index) = prompt_choice(
            &self.ui.window,
            "Import Bookmarks",
            "The bookmarks go into a new folder on the bookmarks bar.",
            &names,
            "_Import",
        )
        .await
        else {
            return;
        };
        let import = match found.into_iter().nth(index as usize) {
            Some(found) => Import::from(found),
            None => {
                let Some(path) = pick_bookmarks_file(Some(self.ui.window.upcast_ref())).await else { return };
                Import::file(path)
            }
        };
        let Some(window) = self.window.upgrade() else { return };
        let (folder, from) = (import.folder.clone(), import.from.clone());
        match import.run(window.browser().clone()).await {
            Ok(0) => self.ui.toast(&format!("No bookmarks found in {from}")),
            Ok(n) => self.ui.toast(&format!("Imported {n} items from {from} into “{folder}”")),
            Err(e) => self.ui.toast(&e),
        }
    }

    /// Into the selected folder, or next to the selected bookmark; the bar when nothing is selected.
    async fn new_folder(&self) {
        let parent = match self.selected() {
            Some(node) if node.kind == NodeKind::Folder => node.id,
            Some(node) => node.parent,
            None => BookmarkId::TOOLBAR,
        };
        let Some(title) = prompt_text(&self.ui.window, "New Folder", "New Folder", "_Create").await else {
            return;
        };
        let Some(core) = self.core() else { return };
        let result = core
            .borrow_mut()
            .bookmarks()
            .add_folder(parent, InsertAt::End, &title)
            .map(drop);
        self.report(result);
    }

    /// A bookmark's name, URL and folder, or a folder's name and parent, in the editor the
    /// bookmarks bar uses.
    async fn edit(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root() && n.kind != NodeKind::Separator) else {
            self.ui.toast("Select a bookmark or folder to edit");
            return;
        };
        let Some(core) = self.core() else { return };
        match bookmark_editor::edit(&self.ui.window, &core, Subject::Existing(node)).await {
            Ok(false) => {}
            result => self.report(result.map(drop)),
        }
    }

    async fn move_selected(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root()) else {
            self.ui.toast("Select a bookmark or folder to move");
            return;
        };
        let folders = {
            let Some(core) = self.core() else { return };
            let mut profile = core.borrow_mut();
            let bookmarks = profile.bookmarks();
            let mut out = Vec::new();
            for root in bookmarks.children(BookmarkId::ROOT) {
                collect_folders(&bookmarks, &root, node.id, String::new(), &mut out);
            }
            out
        };
        let names: Vec<&str> = folders.iter().map(|(_, path)| path.as_str()).collect();
        let Some(index) =
            prompt_choice(&self.ui.window, "Move To", &format!("Move “{}” into:", node.title), &names, "_Move").await
        else {
            return;
        };
        let Some((target, _)) = folders.get(index as usize) else { return };
        let Some(core) = self.core() else { return };
        let result = core.borrow_mut().bookmarks().move_to(node.id, *target, InsertAt::End);
        self.report(result);
    }

    async fn delete(&self) {
        let Some(node) = self.selected().filter(|n| !n.id.is_root()) else {
            self.ui.toast("Select a bookmark or folder to delete");
            return;
        };
        if node.kind == NodeKind::Folder {
            let ok = confirm(
                &self.ui.window,
                "Delete Folder?",
                &format!("“{}” and everything in it will be deleted.", node.title),
                "_Delete",
            )
            .await;
            if !ok {
                return;
            }
        }
        let Some(core) = self.core() else { return };
        let result = core.borrow_mut().bookmarks().remove(node.id);
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
        .map(|boxed| boxed.borrow::<Entry>().node.clone())
}

fn icon_of(item: &glib::Object) -> Option<gdk::Texture> {
    item.downcast_ref::<glib::BoxedAnyObject>()
        .and_then(|boxed| boxed.borrow::<Entry>().icon.clone())
}

/// `nodes` with the stored favicons of their URLs.
fn with_icons(profile: &mut Profile, nodes: Vec<BookmarkNode>) -> Vec<Entry> {
    nodes
        .into_iter()
        .map(|node| {
            let icon = node.url.as_ref().and_then(|url| favicons::stored(profile, url));
            Entry { node, icon }
        })
        .collect()
}

/// A folder's children, fetched from core when the row is first expanded.
fn children_model(core: &Core, item: &glib::Object) -> Option<gio::ListModel> {
    let node = node_of(item)?;
    if node.kind != NodeKind::Folder {
        return None;
    }
    let children = {
        let mut profile = core.borrow_mut();
        let children = profile.bookmarks().children(node.id);
        with_icons(&mut profile, children)
    };
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for child in children {
        store.append(&glib::BoxedAnyObject::new(child));
    }
    Some(store.upcast())
}

/// The node a list item currently shows.
fn bound_node(item: &glib::WeakRef<gtk::ListItem>) -> Option<BookmarkNode> {
    let row = item.upgrade()?.item().and_downcast::<gtk::TreeListRow>()?;
    node_of(&row.item()?)
}

/// The roots stay where they are.
fn draggable(node: &BookmarkNode) -> Option<BookmarkId> {
    (!node.id.is_root()).then_some(node.id)
}

fn row_factory(state: Weak<State>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
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
        let expander = gtk::TreeExpander::builder().child(&content).css_classes(["bookmark-row"]).build();
        let bound = item.downgrade();
        expander.add_controller(bookmark_drag::drag_source(move || bound_node(&bound).as_ref().and_then(draggable)));
        let bound = item.downgrade();
        let state = state.clone();
        expander.add_controller(bookmark_drag::drop_target(
            gtk::Orientation::Vertical,
            move || bound_node(&bound),
            move |id, target, zone| {
                if let Some(state) = state.upgrade() {
                    state.drop_on(id, &target, zone);
                }
            },
        ));
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
        let Some(entry) = row.item() else { return };
        let Some(node) = node_of(&entry) else { return };
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
        match icon_of(&entry) {
            Some(texture) => icon.set_paintable(Some(&texture)),
            None => icon.set_icon_name(Some(icon_name)),
        }
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

#[cfg(test)]
mod tests {
    use vsesvit_core::Url;
    use vsesvit_core::bookmarks::BookmarkError;

    use super::*;
    use crate::test_support::browser;

    fn titles(core: &Core, folder: BookmarkId) -> Vec<String> {
        core.borrow_mut().bookmarks().children(folder).into_iter().map(|node| node.title).collect()
    }

    fn is_expanded(state: &State, id: BookmarkId) -> bool {
        (0..state.tree.n_items())
            .filter_map(|i| state.tree.row(i))
            .find(|row| row.item().and_then(|item| node_of(&item)).is_some_and(|node| node.id == id))
            .is_some_and(|row| row.is_expanded())
    }

    #[gtk::test]
    fn dropping_rows_moves_bookmarks_in_core() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let state = build(&window);
        let core = browser.core().clone();
        let (folder, a, b, c, inner) = {
            let mut profile = core.borrow_mut();
            let mut bookmarks = profile.bookmarks();
            let url = |s: &str| Url::parse(s).unwrap();
            let folder = bookmarks.add_folder(BookmarkId::OTHER, InsertAt::End, "Drag test").unwrap();
            let a = bookmarks.add_url(folder, InsertAt::End, "A", &url("https://a.example/")).unwrap();
            let b = bookmarks.add_url(folder, InsertAt::End, "B", &url("https://b.example/")).unwrap();
            let c = bookmarks.add_url(folder, InsertAt::End, "C", &url("https://c.example/")).unwrap();
            let inner = bookmarks.add_folder(folder, InsertAt::End, "Inner").unwrap();
            (folder, a, b, c, inner)
        };
        state.rebuild();
        let row = (0..state.tree.n_items())
            .filter_map(|i| state.tree.row(i))
            .find(|row| row.item().and_then(|item| node_of(&item)).is_some_and(|node| node.id == folder))
            .expect("the test folder is in the tree");
        row.set_expanded(true);
        let node = |id| core.borrow_mut().bookmarks().get(id).unwrap();

        state.drop_on(c, &node(a), Zone::Before);
        assert_eq!(titles(&core, folder), ["C", "A", "B", "Inner"]);
        state.drop_on(c, &node(b), Zone::After);
        assert_eq!(titles(&core, folder), ["A", "B", "C", "Inner"]);
        state.drop_on(a, &node(inner), Zone::Into);
        assert_eq!(titles(&core, folder), ["B", "C", "Inner"]);
        assert_eq!(titles(&core, inner), ["A"]);
        assert!(is_expanded(&state, folder), "a drop keeps open folders open");

        state.drop_on(folder, &node(inner), Zone::Into);
        assert_eq!(node(folder).parent, BookmarkId::OTHER);
        assert!(matches!(
            bookmark_drag::apply(&core, folder, &node(inner), Zone::Into),
            Err(vsesvit_core::Error::Bookmark(BookmarkError::WouldCycle))
        ));
        assert_eq!(draggable(&node(BookmarkId::TOOLBAR)), None);
        assert_eq!(draggable(&node(folder)), Some(folder));

        core.borrow_mut().bookmarks().remove(folder).unwrap();
        window.destroy();
    }
}
