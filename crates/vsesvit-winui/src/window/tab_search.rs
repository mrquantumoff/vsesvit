//! Tab search, Chrome's Ctrl+Shift+A: a box over the open tabs of every window and the recently
//! closed ones, as core lists them for what is typed, under the tab list's search button. The
//! first row is selected; Up and Down move the selection, Enter or a click switches to the tab
//! (bringing its window to the front) or reopens it here, and Escape or a click elsewhere closes
//! the popup.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use vsesvit_core::prefs::TabsPosition;
use vsesvit_core::tab_search::{Hit, Row};
use windows_collections::IVector;
use windows_core::{IInspectable, Interface, Result};

use super::BrowserWindow;
use super::wiring::with;
use crate::bindings::*;
use crate::bookmark_editor::VK_RETURN;
use crate::browser::TabHit;
use crate::layout::StripKind;
use crate::tab::TabId;
use crate::{exec, xaml};

const VK_ESCAPE: i32 = 0x1B;
const VK_UP: i32 = 0x26;
const VK_DOWN: i32 = 0x28;

const FLYOUT: &str = r#"
<Flyout {ns}>
  <Flyout.FlyoutPresenterStyle>
    <Style TargetType="FlyoutPresenter" BasedOn="{StaticResource DefaultFlyoutPresenterStyle}">
      <Setter Property="Padding" Value="8"/>
    </Style>
  </Flyout.FlyoutPresenterStyle>
</Flyout>"#;

/// The popup's content, loaded as its own root so that its names resolve before it is shown.
const CONTENT: &str = r#"
<Grid {ns} Width="400" RowSpacing="8">
  <Grid.RowDefinitions>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
  </Grid.RowDefinitions>
  <TextBox x:Name="TabSearchQuery" PlaceholderText="Search tabs" AutomationProperties.Name="Search tabs"/>
  <ListView x:Name="TabSearchList" Grid.Row="1" MaxHeight="440" SelectionMode="Single"
            IsItemClickEnabled="True" AutomationProperties.Name="Tabs">
    <ListView.ItemContainerStyle>
      <Style TargetType="ListViewItem" BasedOn="{StaticResource DefaultListViewItemStyle}">
        <Setter Property="Padding" Value="12,0"/>
        <Setter Property="MinHeight" Value="0"/>
      </Style>
    </ListView.ItemContainerStyle>
  </ListView>
  <TextBlock x:Name="TabSearchEmpty" Grid.Row="2" Margin="12,4,12,8" Text="No results found"
             Visibility="Collapsed" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
</Grid>"#;

/// A section's heading: an item of the list that cannot be clicked or selected.
fn heading(text: &str) -> Result<IInspectable> {
    xaml::load(&format!(
        r#"<ListViewItem {{ns}} IsHitTestVisible="False" IsTabStop="False" Padding="12,10,12,2">
             <TextBlock x:Name="Heading" Text="{}" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
           </ListViewItem>"#,
        xaml::escape(text)
    ))
}

/// A tab's row: its icon, its title and its site.
fn row(found: &Row<TabId, u64>, favicon: Option<&ImageSource>) -> Result<IInspectable> {
    let row: FrameworkElement = xaml::load(&format!(
        r#"<Grid {{ns}} Padding="0,6" ColumnSpacing="12">
             <Grid.ColumnDefinitions>
               <ColumnDefinition Width="16"/>
               <ColumnDefinition Width="*"/>
             </Grid.ColumnDefinitions>
             <FontIcon x:Name="Glyph" Glyph="&#xE774;" FontSize="14" VerticalAlignment="Center"/>
             <Image x:Name="Favicon" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
             <StackPanel Grid.Column="1">
               <TextBlock x:Name="Title" Text="{}" TextTrimming="CharacterEllipsis"/>
               <TextBlock x:Name="Site" Text="{}" TextTrimming="CharacterEllipsis"
                          Style="{{StaticResource CaptionTextBlockStyle}}"
                          Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
             </StackPanel>
           </Grid>"#,
        xaml::escape(&found.title),
        xaml::escape(&found.site)
    ))?;
    if let Some(favicon) = favicon {
        let image: Image = xaml::find(&row, "Favicon")?;
        image.SetSource(favicon)?;
        xaml::set_visible(&image, true)?;
        xaml::set_visible(&xaml::find::<UIElement>(&row, "Glyph")?, false)?;
    }
    row.cast()
}

/// The row `step` rows away from `from` among the list's items, of which those in `rows` are
/// rows (the rest headings), wrapping around at the ends as Chrome's list does. From no row,
/// Down goes to the first and Up to the last.
fn step_row(rows: &[bool], from: Option<usize>, step: isize) -> Option<usize> {
    let rows: Vec<usize> = (0..rows.len()).filter(|&i| rows[i]).collect();
    let count = rows.len() as isize;
    if count == 0 {
        return None;
    }
    let next = match from.and_then(|from| rows.iter().position(|&r| r == from)) {
        Some(at) => (at as isize + step).rem_euclid(count),
        None if step > 0 => 0,
        None => count - 1,
    };
    Some(rows[next as usize])
}

/// An open tab search popup.
pub(crate) struct TabSearch {
    window: Weak<BrowserWindow>,
    flyout: FlyoutBase,
    query: TextBox,
    list: ListView,
    empty: UIElement,
    /// The text the list was last built for.
    shown: RefCell<Option<String>>,
    /// What each of the list's items stands for, in order: `None` for a heading.
    hits: RefCell<Vec<Option<TabHit>>>,
}

impl TabSearch {
    /// Opens tab search at `anchor`. `focus` puts the keyboard in its box, as the button and
    /// the shortcut do; scripted runs pass false.
    fn open(
        window: &Rc<BrowserWindow>,
        anchor: &FrameworkElement,
        placement: FlyoutPlacementMode,
        focus: bool,
    ) -> Result<Rc<Self>> {
        let flyout: Flyout = xaml::load(FLYOUT)?;
        let content: FrameworkElement = xaml::load(CONTENT)?;
        flyout.SetContent(&content)?;
        let search = Rc::new(Self {
            window: Rc::downgrade(window),
            flyout: flyout.cast()?,
            query: xaml::find(&content, "TabSearchQuery")?,
            list: xaml::find(&content, "TabSearchList")?,
            empty: xaml::find(&content, "TabSearchEmpty")?,
            shown: RefCell::new(None),
            hits: RefCell::default(),
        });
        search.wire(&content)?;
        search.refresh();

        let options = FlyoutShowOptions::new()?;
        options.SetPlacement(placement)?;
        options.SetShowMode(if focus {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        search.flyout.ShowAtWithOptions(anchor, &options)?;
        if focus {
            let query = search.query.clone();
            exec::spawn(async move {
                let _ = query
                    .cast::<UIElement>()
                    .and_then(|q| q.Focus(FocusState::Programmatic));
            });
        }
        Ok(search)
    }

    fn wire(self: &Rc<Self>, content: &FrameworkElement) -> Result<()> {
        let me = Rc::downgrade(self);
        self.query
            .TextChanged(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.refresh();
                }
            })?
            .forget();
        // Before the box sees them: Up and Down would move its caret.
        let me = Rc::downgrade(self);
        content
            .cast::<UIElement>()?
            .PreviewKeyDown(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                if args.Key().is_ok_and(|k| me.key(k.0)) {
                    let _ = args.SetHandled(true);
                }
            })?
            .forget();
        let me = Rc::downgrade(self);
        self.list
            .cast::<ListViewBase>()?
            .ItemClick(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                let index = args.ClickedItem().ok().and_then(|item| {
                    let mut index = 0;
                    me.items().ok()?.IndexOf(&item, &mut index).ok()?.then_some(index)
                });
                if let Some(index) = index {
                    me.activate(index as usize);
                }
            })?
            .forget();
        let (window, me) = (self.window.clone(), Rc::downgrade(self));
        self.flyout
            .Closed(move |_, _| {
                if let (Some(window), Some(me)) = (window.upgrade(), me.upgrade()) {
                    window.tab_search_closed(&me);
                }
            })?
            .forget();
        Ok(())
    }

    fn items(&self) -> Result<IVector<IInspectable>> {
        self.list.cast::<ItemsControl>()?.Items()?.cast()
    }

    /// Lists the tabs for the box's text, unless the list already shows them, with the first row
    /// selected.
    fn refresh(&self) {
        let text = self.query.Text().map(|t| t.to_string()).unwrap_or_default();
        if self.shown.borrow().as_deref() == Some(text.as_str()) {
            return;
        }
        let Some(browser) = self.window.upgrade().and_then(|w| w.browser()) else {
            return;
        };
        let found = browser.search_tabs(&text);
        if let Err(e) = self.fill(&found) {
            log::warn!("tab search: {e}");
        }
        *self.shown.borrow_mut() = Some(text);
    }

    fn fill(&self, found: &[(Row<TabId, u64>, Option<ImageSource>)]) -> Result<()> {
        let items = self.items()?;
        items.Clear()?;
        let mut hits = Vec::new();
        let mut section = None;
        for (found, favicon) in found {
            let title = match found.hit {
                Hit::Open(_) => "Open tabs",
                Hit::Closed(_) => "Recently closed",
            };
            if section != Some(title) {
                items.Append(&heading(title)?)?;
                hits.push(None);
                section = Some(title);
            }
            items.Append(&row(found, favicon.as_ref())?)?;
            hits.push(Some(found.hit));
        }
        let rows: Vec<bool> = hits.iter().map(Option::is_some).collect();
        *self.hits.borrow_mut() = hits;
        xaml::set_visible(&self.list, !found.is_empty())?;
        xaml::set_visible(&self.empty, found.is_empty())?;
        self.select(step_row(&rows, None, 1))
    }

    fn selected(&self) -> Option<usize> {
        let index = self.list.cast::<Selector>().ok()?.SelectedIndex().ok()?;
        usize::try_from(index).ok()
    }

    fn select(&self, index: Option<usize>) -> Result<()> {
        let at = index.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1);
        self.list.cast::<Selector>()?.SetSelectedIndex(at)?;
        if let Some(index) = index {
            let item = self.items()?.GetAt(u32::try_from(index).unwrap_or(u32::MAX))?;
            self.list.cast::<ListViewBase>()?.ScrollIntoView(&item)?;
        }
        Ok(())
    }

    /// A key pressed in the popup: Up and Down move the selection, Enter chooses the selected
    /// row and Escape closes. Returns whether the key was one of them.
    pub fn key(&self, vk: i32) -> bool {
        match vk {
            VK_UP | VK_DOWN => {
                let rows: Vec<bool> = self.hits.borrow().iter().map(Option::is_some).collect();
                let step = if vk == VK_UP { -1 } else { 1 };
                if let Err(e) = self.select(step_row(&rows, self.selected(), step)) {
                    log::warn!("tab search selection: {e}");
                }
            }
            VK_RETURN => {
                if let Some(index) = self.selected() {
                    self.activate(index);
                }
            }
            VK_ESCAPE => self.close(),
            _ => return false,
        }
        true
    }

    fn activate(&self, index: usize) {
        let hit = self.hits.borrow().get(index).copied().flatten();
        if let Some(hit) = hit {
            self.close();
            with(&self.window, |w| w.tab_search_chose(hit));
        }
    }

    pub fn close(&self) {
        let _ = self.flyout.Hide();
    }

    /// Puts `text` in the box and lists the tabs for it, as typing it does; for scripted runs.
    pub fn set_query(&self, text: &str) -> Result<()> {
        self.query.SetText(text)?;
        self.refresh();
        Ok(())
    }

    /// The list as it shows: each heading, each row as `title | site` after `> ` when selected
    /// and `- ` otherwise, or the note that nothing matched; for scripted runs.
    pub fn lines(&self) -> Vec<String> {
        if xaml::is_visible(&self.empty) {
            return vec!["No results found".to_owned()];
        }
        let Ok(items) = self.items() else {
            return Vec::new();
        };
        let selected = self.selected();
        let hits = self.hits.borrow().clone();
        let text = |item: &FrameworkElement, name: &str| {
            item.FindName(name)
                .and_then(|t| t.cast::<TextBlock>()?.Text())
                .map(|t| t.to_string())
                .unwrap_or_default()
        };
        (&items)
            .into_iter()
            .zip(hits)
            .enumerate()
            .filter_map(|(index, (item, hit))| {
                let item: FrameworkElement = item.cast().ok()?;
                Some(match hit {
                    None => text(&item, "Heading"),
                    Some(_) => {
                        let mark = if selected == Some(index) { ">" } else { "-" };
                        format!("{mark} {} | {}", text(&item, "Title"), text(&item, "Site"))
                    }
                })
            })
            .collect()
    }
}

impl BrowserWindow {
    /// Ctrl+Shift+A and the search buttons: opens tab search, or closes it while it is open.
    pub(super) fn toggle_tab_search(&self) {
        if let Some(open) = self.tab_search.take() {
            open.close();
            return;
        }
        // The page has the whole window, and nothing of the tab list is there to open under.
        if self.fullscreen.get() {
            return;
        }
        let anchor: Result<FrameworkElement> = match self.tab_search_button() {
            Some(button) => button.cast(),
            None => self.ui.toolbar.cast(),
        };
        let placement = if self.tabs_position.get() == TabsPosition::Right {
            FlyoutPlacementMode::BottomEdgeAlignedRight
        } else {
            FlyoutPlacementMode::BottomEdgeAlignedLeft
        };
        let opened = anchor.and_then(|anchor| {
            TabSearch::open(&self.me(), &anchor, placement, self.is_foreground())
        });
        match opened {
            Ok(search) => *self.tab_search.borrow_mut() = Some(search),
            Err(e) => log::error!("tab search: {e}"),
        }
    }

    /// Tab search while it is open.
    pub fn tab_search(&self) -> Option<Rc<TabSearch>> {
        self.tab_search.borrow().clone()
    }

    fn tab_search_closed(&self, closed: &Rc<TabSearch>) {
        let mut open = self.tab_search.borrow_mut();
        if open.as_ref().is_some_and(|o| Rc::ptr_eq(o, closed)) {
            *open = None;
        }
    }

    /// The tab list's search button: atop the expanded pane, or after the tabs on top. The
    /// collapsed pane has none.
    pub fn tab_search_button(&self) -> Option<Button> {
        let button = match StripKind::of(self.tabs_position.get()) {
            StripKind::Top => self.ui.tab_search.clone(),
            StripKind::Side => self.side.search_button().clone(),
        };
        (!self.fullscreen.get() && xaml::is_visible(&button)).then_some(button)
    }

    /// A row of tab search was chosen: switches to its open tab, bringing that tab's window to
    /// the front, or reopens the closed tab in this window, as Ctrl+Shift+T does.
    fn tab_search_chose(&self, hit: TabHit) {
        let Some(browser) = self.browser() else {
            return;
        };
        match hit {
            Hit::Open(id) => {
                let Some(window) = browser.windows().into_iter().find(|w| w.tab(id).is_some())
                else {
                    return;
                };
                window.select_tab(id);
                // Scripted runs never activate a window.
                if !std::ptr::eq(window.as_ref(), self) && browser.config().mode.is_interactive()
                {
                    window.activate();
                }
            }
            Hit::Closed(at) => {
                if let Some(closed) = browser.take_closed_at(at) {
                    self.reopen(closed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::step_row;

    #[test]
    fn the_selection_skips_headings_and_wraps_around() {
        let rows = [false, true, true, false, true];
        assert_eq!(step_row(&rows, None, 1), Some(1));
        assert_eq!(step_row(&rows, None, -1), Some(4));
        assert_eq!(step_row(&rows, Some(1), 1), Some(2));
        assert_eq!(step_row(&rows, Some(2), 1), Some(4));
        assert_eq!(step_row(&rows, Some(4), 1), Some(1));
        assert_eq!(step_row(&rows, Some(1), -1), Some(4));
        assert_eq!(step_row(&rows, Some(0), 1), Some(1));
        assert_eq!(step_row(&[false], None, 1), None);
        assert_eq!(step_row(&[], Some(0), -1), None);
    }
}
