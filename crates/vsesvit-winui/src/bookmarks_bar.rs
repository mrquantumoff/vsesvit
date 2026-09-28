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

/// The bar's items for the bookmark folder `folder`, in display order. Separators are dropped.
pub(crate) fn items_from(
    folder: BookmarkId,
    children: &dyn Fn(BookmarkId) -> Vec<BookmarkNode>,
) -> Vec<BarItem> {
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
                children: items_from(node.id, children),
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
}

pub(crate) type OpenLink = Rc<dyn Fn(&str, Disposition)>;

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
    rows: RefCell<Vec<Row>>,
    shown: Cell<usize>,
}

impl Bar {
    pub fn new(root: FrameworkElement, list: ListView, chevron: Button) -> Self {
        Self {
            root,
            list,
            chevron,
            rows: RefCell::new(Vec::new()),
            shown: Cell::new(0),
        }
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

    /// Replaces the bar's entries.
    pub fn set(&self, items: &[BarItem], open: &OpenLink) {
        let Ok(entries) = self.entries() else {
            return;
        };
        let _ = entries.Clear();
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            match entry(item, open) {
                Ok(element) => {
                    let _ = entries.Append(&element);
                    if let Ok(element) = element.cast() {
                        rows.push(Row {
                            element,
                            item: item.clone(),
                        });
                    }
                }
                Err(e) => log::warn!("bookmarks bar item: {e}"),
            }
        }
        *self.rows.borrow_mut() = rows;
        self.fit();
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
        log::debug!("bookmarks bar: {available} wide, items {widths:?}, {shown} shown");
        self.shown.set(shown);
        let _ = xaml::set_visible(&self.chevron, shown < rows.len());
    }

    /// The user clicked `clicked` (an entry or its content): open the link or the folder's menu.
    pub fn clicked(&self, clicked: &IInspectable, open: &OpenLink) {
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
            BarItem::Link { url, .. } => open(&url, disposition()),
            BarItem::Folder { children, .. } => {
                let shown = element
                    .cast()
                    .and_then(|anchor| show_menu(&anchor, &children, open));
                if let Err(e) = shown {
                    log::warn!("bookmarks bar folder menu: {e}");
                }
            }
        }
    }

    /// The menu of the folder `id` on the bar, as a click opens it.
    pub fn open_folder(&self, id: BookmarkId, open: &OpenLink) -> Result<MenuFlyout> {
        let row = self.rows.borrow().iter().find_map(|row| match &row.item {
            BarItem::Folder {
                id: f, children, ..
            } if *f == id => Some((row.element.clone(), children.clone())),
            _ => None,
        });
        let (element, children) =
            row.ok_or_else(|| windows_core::Error::new(E_FAIL, "no such folder on the bar"))?;
        show_menu(&element.cast()?, &children, open)
    }

    /// The chevron: a menu of the items that do not fit.
    pub fn show_overflow(&self, open: &OpenLink) -> Result<MenuFlyout> {
        show_menu(&self.chevron.cast()?, &self.overflow_items(), open)
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

/// An entry of the bar. Its look is set on the entry itself rather than through the list's item
/// container style, which only reaches entries the list has laid out: `Bar::fit` measures
/// entries it keeps collapsed too.
fn entry(item: &BarItem, open: &OpenLink) -> Result<IInspectable> {
    let (title, glyph, tip) = match item {
        BarItem::Link { title, url, .. } => (title, "&#xE774;", format!("{title}\n{url}")),
        BarItem::Folder { title, .. } => (title, "&#xE8B7;", title.clone()),
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
        tip = xaml::escape(&tip),
        name = xaml::escape(title),
    ))?;
    if let BarItem::Link { url, icon, .. } = item {
        if let Some(png) = icon.clone() {
            xaml::show_favicon(&element.cast()?, png)?;
        }
        let (url, middle_open) = (url.clone(), open.clone());
        let target = element.cast::<UIElement>()?;
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
                    middle_open(&url, Disposition::BackgroundTab);
                }
            })?
            .forget();
    }
    element.cast()
}

fn show_menu(
    anchor: &FrameworkElement,
    children: &[BarItem],
    open: &OpenLink,
) -> Result<MenuFlyout> {
    let menu = xaml::acrylic_menu()?;
    fill_menu(&menu.Items()?, children, open)?;
    menu.cast::<FlyoutBase>()?.ShowAt(anchor)?;
    Ok(menu)
}

fn fill_menu(
    items: &windows_collections::IVector<MenuFlyoutItemBase>,
    children: &[BarItem],
    open: &OpenLink,
) -> Result<()> {
    for child in children {
        match child {
            BarItem::Link {
                title, url, icon, ..
            } => {
                let entry = MenuFlyoutItem::new()?;
                entry.SetText(&menu_label(title))?;
                entry.SetIcon(&link_icon(icon.clone())?)?;
                ToolTipService::SetToolTip(&entry, &xaml::boxed(&format!("{title}\n{url}"))?)?;
                let (url, open) = (url.clone(), open.clone());
                entry.Click(move |_, _| open(&url, disposition()))?.forget();
                items.Append(&entry.cast::<MenuFlyoutItemBase>()?)?;
            }
            BarItem::Folder {
                title, children, ..
            } => {
                let folder = MenuFlyoutSubItem::new()?;
                folder.SetText(&menu_label(title))?;
                folder.SetIcon(&glyph_icon(FOLDER_GLYPH)?)?;
                fill_menu(&folder.Items()?, children, open)?;
                items.Append(&folder.cast::<MenuFlyoutItemBase>()?)?;
            }
        }
    }
    if children.is_empty() {
        let empty = MenuFlyoutItem::new()?;
        empty.SetText("(empty)")?;
        empty.cast::<Control>()?.SetIsEnabled(false)?;
        items.Append(&empty.cast::<MenuFlyoutItemBase>()?)?;
    }
    Ok(())
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
