//! How one tab looks in a tab list: favicon (or a spinner while loading), title, a camera,
//! microphone or screen while the page captures one, a speaker while the page plays sound (it
//! mutes the tab), a pin for pinned tabs, and in the vertical pane a close button; a sleeping
//! tab's icon is faded. Each list builds its own header per tab from a `TabLook`. A tab in a
//! group carries a thin line in the group's colour: along the bottom in the top strip, as
//! Chrome's group line runs, and at the leading edge in the pane.

use std::cell::Cell;

use vsesvit_core::permissions::{Capturing, Permission};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::permissions::glyph;
use crate::shortcuts::{self, Command};
use crate::xaml;

/// Whether a tab makes sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Audio {
    #[default]
    Silent,
    Playing,
    Muted,
}

impl Audio {
    pub fn of(playing: bool, muted: bool) -> Self {
        match (playing, muted) {
            (_, true) => Audio::Muted,
            (true, false) => Audio::Playing,
            (false, false) => Audio::Silent,
        }
    }

    /// The speaker's glyph and what clicking it does.
    fn button(self) -> Option<(&'static str, &'static str)> {
        match self {
            Audio::Silent => None,
            Audio::Playing => Some(("\u{E767}", "Mute tab")),
            Audio::Muted => Some(("\u{E74F}", "Unmute tab")),
        }
    }
}

/// What a tab list shows for a tab.
#[derive(Clone, Default)]
pub(crate) struct TabLook {
    pub title: String,
    pub favicon: Option<ImageSource>,
    pub loading: bool,
    pub audio: Audio,
    pub pinned: bool,
    pub capturing: Capturing,
    /// Memory Saver put it to sleep: its icon shows faded, as in Chrome.
    pub asleep: bool,
}

/// The in-use icon's glyph, and whether it is a recording (camera or microphone, shown red as
/// Chrome's dot is) rather than a screen share.
pub(crate) fn capture_glyph(capturing: Capturing) -> Option<(&'static str, bool)> {
    if capturing.camera {
        Some((glyph(Permission::Camera), true))
    } else if capturing.microphone {
        Some((glyph(Permission::Microphone), true))
    } else if capturing.screen {
        Some((glyph(Permission::ScreenShare), false))
    } else {
        None
    }
}

// No column spacing: the gaps are margins of the parts that collapse, so a row of the collapsed
// pane is exactly as wide as its icon.
const HEADER_XAML: &str = r#"
<Grid {ns} Background="Transparent">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="Auto"/>
    <ColumnDefinition Width="16"/>
    <ColumnDefinition Width="*"/>
    <ColumnDefinition Width="Auto"/>
  </Grid.ColumnDefinitions>
  <Grid x:Name="GroupEdge" Width="3" Margin="0,4,7,4" Visibility="Collapsed"/>
  <Grid x:Name="GroupLine" Grid.ColumnSpan="4" Height="3" Margin="0,0,0,-4" VerticalAlignment="Bottom"
        Visibility="Collapsed"/>
  <FontIcon x:Name="DefaultIcon" Grid.Column="1" Glyph="&#xE774;" FontSize="14" VerticalAlignment="Center"/>
  <Image x:Name="Favicon" Grid.Column="1" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
  <ProgressRing x:Name="Spinner" Grid.Column="1" Width="16" Height="16" MinWidth="16" MinHeight="16"
                VerticalAlignment="Center" IsActive="False" Visibility="Collapsed"/>
  <TextBlock x:Name="Title" Grid.Column="2" Margin="8,0,0,0" Text="New tab"
             TextTrimming="CharacterEllipsis" TextWrapping="NoWrap" VerticalAlignment="Center"/>
  <StackPanel x:Name="Buttons" Grid.Column="3" Orientation="Horizontal" Margin="4,0,0,0">
    <FontIcon x:Name="Pin" Glyph="&#xE718;" FontSize="10" Margin="0,0,6,0" Visibility="Collapsed"
              VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    <Grid x:Name="Capture" Width="20" VerticalAlignment="Center" Visibility="Collapsed" Background="Transparent">
      <FontIcon x:Name="CaptureRecording" FontSize="12" Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
      <FontIcon x:Name="CaptureSharing" FontSize="12" Foreground="{ThemeResource AccentTextFillColorPrimaryBrush}"/>
    </Grid>
    <Button x:Name="Audio" Width="24" Height="24" Padding="0" Background="Transparent"
            BorderThickness="0" VerticalAlignment="Center" Visibility="Collapsed"
            AutomationProperties.Name="Mute or unmute tab">
      <FontIcon x:Name="AudioGlyph" Glyph="&#xE767;" FontSize="12"/>
    </Button>
    <Button x:Name="Close" Width="24" Height="24" Padding="0" Background="Transparent"
            BorderThickness="0" VerticalAlignment="Center" Visibility="Collapsed"
            AutomationProperties.Name="Close tab">
      <FontIcon Glyph="&#xE711;" FontSize="10"/>
    </Button>
  </StackPanel>
</Grid>"#;

pub(crate) struct TabHeader {
    root: FrameworkElement,
    default_icon: UIElement,
    favicon: Image,
    spinner: ProgressRing,
    title: TextBlock,
    buttons: UIElement,
    pin: UIElement,
    capture: FrameworkElement,
    capture_recording: FontIcon,
    capture_sharing: FontIcon,
    audio: Button,
    audio_glyph: FontIcon,
    close: Option<Button>,
    /// The group line: at the leading edge in the pane, along the bottom in the top strip.
    group_line: Panel,
    group_color: Cell<Option<[u8; 3]>>,
}

impl TabHeader {
    /// `closable` shows a close button (the `TabView` draws its own).
    pub fn new(closable: bool) -> Result<Self> {
        let root: FrameworkElement = xaml::load(HEADER_XAML)?;
        let close: Button = xaml::find(&root, "Close")?;
        xaml::set_visible(&close, closable)?;
        let group_line = if closable { "GroupEdge" } else { "GroupLine" };
        let header = Self {
            group_line: xaml::find(&root, group_line)?,
            group_color: Cell::new(None),
            default_icon: xaml::find(&root, "DefaultIcon")?,
            favicon: xaml::find(&root, "Favicon")?,
            spinner: xaml::find(&root, "Spinner")?,
            title: xaml::find(&root, "Title")?,
            buttons: xaml::find(&root, "Buttons")?,
            pin: xaml::find(&root, "Pin")?,
            capture: xaml::find(&root, "Capture")?,
            capture_recording: xaml::find(&root, "CaptureRecording")?,
            capture_sharing: xaml::find(&root, "CaptureSharing")?,
            audio: xaml::find(&root, "Audio")?,
            audio_glyph: xaml::find(&root, "AudioGlyph")?,
            close: closable.then_some(close),
            root,
        };
        header.show_shortcuts();
        Ok(header)
    }

    /// The close button's tooltip, from the bindings in effect.
    pub fn show_shortcuts(&self) {
        if let Some(close) = &self.close {
            let tip = shortcuts::current().tip("Close tab", Command::CloseTab);
            let _ = xaml::set_tip(close, &tip);
        }
    }

    pub fn root(&self) -> &FrameworkElement {
        &self.root
    }

    pub fn close_button(&self) -> Option<&Button> {
        self.close.as_ref()
    }

    pub fn audio_button(&self) -> &Button {
        &self.audio
    }

    pub fn apply(&self, look: &TabLook) {
        let _ = self.title.SetText(&look.title);
        let _ = xaml::set_tip(&self.root, &look.title);
        let favicon = !look.loading && look.favicon.is_some();
        let _ = self.favicon.SetSource(look.favicon.as_ref());
        let _ = self.spinner.SetIsActive(look.loading);
        let _ = xaml::set_visible(&self.spinner, look.loading);
        let _ = xaml::set_visible(&self.favicon, favicon);
        let _ = xaml::set_visible(&self.default_icon, !look.loading && !favicon);
        let opacity = if look.asleep { 0.5 } else { 1.0 };
        let _ = self
            .favicon
            .cast::<UIElement>()
            .and_then(|f| f.SetOpacity(opacity));
        let _ = self.default_icon.SetOpacity(opacity);
        let _ = xaml::set_visible(&self.pin, look.pinned);
        self.show_capture(look.capturing);
        let button = look.audio.button();
        let _ = xaml::set_visible(&self.audio, button.is_some());
        if let Some((glyph, tip)) = button {
            let _ = self.audio_glyph.SetGlyph(glyph);
            let _ = xaml::set_tip(&self.audio, tip);
        }
        // A pinned tab closes from its menu or with Ctrl+W, as in Chrome.
        if let Some(close) = &self.close {
            let _ = xaml::set_visible(close, !look.pinned);
        }
    }

    pub fn capture_shown(&self) -> bool {
        xaml::is_visible(&self.capture)
    }

    /// Shows the tab in its group's colour, or in none.
    pub fn set_group_color(&self, color: Option<[u8; 3]>) -> Result<()> {
        if self.group_color.replace(color) == color {
            return Ok(());
        }
        let children = self.group_line.Children()?;
        children.Clear()?;
        if let Some(rgb) = color {
            children.Append(&color_fill(rgb)?)?;
        }
        xaml::set_visible(&self.group_line, color.is_some())
    }

    fn show_capture(&self, capturing: Capturing) {
        let glyph = capture_glyph(capturing);
        let _ = xaml::set_visible(&self.capture, glyph.is_some());
        let Some((glyph, recording)) = glyph else {
            return;
        };
        let (shown, hidden) = if recording {
            (&self.capture_recording, &self.capture_sharing)
        } else {
            (&self.capture_sharing, &self.capture_recording)
        };
        let _ = shown.SetGlyph(glyph);
        let _ = xaml::set_visible(shown, true);
        let _ = xaml::set_visible(hidden, false);
        if let Some(tip) = capturing.description() {
            let _ = xaml::set_tip(&self.capture, &tip);
        }
    }

    /// Icon only: the collapsed vertical pane.
    pub fn set_compact(&self, compact: bool) {
        let _ = xaml::set_visible(&self.title, !compact);
        let _ = xaml::set_visible(&self.buttons, !compact);
    }
}

/// `#RRGGBB`, as markup writes a colour.
pub(crate) fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A rounded rectangle filled with `rgb`, which fills the panel it is put in. (The bindings
/// carry no brushes, so a colour is set by markup.)
pub(crate) fn color_fill(rgb: [u8; 3]) -> Result<UIElement> {
    xaml::load(&format!(
        r#"<Border {{ns}} Background="{}" CornerRadius="1.5"/>"#,
        hex(rgb)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_written_as_markup_hex() {
        assert_eq!(hex([0x1a, 0x73, 0xe8]), "#1A73E8");
        assert_eq!(hex([0, 0, 0]), "#000000");
    }

    #[test]
    fn capture_glyph_prefers_camera_then_microphone_and_uses_the_permission_glyphs() {
        let all = Capturing {
            camera: true,
            microphone: true,
            screen: true,
        };
        assert_eq!(capture_glyph(all), Some((glyph(Permission::Camera), true)));
        let no_camera = Capturing {
            camera: false,
            ..all
        };
        assert_eq!(
            capture_glyph(no_camera),
            Some((glyph(Permission::Microphone), true))
        );
        let screen = Capturing {
            microphone: false,
            ..no_camera
        };
        assert_eq!(
            capture_glyph(screen),
            Some((glyph(Permission::ScreenShare), false))
        );
        assert_eq!(capture_glyph(Capturing::default()), None);
    }
}
