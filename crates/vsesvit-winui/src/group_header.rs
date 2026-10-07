//! A tab group's header in a tab list, before the group's first tab: a chip in the group's
//! colour with its title (a dot alone while it has none) that collapses and expands the group
//! when clicked, and the group's editor, a flyout the chip's context menu opens (right-click,
//! Shift+F10 or the Menu key) with the name, Chrome's nine colours, New tab in group, Ungroup
//! and Close group. Each list builds its own header per group.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use vsesvit_core::tab_groups::{GroupColor, GroupId, TabGroup};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::tab_header::hex;
use crate::{exec, xaml};

/// What a header reports about its group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GroupEvent {
    /// The chip was clicked.
    ToggleCollapsed,
    Rename(String),
    Recolor(GroupColor),
    NewTab,
    Ungroup,
    Close,
}

pub(crate) type OnGroup = Rc<dyn Fn(GroupId, GroupEvent)>;

const CHIP_XAML: &str = r#"
<Button {ns} Padding="0" MinWidth="0" MinHeight="0" Background="Transparent" BorderThickness="0"
        VerticalAlignment="Center" HorizontalAlignment="Left">
  <Grid x:Name="ChipBody"/>
</Button>"#;

/// The editor's content, loaded as its own root so that its names resolve before it is shown.
const EDITOR_XAML: &str = r#"
<StackPanel {ns} Width="260" Spacing="8">
  <TextBox x:Name="GroupName" PlaceholderText="Name this group" AutomationProperties.Name="Group name"/>
  <StackPanel x:Name="GroupColors" Orientation="Horizontal" Spacing="4"/>
  <Border Height="1" Margin="0,4" Background="{ThemeResource DividerStrokeColorDefaultBrush}"/>
  <Button x:Name="GroupNewTab" Content="New tab in group" HorizontalAlignment="Stretch"
          HorizontalContentAlignment="Left" Background="Transparent" BorderThickness="0"/>
  <Button x:Name="GroupUngroup" Content="Ungroup" HorizontalAlignment="Stretch"
          HorizontalContentAlignment="Left" Background="Transparent" BorderThickness="0"/>
  <Button x:Name="GroupClose" Content="Close group" HorizontalAlignment="Stretch"
          HorizontalContentAlignment="Left" Background="Transparent" BorderThickness="0"/>
</StackPanel>"#;

/// The chip's text over the group's colour: Chrome's shades are dark in the light theme and
/// pastel in the dark one.
fn text_on(dark: bool) -> &'static str {
    if dark { "#202124" } else { "#FFFFFF" }
}

/// The chip's body: a pill with the title, or a dot while the group has no title.
fn chip_body(title: &str, color: GroupColor, dark: bool) -> Result<UIElement> {
    let fill = hex(color.rgb(dark));
    let markup = if title.is_empty() {
        format!(r#"<Border {{ns}} Width="12" Height="12" CornerRadius="6" Background="{fill}"/>"#)
    } else {
        format!(
            r#"<Border {{ns}} CornerRadius="6" Padding="8,1" Background="{fill}">
                 <TextBlock Text="{}" Foreground="{}" FontSize="12" TextTrimming="CharacterEllipsis" MaxWidth="160"/>
               </Border>"#,
            xaml::escape(title),
            text_on(dark)
        )
    };
    xaml::load(&markup)
}

/// A colour's swatch in the editor; the group's own carries a check mark.
fn swatch(color: GroupColor, chosen: bool, dark: bool) -> Result<Button> {
    let check = if chosen {
        format!(
            r#"<FontIcon Glyph="&#xE73E;" FontSize="12" Foreground="{}"/>"#,
            text_on(dark)
        )
    } else {
        String::new()
    };
    xaml::load(&format!(
        r#"<Button {{ns}} Width="24" Height="24" Padding="0" MinWidth="0" CornerRadius="12" BorderThickness="0"
                   Background="{}" AutomationProperties.Name="{}">{check}</Button>"#,
        hex(color.rgb(dark)),
        color.label()
    ))
}

pub(crate) struct GroupHeader {
    pub group: GroupId,
    chip: Button,
    body: Panel,
    editor: FlyoutBase,
    name: TextBox,
    colors: Panel,
    /// The title, colour, compactness and theme the chip was last drawn for.
    drawn: RefCell<Option<(String, GroupColor, bool, bool)>>,
    /// The group's name as last applied (`TabGroup::name`).
    name_shown: RefCell<String>,
    /// The swatches' colour and theme as last built.
    swatches: Cell<Option<(GroupColor, bool)>>,
    /// The name box is being set from the group, not typed in.
    filling: Cell<bool>,
    /// The collapsed pane shows the dot alone.
    compact: Cell<bool>,
    on: OnGroup,
}

impl GroupHeader {
    pub fn new(group: GroupId, on: OnGroup) -> Result<Rc<Self>> {
        let chip: Button = xaml::load(CHIP_XAML)?;
        let content: FrameworkElement = xaml::load(EDITOR_XAML)?;
        let editor: Flyout = xaml::load(r#"<Flyout {ns} Placement="Bottom"/>"#)?;
        editor.SetContent(&content)?;
        let this = Rc::new(Self {
            group,
            body: xaml::find(&chip.cast()?, "ChipBody")?,
            chip,
            editor: editor.cast()?,
            name: xaml::find(&content, "GroupName")?,
            colors: xaml::find(&content, "GroupColors")?,
            drawn: RefCell::new(None),
            name_shown: RefCell::new(String::new()),
            swatches: Cell::new(None),
            filling: Cell::new(false),
            compact: Cell::new(false),
            on,
        });
        this.wire(&content)?;
        Ok(this)
    }

    fn wire(self: &Rc<Self>, content: &FrameworkElement) -> Result<()> {
        let (on, group) = (self.on.clone(), self.group);
        self.chip
            .cast::<ButtonBase>()?
            .Click(move |_, _| on(group, GroupEvent::ToggleCollapsed))?
            .forget();
        self.chip
            .cast::<UIElement>()?
            .SetContextFlyout(&self.editor)?;
        let me = Rc::downgrade(self);
        self.name
            .TextChanged(move |_, _| {
                if let Some(me) = me.upgrade()
                    && !me.filling.get()
                {
                    let title = me.name.Text().map(|t| t.to_string()).unwrap_or_default();
                    (me.on)(me.group, GroupEvent::Rename(title));
                }
            })?
            .forget();
        for (name, event) in [
            ("GroupNewTab", GroupEvent::NewTab),
            ("GroupUngroup", GroupEvent::Ungroup),
            ("GroupClose", GroupEvent::Close),
        ] {
            let (on, group, editor) = (self.on.clone(), self.group, self.editor.clone());
            xaml::find::<Button>(content, name)?
                .cast::<ButtonBase>()?
                .Click(move |_, _| {
                    let _ = editor.Hide();
                    on(group, event.clone());
                })?
                .forget();
        }
        Ok(())
    }

    /// The chip, which a list wraps in an item of its own.
    pub fn chip(&self) -> &Button {
        &self.chip
    }

    /// The group's name as the header shows it.
    pub fn name(&self) -> String {
        self.name_shown.borrow().clone()
    }

    /// Draws the header for `group` under the light or `dark` theme, and keeps the editor's
    /// name and colours in step with it.
    pub fn apply(&self, group: &TabGroup, dark: bool) -> Result<()> {
        let title = if self.compact.get() {
            ""
        } else {
            group.title.as_str()
        };
        let drawn = (title.to_owned(), group.color, self.compact.get(), dark);
        if self.drawn.borrow().as_ref() != Some(&drawn) {
            let children = self.body.Children()?;
            children.Clear()?;
            children.Append(&chip_body(title, group.color, dark)?)?;
            *self.drawn.borrow_mut() = Some(drawn);
        }
        let toggles = if group.collapsed {
            "expand"
        } else {
            "collapse"
        };
        let tip = format!("{}: click to {toggles}", group.name());
        xaml::set_tip(&self.chip, &tip)?;
        AutomationProperties::SetName(&self.chip, &group.name())?;
        *self.name_shown.borrow_mut() = group.name();
        if self.name.Text().is_ok_and(|t| t != group.title) {
            self.filling.set(true);
            let set = self.name.SetText(&group.title);
            self.filling.set(false);
            set?;
        }
        if self.swatches.get() != Some((group.color, dark)) {
            let children = self.colors.Children()?;
            children.Clear()?;
            for color in GroupColor::ALL {
                let button = swatch(color, color == group.color, dark)?;
                let (on, group) = (self.on.clone(), self.group);
                button
                    .cast::<ButtonBase>()?
                    .Click(move |_, _| on(group, GroupEvent::Recolor(color)))?
                    .forget();
                children.Append(&button.cast::<UIElement>()?)?;
            }
            self.swatches.set(Some((group.color, dark)));
        }
        Ok(())
    }

    /// Dot only: the collapsed vertical pane. The next `apply` redraws.
    pub fn set_compact(&self, compact: bool) {
        self.compact.set(compact);
    }

    /// Opens the editor at the chip, as right-clicking it does. `focus` puts the keyboard in
    /// the name box, for a group just made; scripted runs pass false.
    pub fn open_editor(&self, focus: bool) -> Result<()> {
        let options = FlyoutShowOptions::new()?;
        options.SetShowMode(if focus {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        self.editor
            .ShowAtWithOptions(&self.chip.cast::<FrameworkElement>()?, &options)?;
        if focus {
            let name = self.name.clone();
            exec::spawn(async move {
                let _ = name
                    .cast::<UIElement>()
                    .and_then(|n| n.Focus(FocusState::Programmatic));
                let _ = name.SelectAll();
            });
        }
        Ok(())
    }

    pub fn close_editor(&self) {
        let _ = self.editor.Hide();
    }
}

