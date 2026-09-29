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
use crate::{anim, exec, xaml};

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
    /// Whether the tab's row shows its in-use icon.
    fn capture_shown(&self, tab: TabId) -> bool;
}

/// One tab's entry in a list: its XAML item and the header drawn in it.
struct Row {
    tab: TabId,
    item: IInspectable,
    header: TabHeader,
    pinned: Cell<bool>,
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
            row.pinned.set(look.pinned);
        }
    }

    fn is_pinned(&self, tab: TabId) -> Option<bool> {
        self.0
            .borrow()
            .iter()
            .find(|r| r.tab == tab)
            .map(|r| r.pinned.get())
    }

    fn capture_shown(&self, tab: TabId) -> bool {
        self.0
            .borrow()
            .iter()
            .any(|r| r.tab == tab && r.header.capture_shown())
    }

    fn with_header(&self, tab: TabId, f: impl FnOnce(&TabHeader)) {
        if let Some(row) = self.0.borrow().iter().find(|r| r.tab == tab) {
            f(&row.header);
        }
    }

    fn each_header(&self, f: impl Fn(&TabHeader)) {
        for row in self.0.borrow().iter() {
            f(&row.header);
        }
    }

    /// Puts `row` at `index` among the rows, before any leaving row that follows them.
    fn insert(&self, items: &IVector<IInspectable>, index: u32, row: Row) -> Result<()> {
        let at = self.position(items, index);
        let item = row.item.clone();
        self.0.borrow_mut().push(row);
        items.InsertAt(at, &item)
    }

    /// Where the `index`th row is in `items`, which may still hold rows on their way out.
    fn position(&self, items: &IVector<IInspectable>, index: u32) -> u32 {
        let size = items.Size().unwrap_or(0);
        if size as usize == self.0.borrow().len() {
            return index.min(size);
        }
        let mut rows = 0;
        for at in 0..size {
            if items
                .GetAt(at)
                .ok()
                .and_then(|item| self.tab_of(&item))
                .is_some()
            {
                if rows == index {
                    return at;
                }
                rows += 1;
            }
        }
        size
    }

    /// Forgets the tab's row and hands back its item, still in the list.
    fn take(&self, tab: TabId) -> Option<IInspectable> {
        let item = self.item_of(tab);
        self.0.borrow_mut().retain(|r| r.tab != tab);
        item
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

fn remove_item(items: &IVector<IInspectable>, item: &IInspectable) -> Result<()> {
    let mut index = 0;
    if items.IndexOf(item, &mut index)? {
        items.RemoveAt(index)?;
    }
    Ok(())
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
        let weak = Rc::downgrade(&this);
        this.view
            .cast::<FrameworkElement>()?
            .SizeChanged(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    this.fit_widths();
                }
            })?
            .forget();
        Ok(this)
    }

    /// Pinned tabs show their icon only, as in Chrome; the others share the rest of the strip
    /// equally, within the widths `TabView` gives its tabs. (`TabView`'s own equal widths
    /// would make pinned tabs as wide as the rest.)
    fn fit_widths(&self) {
        let rows = self.rows.0.borrow();
        let pinned = rows.iter().filter(|r| r.pinned.get()).count();
        let others = rows.len() - pinned;
        let strip = self
            .view
            .cast::<FrameworkElement>()
            .and_then(|v| v.ActualWidth())
            .unwrap_or(0.0)
            - STRIP_CHROME;
        let shared = if others == 0 {
            0.0
        } else {
            ((strip - pinned as f64 * PINNED_TAB_WIDTH) / others as f64)
                .clamp(TAB_WIDTHS.0, TAB_WIDTHS.1)
                .floor()
        };
        for row in rows.iter() {
            let width = if row.pinned.get() {
                PINNED_TAB_WIDTH
            } else {
                shared
            };
            if let Ok(item) = row.item.cast::<FrameworkElement>() {
                let _ = item.SetMinWidth(width);
                let _ = item.SetMaxWidth(width);
                let _ = item.SetWidth(width);
            }
        }
    }

    /// Widths of the tabs in display order, for scripted runs.
    pub fn widths(&self) -> Vec<(TabId, f64)> {
        self.order()
            .into_iter()
            .filter_map(|tab| {
                let item = self.rows.item_of(tab)?.cast::<FrameworkElement>().ok()?;
                Some((tab, item.ActualWidth().ok()?))
            })
            .collect()
    }
}

/// A pinned tab in the horizontal strip: its icon and the tab's padding.
const PINNED_TAB_WIDTH: f64 = 44.0;
/// `TabView`'s narrowest and widest tabs.
const TAB_WIDTHS: (f64, f64) = (100.0, 240.0);
/// The strip's width that is not tabs: its header, the new tab button and the footer that drags
/// the window.
const STRIP_CHROME: f64 = 8.0 + 40.0 + 188.0;

impl TabStrip for TopStrip {
    fn insert(&self, index: u32, tab: TabId, look: &TabLook) -> Result<()> {
        let header = TabHeader::new(false)?;
        header.apply(look);
        let item = TabViewItem::new()?;
        item.SetHeader(header.root())?;
        item.SetIsClosable(!look.pinned)?;
        header.set_compact(look.pinned);
        wire_header(&self.events, tab, &header, &item.cast()?)?;
        let row = Row {
            tab,
            item: item.cast()?,
            header,
            pinned: Cell::new(look.pinned),
        };
        self.rows.insert(&self.view.TabItems()?, index, row)?;
        self.fit_widths();
        Ok(())
    }

    fn remove(&self, tab: TabId) -> Result<()> {
        if let Some(item) = self.rows.take(tab) {
            remove_item(&self.view.TabItems()?, &item)?;
        }
        self.fit_widths();
        Ok(())
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
        let was_pinned = self.rows.is_pinned(tab);
        self.rows.update(tab, look);
        if let Some(item) = self.rows.item_of(tab).and_then(|i| i.cast::<TabViewItem>().ok()) {
            let _ = item.SetIsClosable(!look.pinned);
        }
        if was_pinned != Some(look.pinned) {
            self.rows.with_header(tab, |header| header.set_compact(look.pinned));
            self.fit_widths();
        }
    }

    fn clear(&self) -> Result<()> {
        self.rows.clear(&self.view.TabItems()?)
    }

    fn capture_shown(&self, tab: TabId) -> bool {
        self.rows.capture_shown(tab)
    }
}

// ---- the vertical pane ----

const PANE_XAML: &str = r#"
<Grid {ns} x:Name="Pane" Width="240" RowSpacing="2" Padding="4,4,4,4" Background="Transparent">
  <Grid.Resources>{width_tween}</Grid.Resources>
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
    <!-- The default transitions without AddDelete, which would fade a pinned tab out and in
         again for most of a second: rows animate their own coming and going (`Row`). -->
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
        <Setter Property="MinHeight" Value="0"/>
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

/// A tab's row in the pane, around its header: it comes in from the side and folds away when
/// its tab closes (`SidePane::depart`).
fn row_markup() -> String {
    format!(
        r#"<Grid {{ns}} x:Name="Row" MinHeight="36" Background="Transparent">
  {transitions}
  <Grid.Resources>{fold}</Grid.Resources>
</Grid>"#,
        transitions = anim::implicit("Grid", anim::ROW),
        fold = anim::Tween::markup("Fold", "Row", "Height", anim::ROW),
    )
}

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
    width_tween: anim::Tween,
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
        let root: FrameworkElement = xaml::load(&PANE_XAML.replacen(
            "{width_tween}",
            &anim::Tween::markup("PaneWidth", "Pane", "Width", anim::PANE),
            1,
        ))?;
        let this = Rc::new(Self {
            width_tween: anim::Tween::find(&root, "PaneWidth")?,
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
        let weak = Rc::downgrade(self);
        let start = move || {
            let Some(this) = weak.upgrade().filter(|this| !this.compact.get()) else {
                return false;
            };
            let Some(origin) = this.origin_x() else {
                return false;
            };
            let fixed = match this.side.get() {
                PaneSide::Left => origin,
                PaneSide::Right => origin + this.width.get(),
            };
            this.drag.set(Some(fixed));
            true
        };
        let weak = Rc::downgrade(self);
        let moved = move |x: f64| {
            if let Some(this) = weak.upgrade()
                && let Some(fixed) = this.drag.get()
            {
                this.set_width((x - fixed).abs());
            }
        };
        let weak = Rc::downgrade(self);
        let ended = move || {
            if let Some(this) = weak.upgrade()
                && this.drag.take().is_some()
            {
                (this.events.pane_resized)(this.width.get());
            }
        };
        xaml::drag_handle(grip, start, moved, ended)
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
            self.width_tween.stop();
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

    /// Every row in the list with its tab, `None` for a row on its way out; for scripted runs.
    pub fn rows(&self) -> Vec<(Option<TabId>, FrameworkElement)> {
        let Ok(items) = self.items() else {
            return Vec::new();
        };
        (&items)
            .into_iter()
            .filter_map(|item| Some((self.rows.tab_of(&item), item.cast().ok()?)))
            .collect()
    }

    pub fn is_compact(&self) -> bool {
        self.compact.get()
    }

    /// Collapsed, the pane is a column of favicons. Once shown, it narrows and widens in place.
    pub fn set_compact(&self, compact: bool) {
        self.compact.set(compact);
        let width = if compact {
            PANE_COMPACT_WIDTH
        } else {
            self.width.get()
        };
        let shown = self.root.ActualWidth().unwrap_or(0.0);
        let _ = self.root.SetWidth(width);
        if shown > 0.0 {
            let _ = self.width_tween.run(shown, width);
        }
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

    /// A row that is no longer a tab fades and folds away, then leaves the list.
    fn depart(&self, item: IInspectable) -> Result<()> {
        let items = self.items()?;
        let row: FrameworkElement = item.cast()?;
        let height = row.ActualHeight()?;
        if !anim::enabled() || height <= 0.0 {
            return remove_item(&items, &item);
        }
        let element = row.cast::<UIElement>()?;
        element.SetIsHitTestVisible(false)?;
        element.SetOpacity(0.0)?;
        row.SetMinHeight(0.0)?;
        row.SetHeight(0.0)?;
        anim::Tween::find(&row, "Fold")?.run(height, 0.0)?;
        exec::spawn(async move {
            exec::sleep(anim::ROW).await;
            if let Err(e) = remove_item(&items, &item) {
                log::warn!("removing a closed tab's row: {e}");
            }
        });
        Ok(())
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
        let item: FrameworkElement = xaml::load(&row_markup())?;
        item.cast::<Panel>()?
            .Children()?
            .Append(&header.root().cast::<UIElement>()?)?;
        anim::rest_on_load(&item)?;
        anim::prepare_entrance(&item.cast()?, -anim::SLIDE)?;
        let row = Row {
            tab,
            item: item.cast()?,
            header,
            pinned: Cell::new(look.pinned),
        };
        self.rows.insert(&self.items()?, index, row)
    }

    /// The tab is gone from `order` at once; only its row takes a moment to leave.
    fn remove(&self, tab: TabId) -> Result<()> {
        match self.rows.take(tab) {
            Some(item) => self.depart(item),
            None => Ok(()),
        }
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
        let selected = self.selected()?;
        let index = self.order().iter().position(|t| *t == selected)?;
        u32::try_from(index).ok()
    }

    fn select_index(&self, index: u32) -> Result<()> {
        match self.order().get(index as usize) {
            Some(tab) => self.select(*tab),
            None => Ok(()),
        }
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

    fn capture_shown(&self, tab: TabId) -> bool {
        self.rows.capture_shown(tab)
    }
}
