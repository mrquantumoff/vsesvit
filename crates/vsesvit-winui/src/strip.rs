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
    /// The tab's speaker was clicked.
    pub toggle_muted: Box<dyn Fn(TabId)>,
    /// The tab's context menu is opening: fill it.
    pub menu: Box<FillMenu>,
    /// The empty space under the vertical pane's tabs changed.
    pub pane_space_changed: Box<dyn Fn()>,
    /// The user dragged the vertical pane to this width.
    pub pane_resized: Box<dyn Fn(f64)>,
}

pub(crate) type FillMenu = dyn Fn(TabId, &MenuFlyout);

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

/// The speaker and the context menu of a tab's header; `menu_owner` is what right-clicks open
/// the menu on.
fn wire_header(events: &Events, tab: TabId, header: &TabHeader, menu_owner: &UIElement) -> Result<()> {
    let e = events.clone();
    header
        .audio_button()
        .cast::<ButtonBase>()?
        .Click(move |_, _| (e.toggle_muted)(tab))?
        .forget();
    let menu = xaml::context_menu()?;
    let flyout = menu.cast::<FlyoutBase>()?;
    let e = events.clone();
    let filled = menu.clone();
    flyout
        .Opening(move |_, _| (e.menu)(tab, &filled))?
        .forget();
    menu_owner.SetContextFlyout(&flyout)
}

// ---- the horizontal strip ----

pub(crate) struct TopStrip {
    view: TabView,
    rows: Rows,
    events: Events,
}

impl TopStrip {
    pub fn new(view: TabView, events: &Events) -> Result<Rc<Self>> {
        let this = Rc::new(Self {
            view,
            rows: Rows::default(),
            events: events.clone(),
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
        item.SetIsClosable(!look.pinned)?;
        wire_header(&self.events, tab, &header, &item.cast()?)?;
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
        if let Some(item) = self.rows.item_of(tab).and_then(|i| i.cast::<TabViewItem>().ok()) {
            let _ = item.SetIsClosable(!look.pinned);
        }
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
    <RowDefinition Height="Auto"/>
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
    <!-- The default transitions without AddDelete: a tab moved by pinning is removed and
         inserted at once, and its row would stay faded out. -->
    <ListView.ItemContainerTransitions>
      <TransitionCollection>
        <ContentThemeTransition/>
        <ReorderThemeTransition/>
        <EntranceThemeTransition IsStaggeringEnabled="False"/>
      </TransitionCollection>
    </ListView.ItemContainerTransitions>
    <ListView.ItemContainerStyle>
      <Style TargetType="ListViewItem" BasedOn="{StaticResource DefaultListViewItemStyle}">
        <Setter Property="Padding" Value="10,0,4,0"/>
        <Setter Property="MinHeight" Value="36"/>
        <Setter Property="HorizontalContentAlignment" Value="Stretch"/>
      </Style>
    </ListView.ItemContainerStyle>
  </ListView>
  <Grid x:Name="MediaHost" Grid.Row="3"/>
  <!-- Drag handles on the pane's edges; the one toward the pages shows. -->
  <Border x:Name="GripRight" Grid.RowSpan="4" Width="6" Margin="0,-4,-4,-4" HorizontalAlignment="Right"
          Background="Transparent" AutomationProperties.Name="Resize the tab list"/>
  <Border x:Name="GripLeft" Grid.RowSpan="4" Width="6" Margin="-4,-4,0,-4" HorizontalAlignment="Left"
          Background="Transparent" Visibility="Collapsed" AutomationProperties.Name="Resize the tab list"/>
</Grid>"#;

/// The widths a dragged pane stays between.
pub(crate) const PANE_WIDTHS: (f64, f64) = (180.0, 480.0);
const PANE_COMPACT_WIDTH: f64 = 48.0;

/// Which side of the pages the pane is on, so which edge drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneSide {
    Left,
    Right,
}

pub(crate) struct SidePane {
    root: FrameworkElement,
    list: ListView,
    toggle: Button,
    new_tab_text: UIElement,
    media_host: Panel,
    grips: [UIElement; 2],
    rows: Rows,
    compact: Cell<bool>,
    width: Cell<f64>,
    side: Cell<PaneSide>,
    /// While dragging: the x of the pane's fixed edge, in window coordinates.
    drag: Cell<Option<f64>>,
    /// The empty space under the tabs as last reported.
    free: Cell<f64>,
    events: Events,
}

impl SidePane {
    pub fn new(events: &Events) -> Result<Rc<Self>> {
        let root: FrameworkElement = xaml::load(PANE_XAML)?;
        let this = Rc::new(Self {
            list: xaml::find(&root, "TabList")?,
            toggle: xaml::find(&root, "PaneToggle")?,
            new_tab_text: xaml::find(&root, "PaneNewTabText")?,
            media_host: xaml::find(&root, "MediaHost")?,
            grips: [xaml::find(&root, "GripRight")?, xaml::find(&root, "GripLeft")?],
            root,
            rows: Rows::default(),
            compact: Cell::new(false),
            width: Cell::new(240.0),
            side: Cell::new(PaneSide::Left),
            drag: Cell::new(None),
            free: Cell::new(0.0),
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
        for grip in &this.grips {
            this.wire_grip(grip)?;
        }
        let weak = Rc::downgrade(&this);
        this.root
            .LayoutUpdated(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    this.measure_free();
                }
            })?
            .forget();
        Ok(this)
    }

    fn wire_grip(self: &Rc<Self>, grip: &UIElement) -> Result<()> {
        let cursor = InputSystemCursor::Create(InputSystemCursorShape::SizeWestEast)?;
        if let Err(e) = grip
            .cast::<IUIElementProtected>()
            .and_then(|g| g.SetProtectedCursor(&cursor.cast::<InputCursor>()?))
        {
            log::debug!("pane grip cursor: {e}");
        }
        let weak = Rc::downgrade(self);
        let target = grip.clone();
        grip.PointerPressed(move |_, args| {
            let (Some(this), Some(args)) = (weak.upgrade(), args.as_ref()) else {
                return;
            };
            if this.compact.get() {
                return;
            }
            let Some(origin) = this.origin_x() else { return };
            let fixed = match this.side.get() {
                PaneSide::Left => origin,
                PaneSide::Right => origin + this.width.get(),
            };
            this.drag.set(Some(fixed));
            let _ = args.Pointer().and_then(|p| target.CapturePointer(&p));
            let _ = args.SetHandled(true);
        })?
        .forget();
        let weak = Rc::downgrade(self);
        grip.PointerMoved(move |_, args| {
            let (Some(this), Some(args)) = (weak.upgrade(), args.as_ref()) else {
                return;
            };
            this.drag_to(args);
        })?
        .forget();
        let weak = Rc::downgrade(self);
        let target = grip.clone();
        grip.PointerReleased(move |_, args| {
            if let (Some(this), Some(args)) = (weak.upgrade(), args.as_ref()) {
                // Moves just before the release may come only with it.
                this.drag_to(args);
                let _ = args.Pointer().and_then(|p| target.ReleasePointerCapture(&p));
                this.end_drag();
            }
        })?
        .forget();
        let weak = Rc::downgrade(self);
        grip.PointerCaptureLost(move |_, _| {
            if let Some(this) = weak.upgrade() {
                this.end_drag();
            }
        })?
        .forget();
        Ok(())
    }

    fn drag_to(&self, args: &PointerRoutedEventArgs) {
        let Some(fixed) = self.drag.get() else { return };
        if let Ok(point) = args
            .GetCurrentPoint(None::<&UIElement>)
            .and_then(|p| p.Position())
        {
            self.set_width((f64::from(point.x) - fixed).abs());
        }
    }

    fn end_drag(&self) {
        if self.drag.take().is_some() {
            (self.events.pane_resized)(self.width.get());
        }
    }

    fn origin_x(&self) -> Option<f64> {
        let origin = self
            .root
            .cast::<UIElement>()
            .and_then(|e| e.TransformToVisual(None::<&UIElement>))
            .and_then(|t| t.TransformPoint(Point { x: 0.0, y: 0.0 }))
            .ok()?;
        Some(f64::from(origin.x))
    }

    /// The expanded pane's width, within [`PANE_WIDTHS`].
    pub fn set_width(&self, width: f64) {
        let width = width.clamp(PANE_WIDTHS.0, PANE_WIDTHS.1).round();
        self.width.set(width);
        if !self.compact.get() {
            let _ = self.root.SetWidth(width);
        }
    }

    pub fn set_side(&self, side: PaneSide) {
        self.side.set(side);
        self.show_grip();
    }

    fn show_grip(&self) {
        let compact = self.compact.get();
        let right = self.side.get() == PaneSide::Left;
        let _ = xaml::set_visible(&self.grips[0], !compact && right);
        let _ = xaml::set_visible(&self.grips[1], !compact && !right);
    }

    /// Puts `element` (the player) under the tab list.
    pub fn set_media(&self, element: &FrameworkElement) -> Result<()> {
        let children = self.media_host.Children()?;
        children.Clear()?;
        children.Append(&element.cast::<UIElement>()?)
    }

    /// The height the tab list does not use, which the player's picture-in-picture may take.
    pub fn free_height(&self) -> f64 {
        self.free.get()
    }

    fn measure_free(&self) {
        let Some(viewer) = self
            .list
            .cast::<DependencyObject>()
            .ok()
            .and_then(|list| xaml::find_descendant::<ScrollViewer>(&list))
        else {
            return;
        };
        // The items panel stretches to the viewport; the tabs' own height is what the scroll
        // viewer measured its content at, without a height limit.
        let content = viewer
            .cast::<ContentControl>()
            .and_then(|v| v.Content())
            .and_then(|c| c.cast::<UIElement>())
            .and_then(|c| c.DesiredSize());
        let free = match (viewer.ViewportHeight(), content) {
            (Ok(viewport), Ok(content)) => viewport - f64::from(content.height),
            _ => return,
        };
        if (free - self.free.get()).abs() > 0.5 {
            self.free.set(free);
            (self.events.pane_space_changed)();
        }
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
            self.width.get()
        };
        let _ = self.root.SetWidth(width);
        self.show_grip();
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
        wire_header(&self.events, tab, header, &root)?;
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
