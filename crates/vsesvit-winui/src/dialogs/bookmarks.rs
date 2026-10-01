//! Bookmarks: the tree of folders and bookmarks, with add folder, rename (and edit the URL),
//! move to another folder or up and down (or drag in the tree), delete, and import from another
//! browser or a bookmarks file. Every change goes to core, then the tree, the bookmarks bars and the stars
//! are rebuilt from core, as they are when the icons of bookmarked pages arrive or a sync changed
//! bookmarks.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, InsertAt, NodeKind};
use vsesvit_core::import::{self, Found, Source};
use vsesvit_core::search::classify_url;
use vsesvit_core::sync::Changed;
use windows_core::{Interface, Result};

use super::{Wired, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::{exec, pickers, xaml};

pub(super) const MARKUP: &str = r#"
  <Grid ColumnSpacing="20">
    <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="280"/></Grid.ColumnDefinitions>
    <TreeView x:Name="BookmarksTree" SelectionMode="Single" CanDragItems="True"
              CanReorderItems="True" AllowDrop="True">
      <!-- Each node's content is its icon and label, built in code. -->
      <TreeView.ItemTemplate>
        <DataTemplate><TreeViewItem Content="{Binding Content}"/></DataTemplate>
      </TreeView.ItemTemplate>
    </TreeView>
    <StackPanel Grid.Column="1" Spacing="10">
      <TextBox x:Name="BookmarkName" Header="Name" IsEnabled="False"/>
      <TextBox x:Name="BookmarkUrl" Header="URL" IsEnabled="False"/>
      <Button x:Name="BookmarkSave" Content="Save changes" IsEnabled="False"/>
      <ComboBox x:Name="BookmarkFolder" Header="Folder" HorizontalAlignment="Stretch" IsEnabled="False"/>
      <StackPanel Orientation="Horizontal" Spacing="8">
        <Button x:Name="BookmarkMove" Content="Move to folder" IsEnabled="False"/>
        <Button x:Name="BookmarkUp" Content="Up" IsEnabled="False"/>
        <Button x:Name="BookmarkDown" Content="Down" IsEnabled="False"/>
      </StackPanel>
      <StackPanel Orientation="Horizontal" Spacing="8">
        <Button x:Name="BookmarkAddFolder" Content="New folder"/>
        <Button x:Name="BookmarkDelete" Content="Delete" IsEnabled="False"/>
      </StackPanel>
      <ComboBox x:Name="ImportFrom" Header="Import bookmarks from" HorizontalAlignment="Stretch"/>
      <Button x:Name="ImportRun" Content="Import"/>
      <TextBlock x:Name="BookmarkStatus" TextWrapping="Wrap"
                 Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    </StackPanel>
  </Grid>"#;

const ROOTS: [BookmarkId; 3] = [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE];

/// An entry of the "Import bookmarks from" box.
enum ImportChoice {
    Browser(Found),
    /// A bookmarks HTML export or a Chromium `Bookmarks` file, chosen in a picker.
    File,
}

struct Editor {
    browser: Weak<Browser>,
    /// The window the import's file picker opens over.
    owner: WindowId,
    tree: TreeView,
    name: TextBox,
    url: TextBox,
    folder: ComboBox,
    import_from: ComboBox,
    import_choices: Vec<ImportChoice>,
    status: TextBlock,
    edit_controls: Vec<Control>,
    /// Tree nodes and the bookmark each shows.
    nodes: RefCell<Vec<(TreeViewNode, BookmarkNode)>>,
    /// The folder list of the "Folder" box, in its order.
    folders: RefCell<Vec<BookmarkId>>,
    selected: Cell<Option<BookmarkId>>,
    /// Set while the tree is rebuilt, whose selection events mean nothing then.
    rendering: Cell<bool>,
    /// The tree node being dragged.
    dragging: RefCell<Option<TreeViewNode>>,
}

/// Where a node dropped in the tree goes in core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropSpot {
    /// Into `folder`, before `before` (at the end when `None`).
    Into {
        folder: BookmarkId,
        before: Option<BookmarkId>,
    },
    /// Onto a bookmark, which cannot hold others: just after it, in its folder.
    After(BookmarkId),
}

/// The sibling a node dropped onto `link` goes before (the folder's end when `None`).
fn next_after(siblings: &[BookmarkId], link: BookmarkId, moved: BookmarkId) -> Option<BookmarkId> {
    let i = siblings.iter().position(|&id| id == link)?;
    siblings[i + 1..]
        .iter()
        .copied()
        .find(|&next| next != moved)
}

/// A button's handler.
type Action = fn(&Editor);

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>, host: &Window) -> Result<Wired> {
    let control = |name: &str| xaml::find::<Control>(root, name);
    let mut import_choices: Vec<ImportChoice> = import::installed_browsers()
        .into_iter()
        .map(ImportChoice::Browser)
        .collect();
    import_choices.push(ImportChoice::File);
    let editor = Rc::new(Editor {
        browser: Rc::downgrade(browser),
        owner: super::window_id(host)?,
        tree: xaml::find(root, "BookmarksTree")?,
        name: xaml::find(root, "BookmarkName")?,
        url: xaml::find(root, "BookmarkUrl")?,
        folder: xaml::find(root, "BookmarkFolder")?,
        import_from: xaml::find(root, "ImportFrom")?,
        import_choices,
        status: xaml::find(root, "BookmarkStatus")?,
        edit_controls: vec![
            control("BookmarkName")?,
            control("BookmarkSave")?,
            control("BookmarkFolder")?,
            control("BookmarkMove")?,
            control("BookmarkUp")?,
            control("BookmarkDown")?,
            control("BookmarkDelete")?,
        ],
        nodes: RefCell::new(Vec::new()),
        folders: RefCell::new(Vec::new()),
        selected: Cell::new(None),
        rendering: Cell::new(false),
        dragging: RefCell::new(None),
    });
    editor.render();
    editor.fill_import_choices();

    let e = Rc::downgrade(&editor);
    editor
        .tree
        .cast::<ITreeView3>()?
        .SelectionChanged(move |_, _| {
            if let Some(e) = e.upgrade() {
                e.selection_changed();
            }
        })?
        .forget();
    let tree = editor.tree.cast::<ITreeView2>()?;
    let e = Rc::downgrade(&editor);
    tree.DragItemsStarting(move |_, args| {
        if let (Some(e), Some(args)) = (e.upgrade(), args.as_ref()) {
            e.drag_starting(args);
        }
    })?
    .forget();
    let e = Rc::downgrade(&editor);
    tree.DragItemsCompleted(move |_, _| {
        if let Some(e) = e.upgrade() {
            e.dropped();
        }
    })?
    .forget();
    let actions: [(&str, Action); 6] = [
        ("BookmarkSave", Editor::save),
        ("BookmarkMove", Editor::move_to_folder),
        ("BookmarkUp", |e| e.shift(-1)),
        ("BookmarkDown", |e| e.shift(1)),
        ("BookmarkAddFolder", Editor::add_folder),
        ("BookmarkDelete", Editor::delete),
    ];
    for (name, action) in actions {
        let button: Button = xaml::find(root, name)?;
        let e = Rc::downgrade(&editor);
        on_click(&button, move || {
            if let Some(e) = e.upgrade() {
                action(&e);
            }
        })?;
    }
    let e = Rc::downgrade(&editor);
    on_click(&xaml::find::<Button>(root, "ImportRun")?, move || {
        if let Some(e) = e.upgrade() {
            e.import();
        }
    })?;
    let e = Rc::downgrade(&editor);
    let repaint: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(e) = e.upgrade() {
            e.render();
        }
    });
    browser.on_favicons_arrived(&repaint);
    let e = Rc::downgrade(&editor);
    let synced: Rc<dyn Fn(&Changed)> = Rc::new(move |changed| {
        if changed.bookmarks
            && let Some(e) = e.upgrade()
        {
            e.render();
        }
    });
    browser.sync().on_applied(&synced);
    Ok(Wired {
        _alive: vec![editor, Rc::new(repaint), Rc::new(synced)],
        on_close: None,
    })
}

impl Editor {
    fn browser(&self) -> Option<Rc<Browser>> {
        self.browser.upgrade()
    }

    /// Rebuilds the tree and the folder list from core, keeping the selection.
    fn render(&self) {
        let Some(browser) = self.browser() else {
            return;
        };
        let (tree, folders) = browser.core(|p| {
            let mut roots: Vec<Branch> = {
                let bookmarks = p.bookmarks();
                let children = |id| bookmarks.children(id);
                ROOTS
                    .iter()
                    .filter_map(|&id| bookmarks.get(id))
                    .map(|node| Branch::of(node, &children))
                    .collect()
            };
            for branch in &mut roots {
                branch.fill_icons(&mut |url| p.favicons().get(url).ok().flatten());
            }
            let mut folders = Vec::new();
            for branch in &roots {
                branch.folders(0, &mut folders);
            }
            (roots, folders)
        });
        let Ok(roots) = self.tree.RootNodes() else {
            return;
        };
        self.rendering.set(true);
        // Clearing the nodes under a selected node corrupts the TreeView's heap.
        if let Ok(tree) = self.tree.cast::<ITreeView2>() {
            let _ = tree.SetSelectedNode(None::<&TreeViewNode>);
        }
        let _ = roots.Clear();
        self.nodes.borrow_mut().clear();
        for branch in &tree {
            match self.node_for(branch) {
                Ok(node) => {
                    let _ = roots.Append(&node);
                }
                Err(e) => log::warn!("bookmark tree: {e}"),
            }
        }
        self.fill_folders(&folders);
        self.rendering.set(false);
        self.select(self.selected.get());
    }

    fn node_for(&self, branch: &Branch) -> Result<TreeViewNode> {
        let node = TreeViewNode::new()?;
        let glyph = match branch.node.kind {
            NodeKind::Folder => "&#xE8B7;",
            NodeKind::Url => "&#xE774;",
            NodeKind::Separator => "",
        };
        let content: FrameworkElement = xaml::load(&format!(
            r#"<StackPanel {{ns}} Orientation="Horizontal" Spacing="8">
                 <Grid Width="16" Height="16" VerticalAlignment="Center">
                   <FontIcon x:Name="Glyph" Glyph="{glyph}" FontSize="14"/>
                   <Image x:Name="Favicon" Width="16" Height="16" Visibility="Collapsed"/>
                 </Grid>
                 <TextBlock x:Name="Label" Text="{label}" VerticalAlignment="Center"
                            TextTrimming="CharacterEllipsis"/>
               </StackPanel>"#,
            label = xaml::escape(&branch.label()),
        ))?;
        if let Some(png) = branch.icon.clone() {
            xaml::show_favicon(&content, png)?;
        }
        node.SetContent(&content)?;
        node.SetIsExpanded(true)?;
        let children = node.Children()?;
        for child in &branch.children {
            children.Append(&self.node_for(child)?)?;
        }
        self.nodes
            .borrow_mut()
            .push((node.clone(), branch.node.clone()));
        Ok(node)
    }

    fn fill_folders(&self, folders: &[(BookmarkId, String)]) {
        let Ok(items) = self.folder.cast::<ItemsControl>().and_then(|c| c.Items()) else {
            return;
        };
        let _ = items.Clear();
        for (_, label) in folders {
            if let Ok(label) = xaml::boxed(label) {
                let _ = items.Append(&label);
            }
        }
        *self.folders.borrow_mut() = folders.iter().map(|(id, _)| *id).collect();
    }

    /// The user (or `select`) picked a node in the tree.
    fn selection_changed(&self) {
        if self.rendering.get() {
            return;
        }
        let Ok(node) = self
            .tree
            .cast::<ITreeView2>()
            .and_then(|t| t.SelectedNode())
        else {
            return;
        };
        let id = self
            .nodes
            .borrow()
            .iter()
            .find(|(tree_node, _)| xaml::same_object(tree_node, &node))
            .map(|(_, b)| b.id);
        if id.is_some() && id != self.selected.get() {
            self.select(id);
        }
    }

    fn selected_node(&self) -> Option<BookmarkNode> {
        let id = self.selected.get()?;
        self.nodes
            .borrow()
            .iter()
            .find(|(_, b)| b.id == id)
            .map(|(_, b)| b.clone())
    }

    /// Shows `id` in the editor; roots can only get new folders.
    fn select(&self, id: Option<BookmarkId>) {
        self.selected.set(id);
        let node = self.selected_node();
        let tree_node = node.as_ref().and_then(|n| {
            self.nodes
                .borrow()
                .iter()
                .find(|(_, b)| b.id == n.id)
                .map(|(t, _)| t.clone())
        });
        if let (Some(tree_node), Ok(tree)) = (tree_node, self.tree.cast::<ITreeView2>()) {
            let _ = tree.SetSelectedNode(&tree_node);
        }
        let editable = node.as_ref().is_some_and(|n| !n.id.is_root());
        for control in &self.edit_controls {
            let _ = control.SetIsEnabled(editable);
        }
        let is_link = node.as_ref().is_some_and(|n| n.kind == NodeKind::Url);
        let _ = self
            .url
            .cast::<Control>()
            .and_then(|c| c.SetIsEnabled(editable && is_link));
        let _ = self
            .name
            .SetText(node.as_ref().map_or("", |n| n.title.as_str()));
        let url = node
            .as_ref()
            .and_then(|n| n.url.as_ref())
            .map(Url::to_string);
        let _ = self.url.SetText(url.as_deref().unwrap_or(""));
        let parent = node.as_ref().map(|n| n.parent);
        let index = parent.and_then(|p| self.folders.borrow().iter().position(|f| *f == p));
        if let Ok(selector) = self.folder.cast::<Selector>() {
            let _ =
                selector.SetSelectedIndex(index.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1));
        }
    }

    /// Runs a core change, then refreshes everything that shows bookmarks.
    fn change(
        &self,
        done: &str,
        f: impl FnOnce(
            &mut vsesvit_core::bookmarks::Bookmarks<'_>,
        ) -> std::result::Result<Option<BookmarkId>, vsesvit_core::Error>,
    ) {
        let Some(browser) = self.browser() else {
            return;
        };
        let result = browser.core(|p| f(&mut p.bookmarks()));
        let text = match result {
            Ok(Some(new_selection)) => {
                self.selected.set(Some(new_selection));
                done.to_owned()
            }
            Ok(None) => {
                self.selected.set(None);
                done.to_owned()
            }
            Err(e) => format!("Not changed: {e}"),
        };
        browser.bookmarks_changed();
        self.render();
        let _ = self.status.SetText(&text);
    }

    fn save(&self) {
        let Some(node) = self.selected_node() else {
            return;
        };
        let name = self.name.Text().unwrap_or_default();
        let url = match node.kind {
            NodeKind::Url => {
                let text = self.url.Text().unwrap_or_default();
                match classify_url(&text) {
                    Some(target) => Some(target.url().clone()),
                    None => {
                        let _ = self.status.SetText("That is not a web address.");
                        return;
                    }
                }
            }
            _ => None,
        };
        self.change("Saved.", |b| {
            b.rename(node.id, name.trim())?;
            if let Some(url) = &url {
                b.set_url(node.id, url)?;
            }
            Ok(Some(node.id))
        });
    }

    fn move_to_folder(&self) {
        let Some(node) = self.selected_node() else {
            return;
        };
        let index = self
            .folder
            .cast::<Selector>()
            .and_then(|s| s.SelectedIndex())
            .ok()
            .and_then(|i| usize::try_from(i).ok());
        let Some(folder) = index.and_then(|i| self.folders.borrow().get(i).copied()) else {
            return;
        };
        self.change("Moved.", |b| {
            b.move_to(node.id, folder, InsertAt::End)?;
            Ok(Some(node.id))
        });
    }

    /// Moves the selection `step` places among its siblings.
    fn shift(&self, step: isize) {
        let Some(node) = self.selected_node() else {
            return;
        };
        let Some(index) = node.index.checked_add_signed(step) else {
            return;
        };
        self.change("Moved.", |b| {
            b.move_to(node.id, node.parent, InsertAt::Index(index))?;
            Ok(Some(node.id))
        });
    }

    /// A new folder in the selected folder, next to the selected bookmark, or in "Other
    /// bookmarks".
    fn add_folder(&self) {
        let parent = match self.selected_node() {
            Some(node) if node.kind == NodeKind::Folder => node.id,
            Some(node) => node.parent,
            None => BookmarkId::OTHER,
        };
        self.change("Folder added. Give it a name and save.", |b| {
            b.add_folder(parent, InsertAt::End, "New folder").map(Some)
        });
    }

    fn delete(&self) {
        let Some(node) = self.selected_node() else {
            return;
        };
        self.change("Deleted.", |b| b.remove(node.id).map(|()| None));
    }

    /// The bookmark a tree node (or its content) shows.
    fn bookmark_of(&self, item: &impl Interface) -> Option<(TreeViewNode, BookmarkNode)> {
        self.nodes
            .borrow()
            .iter()
            .find(|(node, _)| {
                xaml::same_object(node, item)
                    || node
                        .Content()
                        .is_ok_and(|content| xaml::same_object(&content, item))
            })
            .cloned()
    }

    /// Roots stay where they are.
    fn drag_starting(&self, args: &TreeViewDragItemsStartingEventArgs) {
        let dragged = args
            .Items()
            .and_then(|items| items.GetAt(0))
            .ok()
            .and_then(|item| self.bookmark_of(&item));
        match dragged {
            Some((node, bookmark)) if !bookmark.id.is_root() => {
                *self.dragging.borrow_mut() = Some(node);
            }
            _ => {
                let _ = args.SetCancel(true);
            }
        }
    }

    /// Where the dragged node ended up in the tree, as a place in core.
    fn drop_spot(&self, dragged: &TreeViewNode) -> Option<DropSpot> {
        let parent = dragged.Parent().ok()?;
        let (_, target) = self.bookmark_of(&parent)?;
        if target.kind != NodeKind::Folder {
            return Some(DropSpot::After(target.id));
        }
        let siblings = parent.Children().ok()?;
        let mut index = 0;
        if !siblings.IndexOf(dragged, &mut index).ok()? {
            return None;
        }
        let before = siblings
            .GetAt(index + 1)
            .ok()
            .and_then(|next| self.bookmark_of(&next))
            .map(|(_, b)| b.id);
        Some(DropSpot::Into {
            folder: target.id,
            before,
        })
    }

    fn dropped(&self) {
        let Some(dragged) = self.dragging.borrow_mut().take() else {
            return;
        };
        let (Some((_, moved)), Some(browser)) = (self.bookmark_of(&dragged), self.browser()) else {
            return;
        };
        let result = match self.drop_spot(&dragged) {
            Some(DropSpot::Into { folder, before }) => {
                browser.move_bookmark_before(moved.id, folder, before)
            }
            Some(DropSpot::After(link)) => {
                let place = browser.core(|p| {
                    let bookmarks = p.bookmarks();
                    let link = bookmarks.get(link)?;
                    let siblings: Vec<_> = bookmarks
                        .children(link.parent)
                        .iter()
                        .map(|n| n.id)
                        .collect();
                    Some((link.parent, next_after(&siblings, link.id, moved.id)))
                });
                match place {
                    Some((folder, before)) => {
                        browser.move_bookmark_before(moved.id, folder, before)
                    }
                    None => Ok(()),
                }
            }
            // Dropped beside the roots: nothing to move; the rebuild puts it back.
            None => Ok(()),
        };
        self.selected.set(Some(moved.id));
        browser.bookmarks_changed();
        self.render();
        let text = match result {
            Ok(()) => "Moved.".to_owned(),
            Err(e) => format!("Not moved: {e}"),
        };
        let _ = self.status.SetText(&text);
    }

    fn fill_import_choices(&self) {
        let Ok(items) = self
            .import_from
            .cast::<ItemsControl>()
            .and_then(|c| c.Items())
        else {
            return;
        };
        for choice in &self.import_choices {
            let label = match choice {
                ImportChoice::Browser(found) => found.name.as_str(),
                ImportChoice::File => "Bookmarks file (HTML)\u{2026}",
            };
            if let Ok(label) = xaml::boxed(label) {
                let _ = items.Append(&label);
            }
        }
        if let Ok(selector) = self.import_from.cast::<Selector>() {
            let _ = selector.SetSelectedIndex(0);
        }
    }

    fn import(self: &Rc<Self>) {
        let index = self
            .import_from
            .cast::<Selector>()
            .and_then(|s| s.SelectedIndex())
            .ok()
            .and_then(|i| usize::try_from(i).ok());
        match index.and_then(|i| self.import_choices.get(i)) {
            Some(ImportChoice::Browser(found)) => {
                self.import_source(
                    found.folder_title(),
                    found.name.clone(),
                    found.source.clone(),
                );
            }
            Some(ImportChoice::File) => self.import_file(),
            None => {}
        }
    }

    fn import_file(self: &Rc<Self>) {
        let owner = self.owner;
        let me = self.clone();
        exec::spawn(async move {
            match pickers::pick_file(owner, &[".html", ".htm", ".json"]).await {
                Ok(Some(path)) => {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    me.import_source(
                        import::FILE_FOLDER_TITLE.to_owned(),
                        name,
                        Source::File(path),
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = me
                        .status
                        .SetText(&format!("Could not open the picker: {e}"));
                }
            }
        });
    }

    fn import_source(self: &Rc<Self>, folder: String, from: String, source: Source) {
        let Some(browser) = self.browser() else {
            return;
        };
        let _ = self
            .status
            .SetText(&format!("Importing from {from}\u{2026}"));
        let me = self.clone();
        exec::spawn(async move {
            let text = import_bookmarks(&browser, &folder, &from, source).await;
            me.render();
            let _ = me.status.SetText(&text);
        });
    }
}

/// Reads `source` (named `from`) on a worker thread and adds it to the bookmarks bar as the
/// folder `folder`; what happened, in words.
pub(crate) async fn import_bookmarks(
    browser: &Browser,
    folder: &str,
    from: &str,
    source: Source,
) -> String {
    let text = match exec::background(move || source.read().map_err(|e| e.to_string())).await {
        Err(e) => format!("Could not read {from}: {e}"),
        Ok(items) => match browser.core(|p| p.bookmarks().import_folder(folder, items)) {
            Ok(0) => format!("No bookmarks found in {from}."),
            Ok(n) => format!(
                "Imported {n} items from {from} into \u{201C}{folder}\u{201D} on the bookmarks bar."
            ),
            Err(e) => format!("Not imported: {e}"),
        },
    };
    browser.bookmarks_changed();
    text
}

/// A bookmark and its descendants, read from core in one borrow.
struct Branch {
    node: BookmarkNode,
    /// A link's saved favicon (PNG).
    icon: Option<Vec<u8>>,
    children: Vec<Branch>,
}

impl Branch {
    fn of(node: BookmarkNode, children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>) -> Self {
        let kids = if node.kind == NodeKind::Folder {
            children(node.id)
                .into_iter()
                .map(|c| Branch::of(c, children))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            node,
            icon: None,
            children: kids,
        }
    }

    fn fill_icons(&mut self, favicon: &mut dyn FnMut(&Url) -> Option<Vec<u8>>) {
        if let Some(url) = &self.node.url {
            self.icon = favicon(url);
        }
        for child in &mut self.children {
            child.fill_icons(favicon);
        }
    }

    fn label(&self) -> String {
        let node = &self.node;
        match node.kind {
            NodeKind::Folder => node.title.clone(),
            NodeKind::Separator => "\u{2014}\u{2014}\u{2014}".to_owned(),
            NodeKind::Url if node.title.is_empty() => {
                node.url.as_ref().map(Url::to_string).unwrap_or_default()
            }
            NodeKind::Url => node.title.clone(),
        }
    }

    /// Every folder, depth first, labelled with its depth.
    fn folders(&self, depth: usize, out: &mut Vec<(BookmarkId, String)>) {
        if self.node.kind != NodeKind::Folder {
            return;
        }
        out.push((
            self.node.id,
            format!("{}{}", "\u{2003}".repeat(depth), self.node.title),
        ));
        for child in &self.children {
            child.folders(depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Any three distinct ids do.
    const A: BookmarkId = BookmarkId::TOOLBAR;
    const B: BookmarkId = BookmarkId::OTHER;
    const C: BookmarkId = BookmarkId::MOBILE;

    #[test]
    fn dropping_onto_the_previous_bookmark_keeps_the_order() {
        assert_eq!(next_after(&[A, B, C], A, B), Some(C));
        assert_eq!(next_after(&[A, B], A, B), None);
        assert_eq!(next_after(&[A, C, B], A, B), Some(C));
        assert_eq!(next_after(&[A, B, C], C, A), None);
    }
}
