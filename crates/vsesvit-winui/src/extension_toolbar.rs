//! The toolbar's extension actions, as Chrome has them: the pinned actions as buttons in the
//! order of core's synced `extensions::toolbar` preference, which drag to reorder, and the
//! puzzle-piece Extensions button, whose menu lists every action with a pin toggle and opens
//! the Extensions dialog. A right click on a pinned button offers Unpin.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use vsesvit_core::extensions::toolbar::Layout;
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::bookmarks_bar::moved;
use crate::popup::{self, ExtensionAction};
use crate::xaml;

/// What the toolbar asks its window to do. Actions are named by their core extension id.
#[derive(Clone)]
pub(crate) enum Command {
    /// Opens the action's popup under `anchor`.
    Open {
        id: String,
        anchor: FrameworkElement,
    },
    Pin(String, bool),
    /// Moves a pinned action to this place among the pinned ones.
    Move(String, usize),
    Manage,
}

pub(crate) type Host = Rc<dyn Fn(Command)>;

const PIN_GLYPH: &str = "\u{E718}";
const PINNED_GLYPH: &str = "\u{E842}";

pub(crate) struct Toolbar {
    list: ListView,
    puzzle: Button,
    host: Host,
    /// The pinned buttons and the action each shows.
    rows: RefCell<Vec<(ListViewItem, ExtensionAction)>>,
    /// Every action, in install order, and whether it is pinned.
    actions: RefCell<Vec<(ExtensionAction, bool)>>,
    /// Unpin and Manage extensions, for the pinned button it opens on.
    item_menu: MenuFlyout,
}

impl Toolbar {
    pub fn new(list: ListView, puzzle: Button, host: Host) -> Result<Rc<Self>> {
        let toolbar = Rc::new(Self {
            list,
            puzzle,
            host,
            rows: RefCell::new(Vec::new()),
            actions: RefCell::new(Vec::new()),
            item_menu: xaml::context_menu()?,
        });
        let me = Rc::downgrade(&toolbar);
        let flyout = toolbar.item_menu.cast::<FlyoutBase>()?;
        let opening = flyout.clone();
        flyout
            .Opening(move |_, _| {
                if let Some(toolbar) = me.upgrade()
                    && let Err(e) = toolbar.context_opening(&opening)
                {
                    log::warn!("extension button menu: {e}");
                }
            })?
            .forget();
        let list = toolbar.list.cast::<ListViewBase>()?;
        let me = Rc::downgrade(&toolbar);
        list.ItemClick(move |_, args| {
            let clicked = args.as_ref().and_then(|a| a.ClickedItem().ok());
            if let (Some(toolbar), Some(clicked)) = (me.upgrade(), clicked) {
                toolbar.clicked(&clicked);
            }
        })?
        .forget();
        let me = Rc::downgrade(&toolbar);
        list.DragItemsCompleted(move |_, _| {
            if let Some(toolbar) = me.upgrade() {
                toolbar.dropped();
            }
        })?
        .forget();
        let me = Rc::downgrade(&toolbar);
        toolbar
            .puzzle
            .cast::<ButtonBase>()?
            .Click(move |_, _| {
                if let Some(toolbar) = me.upgrade()
                    && let Err(e) = toolbar.show_menu()
                {
                    log::warn!("extensions menu: {e}");
                }
            })?
            .forget();
        Ok(toolbar)
    }

    /// Shows `layout`'s pinned actions in its order; the puzzle button shows while any
    /// extension has an action.
    pub fn set(&self, actions: &[ExtensionAction], layout: &Layout) {
        let pinned: Vec<&ExtensionAction> = layout
            .pinned
            .iter()
            .filter_map(|id| actions.iter().find(|a| a.id == *id))
            .collect();
        *self.actions.borrow_mut() = actions
            .iter()
            .map(|a| (a.clone(), layout.pinned.contains(&a.id)))
            .collect();
        let Ok(entries) = self.entries() else {
            return;
        };
        let _ = entries.Clear();
        let mut rows = Vec::with_capacity(pinned.len());
        for action in pinned {
            match self.button(action) {
                Ok(item) => {
                    let _ = item.cast::<IInspectable>().and_then(|i| entries.Append(&i));
                    rows.push((item, action.clone()));
                }
                Err(e) => log::warn!("extension action {}: {e}", action.extension_id),
            }
        }
        *self.rows.borrow_mut() = rows;
        let _ = xaml::set_visible(&self.puzzle, !actions.is_empty());
    }

    fn entries(&self) -> Result<windows_collections::IVector<IInspectable>> {
        self.list.cast::<ItemsControl>()?.Items()?.cast()
    }

    fn button(&self, action: &ExtensionAction) -> Result<ListViewItem> {
        let title = xaml::escape(&action.title);
        let item: ListViewItem = xaml::load(&format!(
            r#"<ListViewItem {{ns}} MinWidth="0" MinHeight="0" Width="36" Height="32" Padding="0" Margin="0"
                   HorizontalContentAlignment="Center" ToolTipService.ToolTip="{title}"
                   AutomationProperties.Name="{title}">{}</ListViewItem>"#,
            popup::icon_markup(action.icon.as_deref(), 16)
        ))?;
        item.cast::<UIElement>()?
            .SetContextFlyout(&self.item_menu.cast::<FlyoutBase>()?)?;
        Ok(item)
    }

    /// The pinned button of the action the engine knows as `engine_id`, or the puzzle button
    /// for an unpinned one: where its popup opens.
    pub fn anchor_of(&self, engine_id: &str) -> Option<FrameworkElement> {
        let pinned = self
            .rows
            .borrow()
            .iter()
            .find(|(_, a)| a.extension_id == engine_id)
            .and_then(|(item, _)| item.cast().ok());
        pinned.or_else(|| self.puzzle.cast().ok())
    }

    fn clicked(&self, clicked: &IInspectable) {
        let row = self
            .rows
            .borrow()
            .iter()
            .find(|(item, _)| {
                xaml::same_object(item, clicked)
                    || item
                        .cast::<ContentControl>()
                        .and_then(|c| c.Content())
                        .is_ok_and(|content| xaml::same_object(&content, clicked))
            })
            .and_then(|(item, action)| Some((item.cast().ok()?, action.id.clone())));
        if let Some((anchor, id)) = row {
            (self.host)(Command::Open { id, anchor });
        }
    }

    /// After a drag within the pinned buttons: moves the action that moved in core.
    pub(crate) fn dropped(&self) {
        let Ok(entries) = self.entries() else { return };
        let rows = self.rows.borrow();
        let before: Vec<&str> = rows.iter().map(|(_, a)| a.id.as_str()).collect();
        let after: Vec<&str> = (&entries)
            .into_iter()
            .filter_map(|element| {
                rows.iter()
                    .find(|(item, _)| xaml::same_object(item, &element))
                    .map(|(_, a)| a.id.as_str())
            })
            .collect();
        let Some((id, _)) = moved(&before, &after) else {
            return;
        };
        let to = after.iter().position(|a| *a == id).unwrap_or(0);
        let command = Command::Move(id.to_owned(), to);
        drop(rows);
        (self.host)(command);
    }

    fn context_opening(&self, flyout: &FlyoutBase) -> Result<()> {
        let target = flyout.Target()?;
        let id = self
            .rows
            .borrow()
            .iter()
            .find(|(item, _)| xaml::same_object(item, &target))
            .map(|(_, a)| a.id.clone());
        let items = self.item_menu.Items()?;
        items.Clear()?;
        let Some(id) = id else { return Ok(()) };
        let entries = [
            ("Unpin", "\u{E77A}", Command::Pin(id, false)),
            ("Manage extensions", "\u{E713}", Command::Manage),
        ];
        for (label, glyph, command) in entries {
            let item = MenuFlyoutItem::new()?;
            item.SetText(label)?;
            let icon = FontIcon::new()?;
            icon.SetGlyph(glyph)?;
            item.SetIcon(&icon.cast::<IconElement>()?)?;
            let host = self.host.clone();
            item.Click(move |_, _| host(command.clone()))?.forget();
            items.Append(&item.cast::<MenuFlyoutItemBase>()?)?;
        }
        Ok(())
    }

    /// Opens the context menu of the pinned button of `id`, as a right click there does.
    pub fn show_button_menu(&self, id: &str) -> Result<MenuFlyout> {
        let item = self
            .rows
            .borrow()
            .iter()
            .find(|(_, a)| a.id == id)
            .map(|(item, _)| item.clone())
            .ok_or_else(|| windows_core::Error::new(E_FAIL, "that action is not pinned"))?;
        let options = FlyoutShowOptions::new()?;
        options.SetPlacement(FlyoutPlacementMode::BottomEdgeAlignedRight)?;
        self.item_menu
            .cast::<FlyoutBase>()?
            .ShowAtWithOptions(&item.cast::<FrameworkElement>()?, &options)?;
        Ok(self.item_menu.clone())
    }

    /// The puzzle button's menu: every action with its pin, then Manage extensions.
    pub fn show_menu(&self) -> Result<Flyout> {
        let actions = self.actions.borrow().clone();
        let rows: String = actions
            .iter()
            .enumerate()
            .map(|(i, (action, pinned))| {
                let title = xaml::escape(&action.title);
                let (glyph, tip) = if *pinned {
                    (PINNED_GLYPH, "Unpin from the toolbar")
                } else {
                    (PIN_GLYPH, "Pin to the toolbar")
                };
                format!(
                    r#"<Grid ColumnSpacing="4">
                         <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
                         <Button x:Name="Open{i}" HorizontalAlignment="Stretch" HorizontalContentAlignment="Left"
                                 Background="Transparent" BorderThickness="0" Padding="8,6"
                                 AutomationProperties.Name="{title}">
                           <StackPanel Orientation="Horizontal" Spacing="10">{icon}<TextBlock Text="{title}"
                             TextTrimming="CharacterEllipsis" MaxWidth="200"/></StackPanel>
                         </Button>
                         <Button x:Name="Pin{i}" Grid.Column="1" Width="36" Height="32"
                                 Padding="0" Background="Transparent" BorderThickness="0"
                                 ToolTipService.ToolTip="{tip}" AutomationProperties.Name="{tip}">
                           <FontIcon x:Name="PinGlyph{i}" Glyph="{glyph}" FontSize="14"/>
                         </Button>
                       </Grid>"#,
                    icon = popup::icon_markup(action.icon.as_deref(), 16),
                )
            })
            .collect();
        let content: FrameworkElement = xaml::load(&format!(
            r#"<StackPanel {{ns}} Width="300" Spacing="2">
                 <TextBlock Text="Extensions" Style="{{StaticResource BodyStrongTextBlockStyle}}" Margin="8,0,0,6"/>
                 {rows}
                 <Border Height="1" Margin="0,6" Background="{{ThemeResource DividerStrokeColorDefaultBrush}}"/>
                 <Button x:Name="ManageExtensions" HorizontalAlignment="Stretch" HorizontalContentAlignment="Left"
                         Background="Transparent" BorderThickness="0" Padding="8,6">
                   <StackPanel Orientation="Horizontal" Spacing="10">
                     <FontIcon Glyph="&#xE713;" FontSize="16"/><TextBlock Text="Manage extensions"/>
                   </StackPanel>
                 </Button>
               </StackPanel>"#
        ))?;
        let flyout: Flyout = xaml::load(r#"<Flyout {ns} Placement="BottomEdgeAlignedRight"/>"#)?;
        flyout.SetContent(&content.cast::<UIElement>()?)?;
        let hide = flyout.cast::<FlyoutBase>()?;
        for (i, (action, pinned)) in actions.iter().enumerate() {
            let (host, hide_on_open, id) = (self.host.clone(), hide.clone(), action.id.clone());
            let anchor: FrameworkElement = self.puzzle.cast()?;
            xaml::find::<ButtonBase>(&content, &format!("Open{i}"))?
                .Click(move |_, _| {
                    let _ = hide_on_open.Hide();
                    host(Command::Open {
                        id: id.clone(),
                        anchor: anchor.clone(),
                    });
                })?
                .forget();
            let glyph: FontIcon = xaml::find(&content, &format!("PinGlyph{i}"))?;
            let (host, id, state) = (self.host.clone(), action.id.clone(), Cell::new(*pinned));
            xaml::find::<ButtonBase>(&content, &format!("Pin{i}"))?
                .Click(move |_, _| {
                    let pin = !state.get();
                    state.set(pin);
                    let _ = glyph.SetGlyph(if pin { PINNED_GLYPH } else { PIN_GLYPH });
                    host(Command::Pin(id.clone(), pin));
                })?
                .forget();
        }
        let (host, hide_on_manage) = (self.host.clone(), hide.clone());
        xaml::find::<ButtonBase>(&content, "ManageExtensions")?
            .Click(move |_, _| {
                let _ = hide_on_manage.Hide();
                host(Command::Manage);
            })?
            .forget();
        hide.ShowAt(&self.puzzle.cast::<FrameworkElement>()?)?;
        Ok(flyout)
    }

    /// The core ids of the pinned actions, in toolbar order.
    pub fn pinned(&self) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .map(|(_, a)| a.id.clone())
            .collect()
    }

    pub fn list(&self) -> &ListView {
        &self.list
    }
}
