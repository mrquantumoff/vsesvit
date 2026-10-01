//! The bookmarks bar: a horizontal list whose items can be dragged to reorder them. A link
//! opens in the current tab, or in a background tab with Ctrl or the middle button; a folder
//! opens a menu of its children. Links show their saved favicon, in menus too.
//!
//! Like Chrome's, the bar shows only the items that fit whole; the others move into the menu of
//! a chevron at its end. `Bar::fit` runs whenever the items or the bar's width change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, NodeKind};
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::{exec, xaml};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BarItem {
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
    },
}

impl BarItem {
    pub fn id(&self) -> BookmarkId {
        match self {
            BarItem::Link { id, .. } | BarItem::Folder { id, .. } => *id,
        }
    }
}

/// How many folders deep the bookmark views go; a folder deeper still shows as empty. Sync
/// can nest folders without end, and walking thousands of them would overflow the stack.
pub(crate) const MAX_DEPTH: usize = 64;

/// The bar's items for the bookmark folder `folder`, in display order. Separators are dropped.
pub(crate) fn items_from(
    folder: BookmarkId,
    children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>,
) -> Vec<BarItem> {
    items_at(folder, 0, children)
}

/// The items of `folder`, `depth` folders below the bar.
fn items_at(
    folder: BookmarkId,
    depth: usize,
    children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>,
) -> Vec<BarItem> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    children(folder)
        .into_iter()
        .filter_map(|node| match node.kind {
            NodeKind::Url => node.url.map(|url| BarItem::Link {
                id: node.id,
                title: if node.title.is_empty() {
                    url.to_string()
                } else {
                    node.title
                },
                url: url.to_string(),
                icon: None,
            }),
            NodeKind::Folder => Some(BarItem::Folder {
                id: node.id,
                children: items_at(node.id, depth + 1, children),
                title: node.title,
            }),
            NodeKind::Separator => None,
        })
        .collect()
}

/// Gives every link, in folders too, the favicon `favicon` has for its URL.
pub(crate) fn fill_icons(items: &mut [BarItem], favicon: &mut dyn FnMut(&str) -> Option<Vec<u8>>) {
    for item in items {
        match item {
            BarItem::Link { url, icon, .. } => *icon = favicon(url),
            BarItem::Folder { children, .. } => fill_icons(children, favicon),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    CurrentTab,
    BackgroundTab,
    NewWindow,
}

/// What the bar asks its window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BarCommand {
    Open(String, Disposition),
    /// The links directly in a folder, each in a background tab.
    OpenAll(Vec<String>),
    /// The bookmark editor for a bookmark or folder.
    Edit(BookmarkId),
    CopyLink(String),
    /// Deletes a bookmark; a folder with `contents` items in it only once the user confirms.
    Delete {
        id: BookmarkId,
        title: String,
        contents: usize,
    },
    AddPage,
    AddFolder,
    ToggleBar,
    Manager,
}

/// Runs the bar's commands; the window provides it.
pub(crate) type BarHost = Rc<dyn Fn(BarCommand)>;

/// An entry of a bookmark context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuEntry {
    Command(String, BarCommand),
    /// A command with a check mark, checked or not.
    Toggle(String, bool, BarCommand),
    Separator,
}

/// The context menu of a bar item, or of the bar's empty space (`None`), as Chrome has them.
pub(crate) fn context_entries(item: Option<&BarItem>) -> Vec<MenuEntry> {
    use MenuEntry::{Command, Separator, Toggle};
    let command = |label: &str, command| Command(label.to_owned(), command);
    match item {
        Some(BarItem::Link { id, title, url, .. }) => vec![
            command(
                "Open in new tab",
                BarCommand::Open(url.clone(), Disposition::BackgroundTab),
            ),
            command(
                "Open in new window",
                BarCommand::Open(url.clone(), Disposition::NewWindow),
            ),
            Separator,
            command("Edit\u{2026}", BarCommand::Edit(*id)),
            command("Copy link", BarCommand::CopyLink(url.clone())),
            Separator,
            command(
                "Delete",
                BarCommand::Delete {
                    id: *id,
                    title: title.clone(),
                    contents: 0,
                },
            ),
        ],
        Some(BarItem::Folder {
            id,
            title,
            children,
        }) => {
            let links: Vec<String> = children
                .iter()
                .filter_map(|child| match child {
                    BarItem::Link { url, .. } => Some(url.clone()),
                    BarItem::Folder { .. } => None,
                })
                .collect();
            vec![
                Command(
                    format!("Open all ({}) in new tabs", links.len()),
                    BarCommand::OpenAll(links),
                ),
                Separator,
                command("Rename\u{2026}", BarCommand::Edit(*id)),
                Separator,
                command(
                    "Delete",
                    BarCommand::Delete {
                        id: *id,
                        title: title.clone(),
                        contents: children.len(),
                    },
                ),
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

const VK_CONTROL: i32 = 0x11;
/// The bar's left and right padding in the window markup.
const BAR_PADDING: f64 = 16.0;
/// The chevron's width with its margin, kept free at the bar's end once items overflow.
const CHEVRON_WIDTH: f64 = 30.0;
/// Menu labels longer than this are cut, as Chrome caps its menu width.
const MENU_LABEL_CHARS: usize = 50;

/// One entry of the bar and the bookmark it shows.
struct Row {
    element: ListViewItem,
    item: BarItem,
}

/// The bar's list, its chevron and the entries the list holds. Entries past `shown` are
/// collapsed; the chevron's menu lists them instead.
pub(crate) struct Bar {
    root: FrameworkElement,
    list: ListView,
    chevron: Button,
    host: BarHost,
    rows: RefCell<Vec<Row>>,
    shown: Cell<usize>,
    /// The entries of the open bookmark menus and the item each shows.
    menu_rows: RefCell<Vec<(MenuFlyoutItemBase, BarItem)>>,
    /// The context menu of every item, on the bar and in its menus. It fills itself for the
    /// item it opens on.
    item_menu: MenuFlyout,
    /// The context menu of the bar's empty space.
    bar_menu: MenuFlyout,
}

impl Bar {
    pub fn new(
        root: FrameworkElement,
        list: ListView,
        chevron: Button,
        host: BarHost,
    ) -> Result<Rc<Self>> {
        let bar = Rc::new(Self {
            root,
            list,
            chevron,
            host,
            rows: RefCell::new(Vec::new()),
            shown: Cell::new(0),
            menu_rows: RefCell::new(Vec::new()),
            item_menu: xaml::context_menu()?,
            bar_menu: xaml::context_menu()?,
        });
        for menu in [&bar.item_menu, &bar.bar_menu] {
            let me = Rc::downgrade(&bar);
            let flyout = menu.cast::<FlyoutBase>()?;
            let opening = flyout.clone();
            flyout
                .Opening(move |_, _| {
                    if let Some(bar) = me.upgrade() {
                        bar.context_opening(&opening);
                    }
                })?
                .forget();
        }
        bar.root
            .cast::<UIElement>()?
            .SetContextFlyout(&bar.bar_menu.cast::<FlyoutBase>()?)?;
        Ok(bar)
    }

    pub fn list(&self) -> &ListView {
        &self.list
    }

    pub fn items(&self) -> Vec<BarItem> {
        self.rows.borrow().iter().map(|r| r.item.clone()).collect()
    }

    /// The items the bar shows whole, in order; the rest are in the chevron's menu.
    pub fn shown_items(&self) -> Vec<BarItem> {
        let rows = self.rows.borrow();
        rows[..self.shown.get().min(rows.len())]
            .iter()
            .map(|r| r.item.clone())
            .collect()
    }

    pub fn overflow_items(&self) -> Vec<BarItem> {
        let rows = self.rows.borrow();
        rows[self.shown.get().min(rows.len())..]
            .iter()
            .map(|r| r.item.clone())
            .collect()
    }

    pub fn len(&self) -> u32 {
        self.entries().and_then(|e| e.Size()).unwrap_or(0)
    }

    fn entries(&self) -> Result<windows_collections::IVector<IInspectable>> {
        self.list.cast::<ItemsControl>()?.Items()?.cast()
    }

    /// Shows `items`. Entries whose bookmark only got a new favicon stay and show it: pages
    /// that change their icon often would otherwise have the list make, and animate in, every
    /// entry again each time.
    pub fn set(&self, items: &[BarItem]) {
        if self.show_new_icons(items) {
            return;
        }
        let Ok(entries) = self.entries() else {
            return;
        };
        let _ = entries.Clear();
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            match self.entry(item) {
                Ok(element) => {
                    let _ = entries.Append(&element);
                    rows.push(Row {
                        element,
                        item: item.clone(),
                    });
                }
                Err(e) => log::warn!("bookmarks bar item: {e}"),
            }
        }
        *self.rows.borrow_mut() = rows;
        self.fit();
    }

    /// Shows the favicons `items` give the bar's entries, when that is all that changed.
    fn show_new_icons(&self, items: &[BarItem]) -> bool {
        let mut rows = self.rows.borrow_mut();
        if rows.len() != items.len()
            || !rows
                .iter()
                .zip(items)
                .all(|(row, item)| same_but_icons(&row.item, item))
        {
            return false;
        }
        for (row, item) in rows.iter_mut().zip(items) {
            if row.item == *item {
                continue;
            }
            if let BarItem::Link {
                icon: Some(png), ..
            } = item
            {
                let shown = row
                    .element
                    .cast()
                    .and_then(|element| xaml::show_favicon(&element, png.clone()));
                if let Err(e) = shown {
                    log::warn!("bookmarks bar icon: {e}");
                }
            }
            row.item = item.clone();
        }
        true
    }

    /// Shows the items that fit whole in the bar's width and moves the rest behind the chevron.
    pub fn fit(&self) {
        let available = self.root.ActualWidth().unwrap_or(0.0) - BAR_PADDING;
        if available <= 0.0 {
            return;
        }
        let rows = self.rows.borrow();
        let unbounded = Size {
            width: f32::INFINITY,
            height: f32::INFINITY,
        };
        let widths: Vec<f64> = rows
            .iter()
            .map(|row| {
                let element = row.element.cast::<UIElement>().ok();
                element.map_or(0.0, |e| {
                    let _ = xaml::set_visible(&e, true);
                    let _ = e.Measure(unbounded);
                    e.DesiredSize().map_or(0.0, |s| f64::from(s.width))
                })
            })
            .collect();
        let shown = fitting(&widths, available, CHEVRON_WIDTH);
        for row in &rows[shown..] {
            let _ = xaml::set_visible(&row.element, false);
        }
        self.shown.set(shown);
        let _ = xaml::set_visible(&self.chevron, shown < rows.len());
    }

    /// The user clicked `clicked` (an entry or its content): open the link or the folder's menu.
    pub fn clicked(&self, clicked: &IInspectable) {
        let row = self
            .rows
            .borrow()
            .iter()
            .find(|row| {
                xaml::same_object(&row.element, clicked)
                    || row
                        .element
                        .cast::<ContentControl>()
                        .and_then(|c| c.Content())
                        .is_ok_and(|content| xaml::same_object(&content, clicked))
            })
            .map(|row| (row.element.clone(), row.item.clone()));
        let Some((element, item)) = row else {
            return;
        };
        match item {
            BarItem::Link { url, .. } => (self.host)(BarCommand::Open(url, disposition())),
            BarItem::Folder { children, .. } => {
                let shown = element
                    .cast()
                    .and_then(|anchor| self.show_menu(&anchor, &children));
                if let Err(e) = shown {
                    log::warn!("bookmarks bar folder menu: {e}");
                }
            }
        }
    }

    /// The bar entry of `id` while the bar shows it whole.
    pub fn element_of(&self, id: BookmarkId) -> Option<FrameworkElement> {
        let rows = self.rows.borrow();
        rows[..self.shown.get().min(rows.len())]
            .iter()
            .find(|row| row.item.id() == id)
            .and_then(|row| row.element.cast().ok())
    }

    /// The menu of the folder `id` on the bar, as a click opens it.
    pub fn open_folder(&self, id: BookmarkId) -> Result<MenuFlyout> {
        let row = self.rows.borrow().iter().find_map(|row| match &row.item {
            BarItem::Folder {
                id: f, children, ..
            } if *f == id => Some((row.element.clone(), children.clone())),
            _ => None,
        });
        let (element, children) =
            row.ok_or_else(|| windows_core::Error::new(E_FAIL, "no such folder on the bar"))?;
        self.show_menu(&element.cast()?, &children)
    }

    /// The chevron: a menu of the items that do not fit.
    pub fn show_overflow(&self) -> Result<MenuFlyout> {
        self.show_menu(&self.chevron.cast()?, &self.overflow_items())
    }

    /// Opens the context menu of the item `id` (on the bar or in an open menu), or of the bar
    /// itself for `None`, as a right click there does.
    pub fn show_context_menu(&self, id: Option<BookmarkId>) -> Result<MenuFlyout> {
        let options = FlyoutShowOptions::new()?;
        options.SetPlacement(FlyoutPlacementMode::BottomEdgeAlignedLeft)?;
        let Some(id) = id else {
            self.bar_menu
                .cast::<FlyoutBase>()?
                .ShowAtWithOptions(&self.root, &options)?;
            return Ok(self.bar_menu.clone());
        };
        let on_bar = self
            .rows
            .borrow()
            .iter()
            .find(|row| row.item.id() == id)
            .and_then(|row| row.element.cast::<FrameworkElement>().ok());
        let in_menu = || {
            self.menu_rows
                .borrow()
                .iter()
                .find(|(_, item)| item.id() == id)
                .and_then(|(entry, _)| entry.cast::<FrameworkElement>().ok())
        };
        let target = on_bar
            .or_else(in_menu)
            .ok_or_else(|| windows_core::Error::new(E_FAIL, "no such bookmark shown"))?;
        self.item_menu
            .cast::<FlyoutBase>()?
            .ShowAtWithOptions(&target, &options)?;
        Ok(self.item_menu.clone())
    }

    /// A context menu is opening: fill it for the item it opens on.
    fn context_opening(&self, flyout: &FlyoutBase) {
        let target = flyout.Target().ok();
        let item = target.as_ref().and_then(|target| {
            let rows = self.rows.borrow();
            let on_bar = rows
                .iter()
                .find(|row| xaml::same_object(&row.element, target))
                .map(|row| row.item.clone());
            on_bar.or_else(|| {
                self.menu_rows
                    .borrow()
                    .iter()
                    .find(|(entry, _)| xaml::same_object(entry, target))
                    .map(|(_, item)| item.clone())
            })
        });
        let for_bar = xaml::same_object(flyout, &self.bar_menu);
        if !for_bar && item.is_none() {
            return;
        }
        let entries = context_entries(item.as_ref());
        if let Err(e) = flyout
            .cast::<MenuFlyout>()
            .and_then(|menu| self.fill_context(&menu, &entries))
        {
            log::warn!("bookmark context menu: {e}");
        }
    }

    fn fill_context(&self, menu: &MenuFlyout, entries: &[MenuEntry]) -> Result<()> {
        let items = menu.Items()?;
        items.Clear()?;
        for entry in entries {
            let (element, command): (MenuFlyoutItemBase, _) = match entry {
                MenuEntry::Separator => {
                    items.Append(&MenuFlyoutSeparator::new()?.cast::<MenuFlyoutItemBase>()?)?;
                    continue;
                }
                MenuEntry::Command(label, command) => {
                    let item = MenuFlyoutItem::new()?;
                    item.SetText(label)?;
                    if matches!(command, BarCommand::OpenAll(links) if links.is_empty()) {
                        item.cast::<Control>()?.SetIsEnabled(false)?;
                    }
                    (item.cast()?, command.clone())
                }
                MenuEntry::Toggle(label, checked, command) => {
                    let item = ToggleMenuFlyoutItem::new()?;
                    item.cast::<MenuFlyoutItem>()?.SetText(label)?;
                    item.SetIsChecked(*checked)?;
                    (item.cast()?, command.clone())
                }
            };
            let host = self.host.clone();
            element
                .cast::<MenuFlyoutItem>()?
                .Click(move |_, _| host(command.clone()))?
                .forget();
            items.Append(&element)?;
        }
        Ok(())
    }

    /// After a drag within the bar: the bookmark that moved and the bookmark it now sits
    /// before (`None` at the end).
    pub fn dropped(&self) -> Option<(BookmarkId, Option<BookmarkId>)> {
        let entries = self.entries().ok()?;
        let rows = self.rows.borrow();
        let before: Vec<BookmarkId> = rows.iter().map(|row| row.item.id()).collect();
        let after: Vec<BookmarkId> = (&entries)
            .into_iter()
            .filter_map(|element| {
                rows.iter()
                    .find(|row| xaml::same_object(&row.element, &element))
                    .map(|row| row.item.id())
            })
            .collect();
        moved(&before, &after)
    }

    /// An entry of the bar. Its look is set on the entry itself rather than through the list's
    /// item container style, which only reaches entries the list has laid out: `fit` measures
    /// entries it keeps collapsed too.
    fn entry(&self, item: &BarItem) -> Result<ListViewItem> {
        let (title, glyph) = match item {
            BarItem::Link { title, .. } => (title, "&#xE774;"),
            BarItem::Folder { title, .. } => (title, "&#xE8B7;"),
        };
        let element: ListViewItem = xaml::load(&format!(
            r#"<ListViewItem {{ns}} MinWidth="0" MinHeight="24" Height="24" Padding="6,0" Margin="0,0,1,0"
                   ToolTipService.ToolTip="{tip}" AutomationProperties.Name="{name}">
                 <StackPanel Orientation="Horizontal" Spacing="5">
                   <Grid Width="14" Height="14">
                     <FontIcon x:Name="Glyph" Glyph="{glyph}" FontSize="11"/>
                     <Image x:Name="Favicon" Width="14" Height="14" Visibility="Collapsed"/>
                   </Grid>
                   <TextBlock Text="{name}" FontSize="12" MaxWidth="140" TextTrimming="CharacterEllipsis"
                              VerticalAlignment="Center"/>
                 </StackPanel>
               </ListViewItem>"#,
            tip = tip_markup(item),
            name = xaml::escape(title),
        ))?;
        let target = element.cast::<UIElement>()?;
        target.SetContextFlyout(&self.item_menu.cast::<FlyoutBase>()?)?;
        if let BarItem::Link { url, icon, .. } = item {
            if let Some(png) = icon.clone() {
                xaml::show_favicon(&element.cast()?, png)?;
            }
            let (url, host) = (url.clone(), self.host.clone());
            let source = target.clone();
            target
                .PointerReleased(move |_, args| {
                    let Some(args) = args.as_ref() else { return };
                    let middle = args
                        .GetCurrentPoint(&source)
                        .and_then(|point| point.Properties())
                        .and_then(|properties| properties.PointerUpdateKind())
                        .is_ok_and(|kind| kind == PointerUpdateKind::MiddleButtonReleased);
                    if middle {
                        let _ = args.SetHandled(true);
                        host(BarCommand::Open(url.clone(), Disposition::BackgroundTab));
                    }
                })?
                .forget();
        }
        Ok(element)
    }

    fn show_menu(&self, anchor: &FrameworkElement, children: &[BarItem]) -> Result<MenuFlyout> {
        let menu = xaml::acrylic_menu()?;
        self.menu_rows.borrow_mut().clear();
        self.fill_menu(&menu.Items()?, children)?;
        menu.cast::<FlyoutBase>()?.ShowAt(anchor)?;
        Ok(menu)
    }

    fn fill_menu(
        &self,
        items: &windows_collections::IVector<MenuFlyoutItemBase>,
        children: &[BarItem],
    ) -> Result<()> {
        let context = self.item_menu.cast::<FlyoutBase>()?;
        for child in children {
            let entry: MenuFlyoutItemBase = match child {
                BarItem::Link {
                    title, url, icon, ..
                } => {
                    let entry = MenuFlyoutItem::new()?;
                    entry.SetText(&menu_label(title))?;
                    entry.SetIcon(&link_icon(icon.clone())?)?;
                    ToolTipService::SetToolTip(&entry, &xaml::boxed(&format!("{title}\n{url}"))?)?;
                    let (url, host) = (url.clone(), self.host.clone());
                    entry
                        .Click(move |_, _| host(BarCommand::Open(url.clone(), disposition())))?
                        .forget();
                    entry.cast()?
                }
                BarItem::Folder {
                    title, children, ..
                } => {
                    let folder = MenuFlyoutSubItem::new()?;
                    folder.SetText(&menu_label(title))?;
                    folder.SetIcon(&glyph_icon(FOLDER_GLYPH)?)?;
                    self.fill_menu(&folder.Items()?, children)?;
                    folder.cast()?
                }
            };
            entry.cast::<UIElement>()?.SetContextFlyout(&context)?;
            self.menu_rows
                .borrow_mut()
                .push((entry.clone(), child.clone()));
            items.Append(&entry)?;
        }
        if children.is_empty() {
            let empty = MenuFlyoutItem::new()?;
            empty.SetText("(empty)")?;
            empty.cast::<Control>()?.SetIsEnabled(false)?;
            items.Append(&empty.cast::<MenuFlyoutItemBase>()?)?;
        }
        Ok(())
    }
}

/// How many of the items, `widths` wide, fit whole in `available`. When not all of them do,
/// `chevron` is kept free for the button that lists the rest.
pub(crate) fn fitting(widths: &[f64], available: f64, chevron: f64) -> usize {
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
fn same_but_icons(old: &BarItem, new: &BarItem) -> bool {
    match (old, new) {
        (
            BarItem::Link {
                id,
                title,
                url,
                icon,
            },
            BarItem::Link {
                id: new_id,
                title: new_title,
                url: new_url,
                icon: new_icon,
            },
        ) => {
            id == new_id
                && title == new_title
                && url == new_url
                && (new_icon.is_some() || icon.is_none())
        }
        (
            BarItem::Folder {
                id,
                title,
                children,
            },
            BarItem::Folder {
                id: new_id,
                title: new_title,
                children: new_children,
            },
        ) => {
            id == new_id
                && title == new_title
                && children.len() == new_children.len()
                && children
                    .iter()
                    .zip(new_children)
                    .all(|(old, new)| same_but_icons(old, new))
        }
        _ => false,
    }
}

/// The tooltip of a bar entry, as escaped attribute text: a link's title over its URL. The
/// line break is a character reference, as XML reads a literal one in an attribute as a space.
fn tip_markup(item: &BarItem) -> String {
    match item {
        BarItem::Link { title, url, .. } => {
            format!("{}&#10;{}", xaml::escape(title), xaml::escape(url))
        }
        BarItem::Folder { title, .. } => xaml::escape(title),
    }
}

/// A title as a menu shows it: cut to `MENU_LABEL_CHARS` with an ellipsis.
pub(crate) fn menu_label(title: &str) -> String {
    if title.chars().count() <= MENU_LABEL_CHARS {
        return title.to_owned();
    }
    let cut: String = title.chars().take(MENU_LABEL_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The one element that moved between `before` and `after` (same elements), and the element
/// it now precedes. `None` when nothing moved.
pub(crate) fn moved<T: Copy + PartialEq>(before: &[T], after: &[T]) -> Option<(T, Option<T>)> {
    if before.len() != after.len() || before == after {
        return None;
    }
    fn without<T: Copy + PartialEq>(list: &[T], x: T) -> impl Iterator<Item = T> + '_ {
        list.iter().copied().filter(move |&y| y != x)
    }
    let candidate = before
        .iter()
        .zip(after)
        .find(|(a, b)| a != b)
        .into_iter()
        .flat_map(|(&a, &b)| [a, b])
        .find(|&x| without(before, x).eq(without(after, x)))?;
    let at = after.iter().position(|&x| x == candidate)?;
    Some((candidate, after.get(at + 1).copied()))
}

/// Opens in the current tab; with Ctrl held, in a background tab.
fn disposition() -> Disposition {
    if unsafe { GetKeyState(VK_CONTROL) } < 0 {
        Disposition::BackgroundTab
    } else {
        Disposition::CurrentTab
    }
}

const FOLDER_GLYPH: &str = "\u{E8B7}";
const PAGE_GLYPH: &str = "\u{E774}";

fn glyph_icon(glyph: &str) -> Result<IconElement> {
    let icon = FontIcon::new()?;
    icon.SetGlyph(glyph)?;
    icon.cast()
}

/// A link's saved favicon once it has decoded; the page glyph without one.
fn link_icon(png: Option<Vec<u8>>) -> Result<IconElement> {
    let Some(png) = png else {
        return glyph_icon(PAGE_GLYPH);
    };
    let icon = ImageIcon::new()?;
    let target = icon.clone();
    exec::spawn(async move {
        match xaml::png_image(&png).await {
            Ok(source) => {
                let _ = target.SetSource(&source);
            }
            Err(e) => log::debug!("menu favicon: {e}"),
        }
    });
    icon.cast()
}

#[cfg(test)]
mod tests {
    use vsesvit_core::Url;

    use super::*;

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
                node(
                    BookmarkId::MOBILE,
                    NodeKind::Url,
                    "",
                    Some("https://a.test/"),
                ),
                node(BookmarkId::ROOT, NodeKind::Separator, "", None),
                node(BookmarkId::OTHER, NodeKind::Folder, "F", None),
            ],
            f if f == BookmarkId::OTHER => {
                vec![node(
                    BookmarkId::MOBILE,
                    NodeKind::Url,
                    "B",
                    Some("https://b.test/"),
                )]
            }
            _ => vec![],
        };
        assert_eq!(
            items_from(BookmarkId::TOOLBAR, &children),
            [
                BarItem::Link {
                    id: BookmarkId::MOBILE,
                    title: "https://a.test/".into(),
                    url: "https://a.test/".into(),
                    icon: None,
                },
                BarItem::Folder {
                    id: BookmarkId::OTHER,
                    title: "F".into(),
                    children: vec![BarItem::Link {
                        id: BookmarkId::MOBILE,
                        title: "B".into(),
                        url: "https://b.test/".into(),
                        icon: None,
                    }],
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
    fn only_whole_items_fit_and_the_chevron_keeps_its_room() {
        assert_eq!(
            fitting(&[100.0, 100.0, 100.0], 300.0, 30.0),
            3,
            "all fit, no chevron"
        );
        assert_eq!(fitting(&[100.0, 100.0, 100.0], 299.0, 30.0), 2);
        assert_eq!(
            fitting(&[100.0, 100.0, 100.0], 229.0, 30.0),
            1,
            "room for the chevron"
        );
        assert_eq!(fitting(&[100.0, 100.0], 120.0, 30.0), 0);
        assert_eq!(fitting(&[], 10.0, 30.0), 0);
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
    fn icons_reach_links_inside_folders() {
        let link = |url: &str| BarItem::Link {
            id: BookmarkId::MOBILE,
            title: url.into(),
            url: url.into(),
            icon: None,
        };
        let mut items = vec![
            link("https://a.test/"),
            BarItem::Folder {
                id: BookmarkId::OTHER,
                title: "F".into(),
                children: vec![link("https://b.test/")],
            },
        ];
        fill_icons(&mut items, &mut |url| {
            url.contains("b.test").then(|| vec![1])
        });
        assert!(matches!(&items[0], BarItem::Link { icon: None, .. }));
        let BarItem::Folder { children, .. } = &items[1] else {
            panic!("a folder")
        };
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
        };
        assert!(same_but_icons(&link("A", None), &link("A", Some(1))));
        assert!(same_but_icons(&link("A", Some(1)), &link("A", Some(2))));
        assert!(same_but_icons(
            &folder(link("A", None)),
            &folder(link("A", Some(1)))
        ));
        assert!(!same_but_icons(&link("A", Some(1)), &link("A", None)));
        assert!(!same_but_icons(&link("A", None), &link("B", None)));
        assert!(!same_but_icons(
            &folder(link("A", None)),
            &folder(link("B", Some(1)))
        ));
        assert!(!same_but_icons(&link("A", None), &folder(link("A", None))));
    }

    #[test]
    fn context_menus_match_chromes() {
        let link = BarItem::Link {
            id: BookmarkId::MOBILE,
            title: "A".into(),
            url: "https://a.test/".into(),
            icon: None,
        };
        let labels = |item: Option<&BarItem>| -> Vec<String> {
            context_entries(item)
                .into_iter()
                .map(|e| match e {
                    MenuEntry::Command(label, _) | MenuEntry::Toggle(label, _, _) => label,
                    MenuEntry::Separator => "-".into(),
                })
                .collect()
        };
        assert_eq!(
            labels(Some(&link)),
            [
                "Open in new tab",
                "Open in new window",
                "-",
                "Edit\u{2026}",
                "Copy link",
                "-",
                "Delete"
            ]
        );
        let folder = BarItem::Folder {
            id: BookmarkId::OTHER,
            title: "F".into(),
            children: vec![link.clone(), link.clone()],
        };
        assert_eq!(
            labels(Some(&folder)),
            [
                "Open all (2) in new tabs",
                "-",
                "Rename\u{2026}",
                "-",
                "Delete"
            ]
        );
        assert!(context_entries(Some(&folder)).contains(&MenuEntry::Command(
            "Delete".into(),
            BarCommand::Delete {
                id: BookmarkId::OTHER,
                title: "F".into(),
                contents: 2
            }
        )));
        assert_eq!(
            labels(None),
            [
                "Add page\u{2026}",
                "Add folder\u{2026}",
                "-",
                "Show bookmarks bar",
                "Bookmark manager"
            ]
        );
    }

    #[test]
    fn link_tooltip_keeps_line_break_between_title_and_url() {
        let link = |title: &str| BarItem::Link {
            id: BookmarkId::MOBILE,
            title: title.into(),
            url: "https://blog.rust-lang.org/".into(),
            icon: None,
        };
        assert_eq!(
            tip_markup(&link("Rust Blog")),
            "Rust Blog&#10;https://blog.rust-lang.org/"
        );
        assert_eq!(
            tip_markup(&link("a<b")),
            "a&lt;b&#10;https://blog.rust-lang.org/"
        );
    }

    #[test]
    fn a_drag_is_found_in_either_direction() {
        let before = [1, 2, 3, 4, 5];
        assert_eq!(moved(&before, &[1, 3, 4, 2, 5]), Some((2, Some(5))));
        assert_eq!(moved(&before, &[1, 4, 2, 3, 5]), Some((4, Some(2))));
        assert_eq!(moved(&before, &[2, 3, 4, 5, 1]), Some((1, None)));
        assert_eq!(moved(&before, &[5, 1, 2, 3, 4]), Some((5, Some(1))));
        assert_eq!(
            moved(&before, &[2, 1, 3, 4, 5]),
            Some((1, Some(3))),
            "a swap reads as the first one moving"
        );
        assert_eq!(moved(&before, &before), None);
        assert_eq!(moved(&before, &[1, 2, 3]), None);
    }
}
