//! Settings' Keyboard shortcuts page: a group per core section, a row per command this shell
//! implements with its shortcuts, then a row per extension action command, and the flyout that
//! captures a new one.
//!
//! Every edit goes through core's `Keymap` (see `Browser::edit_keymap`) and applies at once in
//! every window.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use vsesvit_core::extensions::commands::{ExtensionCommand, ExtensionShortcuts};
use vsesvit_core::shortcuts::{
    Chord, Command as Core, Key, Keymap, NEEDS_MODIFIER_NOTE, NOT_A_KEY_NOTE, Section,
};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::shortcuts::{self, Bindings, Mods, Press};
use crate::window::BrowserWindow;
use crate::{exec, platform, xaml};

pub(super) const PANEL: &str = r#"
    <ScrollViewer x:Name="ShortcutsPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="20" Padding="0,0,0,12">
        <Grid ColumnSpacing="12">
          <Grid.ColumnDefinitions>
            <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/>
          </Grid.ColumnDefinitions>
          <TextBlock TextWrapping="Wrap" VerticalAlignment="Center" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                     Text="Select a command to give it another shortcut."/>
          <Button x:Name="ShortcutsResetAll" Grid.Column="1" Content="Reset all">
            <Button.Flyout>
              <Flyout x:Name="ShortcutsResetAllFlyout" Placement="BottomEdgeAlignedRight">
                <StackPanel Width="300" Spacing="12">
                  <TextBlock Text="Reset all shortcuts?" Style="{StaticResource BodyStrongTextBlockStyle}"/>
                  <TextBlock TextWrapping="Wrap" Text="Every command gets its default shortcuts back."/>
                  <Button x:Name="ShortcutsResetAllConfirm" Content="Reset" Style="{StaticResource AccentButtonStyle}"/>
                </StackPanel>
              </Flyout>
            </Button.Flyout>
          </Button>
        </Grid>
        <StackPanel x:Name="ShortcutsList" Spacing="24"/>
      </StackPanel>
    </ScrollViewer>"#;

const CAPTURE: &str = r#"<Flyout {ns} Placement="Bottom"/>"#;

const CAPTURE_CONTENT: &str = r#"
<StackPanel {ns} x:Name="Capture" Width="340" Spacing="12">
  <TextBlock x:Name="CaptureTitle" TextWrapping="Wrap" Style="{StaticResource BodyStrongTextBlockStyle}"/>
  <Border MinHeight="56" Padding="12" CornerRadius="{ThemeResource ControlCornerRadius}"
          Background="{ThemeResource SubtleFillColorSecondaryBrush}">
    <StackPanel x:Name="CaptureKeys" Orientation="Horizontal" Spacing="4"
                HorizontalAlignment="Center" VerticalAlignment="Center"/>
  </Border>
  <TextBlock x:Name="CaptureNote" TextWrapping="Wrap" Visibility="Collapsed"/>
  <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
             Foreground="{ThemeResource TextFillColorSecondaryBrush}"
             Text="Esc cancels. Backspace removes the shortcut."/>
  <StackPanel Orientation="Horizontal" Spacing="8" HorizontalAlignment="Right">
    <Button x:Name="CaptureSave" Content="Save" Style="{StaticResource AccentButtonStyle}" IsEnabled="False"/>
    <Button x:Name="CaptureCancel" Content="Cancel"/>
  </StackPanel>
</StackPanel>"#;

/// How a keycap names a key: symbol keys by their symbol.
fn cap(key: Key) -> &'static str {
    match key {
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Equal => "=",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::Question => "?",
        Key::Semicolon => ";",
        Key::PageUp => "Page Up",
        Key::PageDown => "Page Down",
        Key::KeypadPlus => "Num +",
        Key::KeypadMinus => "Num -",
        Key::Keypad0 => "Num 0",
        Key::Keypad1 => "Num 1",
        Key::Keypad2 => "Num 2",
        Key::Keypad3 => "Num 3",
        Key::Keypad4 => "Num 4",
        Key::Keypad5 => "Num 5",
        Key::Keypad6 => "Num 6",
        Key::Keypad7 => "Num 7",
        Key::Keypad8 => "Num 8",
        Key::Keypad9 => "Num 9",
        key => key.name(),
    }
}

/// The keys of a chord, one keycap each.
fn caps(chord: Chord) -> Vec<&'static str> {
    let mods = chord.mods();
    [
        (mods.ctrl, "Ctrl"),
        (mods.alt, "Alt"),
        (mods.shift, "Shift"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .chain([cap(chord.key())])
    .collect()
}

/// Keycaps for `chords`, "or" between chords, and "Disabled" for none.
fn chords_markup(chords: &[Chord]) -> String {
    const SECONDARY: &str = r#"Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource TextFillColorSecondaryBrush}" VerticalAlignment="Center""#;
    if chords.is_empty() {
        return format!(r#"<TextBlock Text="Disabled" {SECONDARY}/>"#);
    }
    chords
        .iter()
        .map(|&chord| {
            caps(chord)
                .into_iter()
                .map(|name| {
                    format!(
                        r#"<Border MinWidth="28" Padding="7,2" CornerRadius="4" BorderThickness="1,1,1,2"
                                   BorderBrush="{{ThemeResource ControlStrokeColorDefaultBrush}}"
                                   Background="{{ThemeResource ControlFillColorDefaultBrush}}">
                             <TextBlock Text="{}" FontSize="12" HorizontalAlignment="Center"/>
                           </Border>"#,
                        xaml::escape(name)
                    )
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(&format!(r#"<TextBlock Text="or" Margin="4,0" {SECONDARY}/>"#))
}

fn chords_text(chords: &[Chord]) -> String {
    if chords.is_empty() {
        return "Disabled".to_owned();
    }
    chords
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

/// Chrome takes no extension shortcut without Ctrl or Alt.
const EXTENSION_NEEDS_MODIFIER_NOTE: &str = "Extension shortcuts need Ctrl or Alt.";

/// The `x:Name` of a command's row button, from its stable id.
pub(crate) fn row_name(command: Core) -> String {
    format!("Shortcut_{}", command.id().replace('-', "_"))
}

/// The `x:Name` of an extension command's row button. Only action commands have rows, and
/// their names are XAML names already; a Gecko id (`name@example.org`, `{uuid}`) is not.
pub(crate) fn extension_row_name(command: &ExtensionCommand) -> String {
    let extension: String = command
        .extension
        .as_str()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("ExtensionShortcut_{extension}_{}", command.command.name)
}

/// A row's command, which the capture flyout asks a shortcut for.
#[derive(Clone)]
enum Target {
    Browser(Core),
    Extension(ExtensionCommand),
}

impl Target {
    fn row_name(&self) -> String {
        match self {
            Self::Browser(command) => row_name(*command),
            Self::Extension(command) => extension_row_name(command),
        }
    }

    fn reset_name(&self) -> String {
        match self {
            Self::Browser(command) => format!("ShortcutReset_{}", command.id().replace('-', "_")),
            Self::Extension(command) => format!("Reset{}", extension_row_name(command)),
        }
    }

    fn title(&self) -> String {
        match self {
            Self::Browser(command) => command.title().to_owned(),
            Self::Extension(command) => format!("{}: {}", command.extension_name, command.title()),
        }
    }

    /// What saving `chord` would assign, and a note about it, as `Keymap::offer`.
    fn offer(&self, bindings: &Bindings, chord: Chord) -> (Option<Chord>, Option<String>) {
        let (keymap, extensions) = (bindings.keymap(), bindings.extensions());
        match self {
            Self::Browser(command) => {
                let (chord, note) =
                    keymap.offer(*command, chord, |h| !shortcuts::listed().any(|c| c == h));
                let note = note.or_else(|| extensions.note_for_browser(chord?));
                (chord, note)
            }
            Self::Extension(_) if !(chord.mods().ctrl || chord.mods().alt) => {
                (None, Some(EXTENSION_NEEDS_MODIFIER_NOTE.to_owned()))
            }
            Self::Extension(c) => {
                keymap.offer_extension(extensions, &c.extension, &c.command.name, chord)
            }
        }
    }

    /// Gives the command exactly `chord`; `None` removes its shortcut.
    fn assign(&self, keymap: &mut Keymap, extensions: &ExtensionShortcuts, chord: Option<Chord>) {
        match self {
            Self::Browser(command) => {
                keymap.assign(*command, chord);
            }
            Self::Extension(c) => {
                keymap.assign_extension(extensions, &c.extension, &c.command.name, chord);
            }
        }
    }

    fn reset(&self, keymap: &mut Keymap, extensions: &ExtensionShortcuts) {
        match self {
            Self::Browser(command) => {
                keymap.reset(*command);
            }
            Self::Extension(c) => keymap.reset_extension(extensions, &c.extension, &c.command.name),
        }
    }
}

fn row_markup(target: &Target, chords: &[Chord], is_default: bool) -> String {
    format!(
        r#"<Grid {{ns}} ColumnSpacing="4">
             <Grid.ColumnDefinitions>
               <ColumnDefinition Width="*"/><ColumnDefinition Width="40"/>
             </Grid.ColumnDefinitions>
             <Button x:Name="{row}" HorizontalAlignment="Stretch" HorizontalContentAlignment="Stretch"
                     MinHeight="44" Padding="12,6"
                     Background="{{ThemeResource CardBackgroundFillColorDefaultBrush}}"
                     BorderBrush="{{ThemeResource CardStrokeColorDefaultBrush}}"
                     AutomationProperties.Name="{title}: {text}">
               <Grid ColumnSpacing="12">
                 <Grid.ColumnDefinitions>
                   <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/>
                 </Grid.ColumnDefinitions>
                 <TextBlock Text="{title}" VerticalAlignment="Center" TextTrimming="CharacterEllipsis"/>
                 <StackPanel Grid.Column="1" Orientation="Horizontal" Spacing="4">{chords}</StackPanel>
               </Grid>
             </Button>
             <Button x:Name="{reset}" Grid.Column="1" Width="36" Height="36" Padding="0"
                     Background="Transparent" BorderThickness="0" Visibility="{visibility}"
                     ToolTipService.ToolTip="Reset to the default" AutomationProperties.Name="Reset {title} to the default">
               <FontIcon Glyph="&#xE7A7;" FontSize="14"/>
             </Button>
           </Grid>"#,
        row = target.row_name(),
        reset = target.reset_name(),
        title = xaml::escape(&target.title()),
        text = xaml::escape(&chords_text(chords)),
        chords = chords_markup(chords),
        visibility = if is_default { "Collapsed" } else { "Visible" },
    )
}

/// The page, refilled after every edit.
pub(crate) struct Page {
    scroller: IScrollViewer,
    list: Panel,
    browser: Weak<Browser>,
    /// Its accelerators are off while a shortcut is captured, for when the page shows over it.
    window: Weak<BrowserWindow>,
    /// The window the page shows in, its own or `window`.
    host: HWND,
    capture: Capture,
    me: Weak<Page>,
}

/// The flyout that asks for a new shortcut, and what it holds so far.
struct Capture {
    flyout: FlyoutBase,
    title: TextBlock,
    keys: Panel,
    note: TextBlock,
    save: Control,
    target: RefCell<Option<Target>>,
    chord: Cell<Option<Chord>>,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    host: &Window,
) -> Result<Rc<Page>> {
    let host = platform::window_handle(host)?;
    let flyout: Flyout = xaml::load(CAPTURE)?;
    let content: FrameworkElement = xaml::load(CAPTURE_CONTENT)?;
    content.SetRequestedTheme(super::element_theme(browser.theme()))?;
    flyout.SetContent(&content)?;
    let capture = Capture {
        flyout: flyout.cast()?,
        title: xaml::find(&content, "CaptureTitle")?,
        keys: xaml::find(&content, "CaptureKeys")?,
        note: xaml::find(&content, "CaptureNote")?,
        save: xaml::find(&content, "CaptureSave")?,
        target: RefCell::new(None),
        chord: Cell::new(None),
    };
    let scroller = xaml::find(root, "ShortcutsPanel")?;
    let list = xaml::find(root, "ShortcutsList")?;
    let page = Rc::new_cyclic(|me| Page {
        scroller,
        list,
        browser: Rc::downgrade(browser),
        window: Rc::downgrade(window),
        host,
        capture,
        me: me.clone(),
    });
    page.wire_capture(&content)?;
    let reset_all_flyout: FlyoutBase = xaml::find(root, "ShortcutsResetAllFlyout")?;
    let p = page.me.clone();
    on_click(
        &xaml::find::<Button>(root, "ShortcutsResetAllConfirm")?,
        move || {
            let _ = reset_all_flyout.Hide();
            if let Some(p) = p.upgrade() {
                p.edit(Keymap::reset_all);
            }
        },
    )?;
    page.fill()?;
    Ok(page)
}

impl Page {
    fn fill(&self) -> Result<()> {
        let bindings = shortcuts::current();
        let keymap = bindings.keymap();
        let children = self.list.Children()?;
        children.Clear()?;
        for section in Section::ALL {
            let commands: Vec<Core> = shortcuts::listed()
                .filter(|c| c.section() == section)
                .collect();
            if commands.is_empty() {
                continue;
            }
            let group: Panel = xaml::load(&format!(
                r#"<StackPanel {{ns}} Spacing="4">
                     <TextBlock Text="{}" Margin="0,0,0,4" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
                   </StackPanel>"#,
                xaml::escape(section.title())
            ))?;
            children.Append(&group.cast::<UIElement>()?)?;
            for command in commands {
                self.add_row(
                    &group,
                    Target::Browser(command),
                    keymap.chords(command),
                    keymap.is_default(command),
                )?;
            }
        }
        let extensions = bindings.extensions();
        let actions: Vec<_> = extensions
            .iter()
            .filter(|(c, _)| c.command.activates_action())
            .collect();
        if actions.is_empty() {
            return Ok(());
        }
        let group: Panel = xaml::load(
            r#"<StackPanel {ns} Spacing="4">
                 <TextBlock Text="Extension shortcuts" Margin="0,0,0,4" Style="{StaticResource BodyStrongTextBlockStyle}"/>
               </StackPanel>"#,
        )?;
        children.Append(&group.cast::<UIElement>()?)?;
        for (command, chord) in actions {
            let is_default = extensions.is_default(&command.extension, &command.command.name);
            let chords: Vec<Chord> = chord.into_iter().collect();
            self.add_row(
                &group,
                Target::Extension(command.clone()),
                &chords,
                is_default,
            )?;
        }
        Ok(())
    }

    /// Adds `target`'s row to `group`: a click on it captures a shortcut, its reset button
    /// resets it.
    fn add_row(
        &self,
        group: &Panel,
        target: Target,
        chords: &[Chord],
        is_default: bool,
    ) -> Result<()> {
        let row: FrameworkElement = xaml::load(&row_markup(&target, chords, is_default))?;
        group.Children()?.Append(&row.cast::<UIElement>()?)?;
        let button = xaml::find::<FrameworkElement>(&row, &target.row_name())?;
        let (me, anchor, opened) = (self.me.clone(), button.clone(), target.clone());
        on_click(&button, move || {
            if let Some(me) = me.upgrade() {
                me.open_capture(opened.clone(), &anchor);
            }
        })?;
        let me = self.me.clone();
        on_click(
            &xaml::find::<Button>(&row, &target.reset_name())?,
            move || {
                if let Some(me) = me.upgrade() {
                    let bindings = shortcuts::current();
                    me.edit(|k| target.reset(k, bindings.extensions()));
                }
            },
        )
    }

    /// Edits the keymap, then shows the result, after the click that asked for it has finished
    /// with the row it came from.
    fn edit(&self, edit: impl FnOnce(&mut Keymap)) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        browser.edit_keymap(edit);
        let me = self.me.clone();
        exec::spawn(async move {
            let Some(me) = me.upgrade() else { return };
            // Refilling empties the list for a moment, which scrolls the page to its top.
            let offset = me.scroller.VerticalOffset().unwrap_or(0.0);
            let refilled = me.fill().and_then(|()| {
                me.list.cast::<UIElement>()?.UpdateLayout()?;
                me.scroller
                    .ChangeViewWithOptionalAnimation(None, Some(offset), None, true)?;
                Ok(())
            });
            if let Err(e) = refilled {
                log::warn!("keyboard shortcuts page: {e}");
            }
        });
    }

    fn wire_capture(&self, content: &FrameworkElement) -> Result<()> {
        let me = self.me.clone();
        content
            .cast::<UIElement>()?
            .PreviewKeyDown(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                let _ = args.SetHandled(true);
                let vk = args.Key().map_or(0, |k| u16::try_from(k.0).unwrap_or(0));
                me.capture_key(vk, platform::held_modifiers());
            })?
            .forget();
        let me = self.me.clone();
        on_click(&self.capture.save, move || {
            if let Some(me) = me.upgrade() {
                me.capture_key(0x0D, Mods::NONE);
            }
        })?;
        let me = self.me.clone();
        on_click(
            &xaml::find::<Button>(content, "CaptureCancel")?,
            move || {
                if let Some(me) = me.upgrade() {
                    me.capture_key(0x1B, Mods::NONE);
                }
            },
        )?;
        let me = self.me.clone();
        self.capture
            .flyout
            .Closed(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.capture.target.replace(None);
                    if let Some(window) = me.window.upgrade() {
                        window.suspend_shortcuts(false);
                    }
                }
            })?
            .forget();
        Ok(())
    }

    fn open_capture(&self, target: Target, anchor: &FrameworkElement) {
        let capture = &self.capture;
        let _ = capture
            .title
            .SetText(&format!("Press the new shortcut for {}", target.title()));
        capture.target.replace(Some(target));
        capture.chord.set(None);
        self.show_capture(None, None);
        let Some(window) = self.window.upgrade() else {
            return;
        };
        window.suspend_shortcuts(true);
        let shown = FlyoutShowOptions::new().and_then(|options| {
            options.SetShowMode(if unsafe { GetForegroundWindow() } == self.host {
                FlyoutShowMode::Standard
            } else {
                FlyoutShowMode::Transient
            })?;
            capture.flyout.ShowAtWithOptions(anchor, &options)
        });
        if let Err(e) = shown {
            log::warn!("shortcut capture: {e}");
            window.suspend_shortcuts(false);
        }
    }

    /// What the capture flyout shows: the chord pressed, and a note about it.
    fn show_capture(&self, chord: Option<Chord>, note: Option<&str>) {
        let capture = &self.capture;
        capture.chord.set(chord);
        let markup = chord.map_or_else(
            || {
                r#"<TextBlock Text="…" Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>"#
                    .to_owned()
            },
            |c| chords_markup(&[c]),
        );
        let shown = capture.keys.Children().and_then(|children| {
            children.Clear()?;
            let keys: Panel = xaml::load(&format!(
                r#"<StackPanel {{ns}} Orientation="Horizontal" Spacing="4">{markup}</StackPanel>"#
            ))?;
            children.Append(&keys.cast::<UIElement>()?)
        });
        if let Err(e) = shown {
            log::warn!("shortcut capture: {e}");
        }
        let _ = capture.note.SetText(note.unwrap_or_default());
        let _ = xaml::set_visible(&capture.note, note.is_some());
        let _ = capture.save.SetIsEnabled(chord.is_some());
    }

    /// A key pressed while the capture flyout is open, as its key handler and buttons report it.
    pub(crate) fn capture_key(&self, vk: u16, mods: Mods) {
        let Some(target) = self.capture.target.borrow().clone() else {
            return;
        };
        let bindings = shortcuts::current();
        let assign = |chord| {
            self.close_capture();
            self.edit(|k| target.assign(k, bindings.extensions(), chord));
        };
        match shortcuts::press(vk, mods) {
            Press::Cancel => self.close_capture(),
            Press::Remove => assign(None),
            Press::Save => {
                if let Some(chord) = self.capture.chord.get() {
                    assign(Some(chord));
                }
            }
            Press::Modifier => {}
            Press::Chord(chord) => {
                let (chord, note) = target.offer(&bindings, chord);
                self.show_capture(chord, note.as_deref());
            }
            Press::NeedsModifier => self.show_capture(None, Some(NEEDS_MODIFIER_NOTE)),
            Press::NotAKey => self.show_capture(None, Some(NOT_A_KEY_NOTE)),
        }
    }

    fn close_capture(&self) {
        let _ = self.capture.flyout.Hide();
    }

    /// The capture flyout's note, for scripted runs.
    pub(crate) fn capture_note(&self) -> String {
        self.capture.note.Text().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use vsesvit_core::extensions::ExtensionId;
    use vsesvit_core::extensions::manifest::{ACTION_COMMANDS, ManifestCommand};

    use super::*;
    use crate::popup::ExtensionAction;

    #[test]
    fn keycaps_name_each_key() {
        assert_eq!(
            caps("Ctrl+Shift+S".parse().unwrap()),
            ["Ctrl", "Shift", "S"]
        );
        assert_eq!(caps("Ctrl+Plus".parse().unwrap()), ["Ctrl", "+"]);
        assert_eq!(caps("Alt+Left".parse().unwrap()), ["Alt", "Left"]);
        assert_eq!(caps("F9".parse().unwrap()), ["F9"]);
        assert_eq!(chords_text(&[]), "Disabled");
    }

    const PROBE: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn action_command(name: &str, key: &str) -> ExtensionCommand {
        ExtensionCommand {
            extension: ExtensionId::parse(PROBE).unwrap(),
            extension_name: "Probe".into(),
            command: ManifestCommand {
                name: name.into(),
                description: String::new(),
                suggested_key: key.parse().ok(),
            },
        }
    }

    #[test]
    fn row_names_are_xaml_names() {
        let targets = Core::ALL
            .iter()
            .map(|&command| Target::Browser(command))
            .chain(ACTION_COMMANDS.map(|name| Target::Extension(action_command(name, ""))))
            .chain(
                [
                    "{8a5d2c3e-1b4f-4e6a-9c7d-0f1e2d3c4b5a}",
                    "commands@vsesvit.test",
                ]
                .map(|id| {
                    Target::Extension(ExtensionCommand {
                        extension: ExtensionId::parse(id).unwrap(),
                        ..action_command("_execute_action", "")
                    })
                }),
            );
        for target in targets {
            for name in [target.row_name(), target.reset_name()] {
                assert!(
                    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn capture_notes_name_the_other_holder() {
        let command = action_command("_execute_action", "Alt+Shift+P");
        let action = ExtensionAction {
            id: command.extension.as_str().into(),
            extension_id: "engine".into(),
            title: "Probe".into(),
            popup: None,
            icon: None,
        };
        let bindings = Bindings::new(Keymap::default(), vec![command.clone()], &[action]);
        let chord = |s: &str| s.parse::<Chord>().unwrap();
        let extension = Target::Extension(command);
        assert_eq!(
            extension.offer(&bindings, chord("F9")),
            (None, Some(EXTENSION_NEEDS_MODIFIER_NOTE.to_owned()))
        );
        assert_eq!(
            extension.offer(&bindings, chord("Ctrl+Alt+Y")),
            (Some(chord("Ctrl+Alt+Y")), None)
        );
        assert_eq!(
            extension.offer(&bindings, chord("Ctrl+T")).0,
            None,
            "the browser's chords stay the browser's"
        );
        assert_eq!(
            Target::Browser(Core::ShowHistory).offer(&bindings, chord("Alt+Shift+P")),
            (
                Some(chord("Alt+Shift+P")),
                Some("Also used by Probe: Activate the extension. Saving moves it here.".into())
            )
        );
    }
}
