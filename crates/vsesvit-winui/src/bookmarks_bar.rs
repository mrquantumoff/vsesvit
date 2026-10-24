//! The bookmarks bar: a horizontal list whose items can be dragged to reorder them. A link
//! opens in the current tab, or in a background tab with Ctrl or the middle button; a folder
//! opens a menu of its children. Links show their saved favicon, in menus too.
//!
//! Like Chrome's, the bar shows only the items that fit whole; the others move into the menu of
//! a chevron at its end. `Bar::fit` runs whenever the items or the bar's width change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use vsesvit_core::bookmarks::BookmarkId;
use vsesvit_core::bookmarks::bar::{BarItem, fitting, same_but_icons};
use vsesvit_core::bookmarks::menu::{
    BarCommand, Disposition, MenuEntry, context_entries, link_name, menu_label, tip_lines, untitled,
};
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::{exec, xaml};

/// Runs the bar's commands; the window provides it.
pub(crate) type BarHost = Rc<dyn Fn(BarCommand)>;

const VK_CONTROL: i32 = 0x11;
/// The bar's left and right padding in the window markup.
const BAR_PADDING: f64 = 16.0;
/// The chevron's width with its margin, kept free at the bar's end once items overflow.
const CHEVRON_WIDTH: f64 = 30.0;

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
        // An untitled entry shows only its icon; a link then goes by its URL.
        let (title, name, glyph) = match item {
            BarItem::Link { title, url, .. } => (title, link_name(title, url), "&#xE774;"),
            BarItem::Folder { title, .. } => (title, title.as_str(), "&#xE8B7;"),
        };
        let element: ListViewItem = xaml::load(&format!(
            r#"<ListViewItem {{ns}} MinWidth="0" MinHeight="24" Height="24" Padding="6,0" Margin="0,0,1,0"
                   ToolTipService.ToolTip="{tip}" AutomationProperties.Name="{name}">
                 <StackPanel Orientation="Horizontal" Spacing="5">
                   <Grid Width="14" Height="14">
                     <FontIcon x:Name="Glyph" Glyph="{glyph}" FontSize="11"/>
                     <Image x:Name="Favicon" Width="14" Height="14" Visibility="Collapsed"/>
                   </Grid>
                   <TextBlock Text="{text}" FontSize="12" MaxWidth="140" TextTrimming="CharacterEllipsis"
                              VerticalAlignment="Center" Visibility="{visibility}"/>
                 </StackPanel>
               </ListViewItem>"#,
            tip = tip_markup(item),
            name = xaml::escape(name),
            text = xaml::escape(title),
            visibility = if untitled(title) {
                "Collapsed"
            } else {
                "Visible"
            },
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
                    entry.SetText(&menu_label(link_name(title, url)))?;
                    entry.SetIcon(&link_icon(icon.clone())?)?;
                    xaml::set_tip(&entry, &tip_lines(child).join("\n"))?;
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

/// The tooltip of a bar entry, as escaped attribute text: a link's title over its URL, or just
/// the URL if untitled. The line break is a character reference, as XML reads a literal one in
/// an attribute as a space.
fn tip_markup(item: &BarItem) -> String {
    let lines: Vec<String> = tip_lines(item).into_iter().map(xaml::escape).collect();
    lines.join("&#10;")
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
    use vsesvit_core::bookmarks::BookmarkId;

    use super::*;

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
        assert_eq!(tip_markup(&link("")), "https://blog.rust-lang.org/");
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
