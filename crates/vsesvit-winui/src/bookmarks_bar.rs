//! The bookmarks bar: a horizontal list whose items can be dragged to reorder them. A link
//! opens in the current tab, or in a background tab with Ctrl or the middle button; a folder
//! opens a menu of its children. Links show their saved favicon.

use std::cell::RefCell;
use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, NodeKind};
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::xaml;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BarItem {
    Link {
        id: BookmarkId,
        title: String,
        url: String,
        /// The saved favicon (PNG), for items on the bar itself.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    CurrentTab,
    BackgroundTab,
}

pub(crate) type OpenLink = Rc<dyn Fn(&str, Disposition)>;

const VK_CONTROL: i32 = 0x11;

/// The bar's list and the item each of its entries shows.
pub(crate) struct Bar {
    list: ListView,
    rows: RefCell<Vec<(IInspectable, BarItem)>>,
}

impl Bar {
    pub fn new(list: ListView) -> Self {
        Self {
            list,
            rows: RefCell::new(Vec::new()),
        }
    }

    pub fn list(&self) -> &ListView {
        &self.list
    }

    pub fn items(&self) -> Vec<BarItem> {
        self.rows
            .borrow()
            .iter()
            .map(|(_, item)| item.clone())
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
                    rows.push((element, item.clone()));
                }
                Err(e) => log::warn!("bookmarks bar item: {e}"),
            }
        }
        *self.rows.borrow_mut() = rows;
    }

    /// The user clicked `clicked` (an entry or its content): open the link or the folder's menu.
    pub fn clicked(&self, clicked: &IInspectable, open: &OpenLink) {
        let row = self
            .rows
            .borrow()
            .iter()
            .find(|(element, _)| {
                xaml::same_object(element, clicked)
                    || element
                        .cast::<ContentControl>()
                        .and_then(|c| c.Content())
                        .is_ok_and(|content| xaml::same_object(&content, clicked))
            })
            .cloned();
        let Some((element, item)) = row else {
            return;
        };
        match item {
            BarItem::Link { url, .. } => open(&url, disposition()),
            BarItem::Folder { children, .. } => {
                if let Err(e) = show_menu(&element, &children, open) {
                    log::warn!("bookmarks bar folder menu: {e}");
                }
            }
        }
    }

    /// After a drag within the bar: the bookmark that moved and the bookmark it now sits
    /// before (`None` at the end).
    pub fn dropped(&self) -> Option<(BookmarkId, Option<BookmarkId>)> {
        let entries = self.entries().ok()?;
        let rows = self.rows.borrow();
        let before: Vec<BookmarkId> = rows.iter().map(|(_, item)| item.id()).collect();
        let after: Vec<BookmarkId> = (&entries)
            .into_iter()
            .filter_map(|element| {
                rows.iter()
                    .find(|(e, _)| xaml::same_object(e, &element))
                    .map(|(_, item)| item.id())
            })
            .collect();
        moved(&before, &after)
    }
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

fn entry(item: &BarItem, open: &OpenLink) -> Result<IInspectable> {
    let (title, glyph, tip) = match item {
        BarItem::Link { title, url, .. } => (title, "&#xE774;", format!("{title}\n{url}")),
        BarItem::Folder { title, .. } => (title, "&#xE8B7;", title.clone()),
    };
    let element: ListViewItem = xaml::load(&format!(
        r#"<ListViewItem {{ns}} ToolTipService.ToolTip="{tip}" AutomationProperties.Name="{name}">
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

fn show_menu(anchor: &IInspectable, children: &[BarItem], open: &OpenLink) -> Result<()> {
    let menu = xaml::acrylic_menu()?;
    fill_menu(&menu.Items()?, children, open)?;
    menu.cast::<FlyoutBase>()?
        .ShowAt(&anchor.cast::<FrameworkElement>()?)
}

fn fill_menu(
    items: &windows_collections::IVector<MenuFlyoutItemBase>,
    children: &[BarItem],
    open: &OpenLink,
) -> Result<()> {
    for child in children {
        match child {
            BarItem::Link { title, url, .. } => {
                let entry = MenuFlyoutItem::new()?;
                entry.SetText(title)?;
                let (url, open) = (url.clone(), open.clone());
                entry.Click(move |_, _| open(&url, disposition()))?.forget();
                items.Append(&entry.cast::<MenuFlyoutItemBase>()?)?;
            }
            BarItem::Folder {
                title, children, ..
            } => {
                let folder = MenuFlyoutSubItem::new()?;
                folder.SetText(title)?;
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
