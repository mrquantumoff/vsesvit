//! Buttons of the bookmarks bar. A link opens in the current tab, or in a background tab with
//! Ctrl or the middle button; a folder opens a menu of its children.

use std::rc::Rc;

use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, NodeKind};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::xaml;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BarItem {
    Link {
        title: String,
        url: String,
    },
    Folder {
        title: String,
        children: Vec<BarItem>,
    },
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
                title: if node.title.is_empty() {
                    url.to_string()
                } else {
                    node.title
                },
                url: url.to_string(),
            }),
            NodeKind::Folder => Some(BarItem::Folder {
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

pub(crate) fn button(item: &BarItem, open: &OpenLink) -> Result<UIElement> {
    let (title, glyph, tip) = match item {
        BarItem::Link { title, url } => (title, "&#xE774;", format!("{title}\n{url}")),
        BarItem::Folder { title, .. } => (title, "&#xE8B7;", title.clone()),
    };
    let button: Button = xaml::load(&format!(
        r#"<Button {{ns}} Background="Transparent" BorderThickness="0" Padding="8,2" Height="28" MaxWidth="220"
                   ToolTipService.ToolTip="{tip}" AutomationProperties.Name="{name}">
             <StackPanel Orientation="Horizontal" Spacing="6">
               <FontIcon Glyph="{glyph}" FontSize="12"/>
               <TextBlock Text="{name}" TextTrimming="CharacterEllipsis"/>
             </StackPanel>
           </Button>"#,
        tip = xaml::escape(&tip),
        name = xaml::escape(title),
    ))?;
    match item {
        BarItem::Link { url, .. } => {
            let (click_url, click_open) = (url.clone(), open.clone());
            button
                .cast::<ButtonBase>()?
                .Click(move |_, _| click_open(&click_url, clicked()))?
                .forget();
            let (url, middle_open) = (url.clone(), open.clone());
            let element = button.cast::<UIElement>()?;
            let target = element.clone();
            element
                .PointerReleased(move |_, args| {
                    let Some(args) = args.as_ref() else { return };
                    let middle = args
                        .GetCurrentPoint(&target)
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
        BarItem::Folder { children, .. } => {
            let menu = MenuFlyout::new()?;
            fill_menu(&menu.Items()?, children, open)?;
            button.SetFlyout(&menu.cast::<FlyoutBase>()?)?;
        }
    }
    button.cast()
}

/// A plain click opens in the current tab; Ctrl+click in a background tab.
fn clicked() -> Disposition {
    if unsafe { GetKeyState(VK_CONTROL) } < 0 {
        Disposition::BackgroundTab
    } else {
        Disposition::CurrentTab
    }
}

fn fill_menu(
    items: &windows_collections::IVector<MenuFlyoutItemBase>,
    children: &[BarItem],
    open: &OpenLink,
) -> Result<()> {
    for child in children {
        match child {
            BarItem::Link { title, url } => {
                let entry = MenuFlyoutItem::new()?;
                entry.SetText(title)?;
                let (url, open) = (url.clone(), open.clone());
                entry.Click(move |_, _| open(&url, clicked()))?.forget();
                items.Append(&entry.cast::<MenuFlyoutItemBase>()?)?;
            }
            BarItem::Folder { title, children } => {
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
                    title: "https://a.test/".into(),
                    url: "https://a.test/".into()
                },
                BarItem::Folder {
                    title: "F".into(),
                    children: vec![BarItem::Link {
                        title: "B".into(),
                        url: "https://b.test/".into()
                    }],
                },
            ]
        );
    }
}
