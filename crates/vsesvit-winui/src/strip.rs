//! The two tab lists of a window: the horizontal `TabView` strip in the title bar and the
//! vertical pane (a reorderable `ListView`) beside the web content. Only the list of the current
//! `tabs.position` holds tabs; switching rebuilds the other one in the same order.
//!
//! The order of tabs and the selection live in the live list's XAML items, which the user
//! reorders by dragging. No `RefCell` borrow is held across a XAML call, because XAML raises
//! `SelectionChanged` synchronously from inside item changes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows_collections::IVector;
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::layout::StripKind;
use crate::tab::TabId;
use crate::tab_header::{TabHeader, TabLook};
use crate::xaml;

/// What a list reports to its window.
pub(crate) struct StripEvents {
    pub selection_changed: Box<dyn Fn(StripKind)>,
    pub close: Box<dyn Fn(TabId)>,
    pub new_tab: Box<dyn Fn()>,
    pub reordered: Box<dyn Fn()>,
    pub toggle_collapsed: Box<dyn Fn()>,
}

pub(crate) type Events = Rc<StripEvents>;

pub(crate) trait TabStrip {
    fn insert(&self, index: u32, tab: TabId, look: &TabLook) -> Result<()>;
    fn remove(&self, tab: TabId) -> Result<()>;
    fn select(&self, tab: TabId) -> Result<()>;
    fn selected(&self) -> Option<TabId>;
    fn selected_index(&self) -> Option<u32>;
    fn select_index(&self, index: u32) -> Result<()>;
    /// Tab ids in display order.
    fn order(&self) -> Vec<TabId>;
    fn update(&self, tab: TabId, look: &TabLook);
    fn clear(&self) -> Result<()>;
}

/// One tab's entry in a list: its XAML item and the header drawn in it.
struct Row {
    tab: TabId,
    item: IInspectable,
    header: TabHeader,
}

/// A list's rows, mapped to and from its XAML items by COM identity.
#[derive(Default)]
struct Rows(RefCell<Vec<Row>>);

impl Rows {
    fn tab_of(&self, item: &IInspectable) -> Option<TabId> {
        self.0
            .borrow()
            .iter()
            .find(|r| xaml::same_object(&r.item, item))
            .map(|r| r.tab)
    }

    fn item_of(&self, tab: TabId) -> Option<IInspectable> {
        self.0
            .borrow()
            .iter()
            .find(|r| r.tab == tab)
            .map(|r| r.item.clone())
    }

    fn update(&self, tab: TabId, look: &TabLook) {
        if let Some(row) = self.0.borrow().iter().find(|r| r.tab == tab) {
            row.header.apply(look);
        }
    }

    fn each_header(&self, f: impl Fn(&TabHeader)) {
        for row in self.0.borrow().iter() {
            f(&row.header);
        }
    }

    fn insert(&self, items: &IVector<IInspectable>, index: u32, row: Row) -> Result<()> {
        let item = row.item.clone();
        self.0.borrow_mut().push(row);
        items.InsertAt(index.min(items.Size()?), &item)
    }

    fn remove(&self, items: &IVector<IInspectable>, tab: TabId) -> Result<()> {
        if let Some(item) = self.item_of(tab) {
            let mut index = 0;
            if items.IndexOf(&item, &mut index)? {
                items.RemoveAt(index)?;
            }
        }
        self.0.borrow_mut().retain(|r| r.tab != tab);
        Ok(())
    }

    fn order(&self, items: &IVector<IInspectable>) -> Vec<TabId> {
        let size = items.Size().unwrap_or(0);
        (0..size)
            .filter_map(|i| items.GetAt(i).ok())
            .filter_map(|item| self.tab_of(&item))
            .collect()
    }

    fn clear(&self, items: &IVector<IInspectable>) -> Result<()> {
        self.0.borrow_mut().clear();
        items.Clear()
    }
}

// ---- the horizontal strip ----

pub(crate) struct TopStrip {
    view: TabView,
    rows: Rows,
}

impl TopStrip {
    pub fn new(view: TabView, events: &Events) -> Result<Rc<Self>> {
        let this = Rc::new(Self {
            view,
            rows: Rows::default(),
        });
        let weak = Rc::downgrade(&this);
        let e = events.clone();
        this.view
            .AddTabButtonClick(move |_, _| (e.new_tab)())?
            .forget();
        let e = events.clone();
        this.view
            .TabCloseRequested(move |_, args| {
                let (Some(this), Some(args)) = (weak.upgrade(), args.as_ref()) else {
                    return;
                };
                let item = args.Tab().and_then(|t| t.cast::<IInspectable>());
                if let Some(id) = item.ok().and_then(|item| this.rows.tab_of(&item)) {
                    (e.close)(id);
                }
            })?
            .forget();
        let e = events.clone();
        this.view
            .SelectionChanged(move |_, _| (e.selection_changed)(StripKind::Top))?
            .forget();
        Ok(this)
    }
}

impl TabStrip for TopStrip {
    fn insert(&self, index: u32, tab: TabId, look: &TabLook) -> Result<()> {
        let header = TabHeader::new(false)?;
        header.apply(look);
        let item = TabViewItem::new()?;
        item.SetHeader(header.root())?;
        let row = Row {
            tab,
            item: item.cast()?,
            header,
        };
        self.rows.insert(&self.view.TabItems()?, index, row)
    }

    fn remove(&self, tab: TabId) -> Result<()> {
        self.rows.remove(&self.view.TabItems()?, tab)
    }

    fn select(&self, tab: TabId) -> Result<()> {
        match self.rows.item_of(tab) {
            Some(item) => self.view.SetSelectedItem(&item),
            None => Ok(()),
        }
    }

    fn selected(&self) -> Option<TabId> {
        self.rows.tab_of(&self.view.SelectedItem().ok()?)
    }

    fn selected_index(&self) -> Option<u32> {
        u32::try_from(self.view.SelectedIndex().ok()?).ok()
    }

    fn select_index(&self, index: u32) -> Result<()> {
        self.view
            .SetSelectedIndex(i32::try_from(index).unwrap_or(i32::MAX))
    }

    fn order(&self) -> Vec<TabId> {
        self.view
            .TabItems()
            .map(|items| self.rows.order(&items))
            .unwrap_or_default()
    }

    fn update(&self, tab: TabId, look: &TabLook) {
        self.rows.update(tab, look);
    }

    fn clear(&self) -> Result<()> {
        self.rows.clear(&self.view.TabItems()?)
    }
}

// ---- the vertical pane ----

const PANE_XAML: &str = r#"
<Grid {ns} Width="240" RowSpacing="2" Padding="4,4,4,4" Background="Transparent">
  <Grid.RowDefinitions>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="*"/>
  </Grid.RowDefinitions>
  <Button x:Name="PaneToggle" Width="36" Height="32" Padding="0" Background="Transparent" BorderThickness="0"
          ToolTipService.ToolTip="Collapse the tab list (Ctrl+S)" AutomationProperties.Name="Collapse the tab list">
    <FontIcon Glyph="&#xE700;" FontSize="16"/>
  </Button>
  <Button x:Name="PaneNewTab" Grid.Row="1" Height="36" Padding="10,0" HorizontalAlignment="Stretch"
          HorizontalContentAlignment="Left" Background="Transparent" BorderThickness="0"
          ToolTipService.ToolTip="New tab (Ctrl+T)" AutomationProperties.Name="New tab">
    <StackPanel Orientation="Horizontal" Spacing="12">
      <FontIcon Glyph="&#xE710;" FontSize="14"/>
      <TextBlock x:Name="PaneNewTabText" Text="New tab"/>
    </StackPanel>
  </Button>
  <ListView x:Name="TabList" Grid.Row="2" SelectionMode="Single" CanReorderItems="True"
            CanDragItems="True" AllowDrop="True" AutomationProperties.Name="Tabs">
    <ListView.ItemContainerStyle>
      <Style TargetType="ListViewItem" BasedOn="{StaticResource DefaultListViewItemStyle}">
        <Setter Property="Padding" Value="10,0,4,0"/>
        <Setter Property="MinHeight" Value="36"/>
        <Setter Property="HorizontalContentAlignment" Value="Stretch"/>
      </Style>
    </ListView.ItemContainerStyle>
  </ListView>
</Grid>"#;

const PANE_WIDTH: f64 = 240.0;
const PANE_COMPACT_WIDTH: f64 = 48.0;

pub(crate) struct SidePane {
    root: FrameworkElement,
    list: ListView,
    toggle: Button,
    new_tab_text: UIElement,
    rows: Rows,
    compact: Cell<bool>,
    events: Events,
}

impl SidePane {
    pub fn new(events: &Events) -> Result<Rc<Self>> {
        let root: FrameworkElement = xaml::load(PANE_XAML)?;
        let this = Rc::new(Self {
            list: xaml::find(&root, "TabList")?,
            toggle: xaml::find(&root, "PaneToggle")?,
            new_tab_text: xaml::find(&root, "PaneNewTabText")?,
            root,
            rows: Rows::default(),
            compact: Cell::new(false),
            events: events.clone(),
        });
        let e = events.clone();
        this.toggle
            .cast::<ButtonBase>()?
            .Click(move |_, _| (e.toggle_collapsed)())?
            .forget();
        let new_tab: ButtonBase = xaml::find(&this.root, "PaneNewTab")?;
        let e = events.clone();
        new_tab.Click(move |_, _| (e.new_tab)())?.forget();
        let e = events.clone();
        this.selector()?
            .SelectionChanged(move |_, _| (e.selection_changed)(StripKind::Side))?
            .forget();
        let e = events.clone();
        this.list
            .cast::<ListViewBase>()?
            .DragItemsCompleted(move |_, _| (e.reordered)())?
            .forget();
        Ok(this)
    }

    pub fn element(&self) -> &FrameworkElement {
        &self.root
    }

    pub fn is_compact(&self) -> bool {
        self.compact.get()
    }

    /// Collapsed, the pane is a column of favicons.
    pub fn set_compact(&self, compact: bool) {
        self.compact.set(compact);
        let width = if compact {
            PANE_COMPACT_WIDTH
        } else {
            PANE_WIDTH
        };
        let _ = self.root.SetWidth(width);
        let _ = xaml::set_visible(&self.new_tab_text, !compact);
        let tip = if compact {
            "Expand the tab list (Ctrl+S)"
        } else {
            "Collapse the tab list (Ctrl+S)"
        };
        let _ = xaml::boxed(tip).and_then(|tip| ToolTipService::SetToolTip(&self.toggle, &tip));
        self.rows.each_header(|header| header.set_compact(compact));
    }

    fn selector(&self) -> Result<Selector> {
        self.list.cast()
    }

    fn items(&self) -> Result<IVector<IInspectable>> {
        self.list.cast::<ItemsControl>()?.Items()?.cast()
    }

    fn wire_row(&self, tab: TabId, header: &TabHeader) -> Result<()> {
        if let Some(close) = header.close_button() {
            let e = self.events.clone();
            close
                .cast::<ButtonBase>()?
                .Click(move |_, _| (e.close)(tab))?
                .forget();
        }
        let root = header.root().cast::<UIElement>()?;
        let target = root.clone();
        let e = self.events.clone();
        root.PointerReleased(move |_, args| {
            let Some(args) = args.as_ref() else { return };
            let middle = args
                .GetCurrentPoint(&target)
                .and_then(|point| point.Properties())
                .and_then(|properties| properties.PointerUpdateKind())
                .is_ok_and(|kind| kind == PointerUpdateKind::MiddleButtonReleased);
            if middle {
                let _ = args.SetHandled(true);
                (e.close)(tab);
            }
        })?
        .forget();
        Ok(())
    }
}

impl TabStrip for SidePane {
    fn insert(&self, index: u32, tab: TabId, look: &TabLook) -> Result<()> {
        let header = TabHeader::new(true)?;
        header.apply(look);
        header.set_compact(self.compact.get());
        self.wire_row(tab, &header)?;
        // The header itself is the item; the list wraps it in a container.
        let row = Row {
            tab,
            item: header.root().cast()?,
            header,
        };
        self.rows.insert(&self.items()?, index, row)
    }

    fn remove(&self, tab: TabId) -> Result<()> {
        self.rows.remove(&self.items()?, tab)
    }

    fn select(&self, tab: TabId) -> Result<()> {
        match self.rows.item_of(tab) {
            Some(item) => self.selector()?.SetSelectedItem(&item),
            None => Ok(()),
        }
    }

    fn selected(&self) -> Option<TabId> {
        self.rows
            .tab_of(&self.selector().ok()?.SelectedItem().ok()?)
    }

    fn selected_index(&self) -> Option<u32> {
        u32::try_from(self.selector().ok()?.SelectedIndex().ok()?).ok()
    }

    fn select_index(&self, index: u32) -> Result<()> {
        self.selector()?
            .SetSelectedIndex(i32::try_from(index).unwrap_or(i32::MAX))
    }

    fn order(&self) -> Vec<TabId> {
        self.items()
            .map(|items| self.rows.order(&items))
            .unwrap_or_default()
    }

    fn update(&self, tab: TabId, look: &TabLook) {
        self.rows.update(tab, look);
    }

    fn clear(&self) -> Result<()> {
        self.rows.clear(&self.items()?)
    }
}
