//! How one tab looks in a tab list: favicon (or a spinner while loading), title, a speaker while
//! the page plays sound (it mutes the tab), a pin for pinned tabs, and in the vertical pane a
//! close button. Each list builds its own header per tab from a `TabLook`.

use windows_core::Result;

use crate::bindings::*;
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
}

// No column spacing: the gaps are margins of the parts that collapse, so a row of the collapsed
// pane is exactly as wide as its icon.
const HEADER_XAML: &str = r#"
<Grid {ns} Background="Transparent">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="16"/>
    <ColumnDefinition Width="*"/>
    <ColumnDefinition Width="Auto"/>
  </Grid.ColumnDefinitions>
  <FontIcon x:Name="DefaultIcon" Glyph="&#xE774;" FontSize="14" VerticalAlignment="Center"/>
  <Image x:Name="Favicon" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
  <ProgressRing x:Name="Spinner" Width="16" Height="16" MinWidth="16" MinHeight="16"
                VerticalAlignment="Center" IsActive="False" Visibility="Collapsed"/>
  <TextBlock x:Name="Title" Grid.Column="1" Margin="8,0,0,0" Text="New tab"
             TextTrimming="CharacterEllipsis" TextWrapping="NoWrap" VerticalAlignment="Center"/>
  <StackPanel x:Name="Buttons" Grid.Column="2" Orientation="Horizontal" Margin="4,0,0,0">
    <FontIcon x:Name="Pin" Glyph="&#xE718;" FontSize="10" Margin="0,0,6,0" Visibility="Collapsed"
              VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    <Button x:Name="Audio" Width="24" Height="24" Padding="0" Background="Transparent"
            BorderThickness="0" VerticalAlignment="Center" Visibility="Collapsed"
            AutomationProperties.Name="Mute or unmute tab">
      <FontIcon x:Name="AudioGlyph" Glyph="&#xE767;" FontSize="12"/>
    </Button>
    <Button x:Name="Close" Width="24" Height="24" Padding="0" Background="Transparent"
            BorderThickness="0" VerticalAlignment="Center" Visibility="Collapsed"
            ToolTipService.ToolTip="Close tab (Ctrl+W)" AutomationProperties.Name="Close tab">
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
    audio: Button,
    audio_glyph: FontIcon,
    close: Option<Button>,
}

impl TabHeader {
    /// `closable` shows a close button (the `TabView` draws its own).
    pub fn new(closable: bool) -> Result<Self> {
        let root: FrameworkElement = xaml::load(HEADER_XAML)?;
        let close: Button = xaml::find(&root, "Close")?;
        xaml::set_visible(&close, closable)?;
        Ok(Self {
            default_icon: xaml::find(&root, "DefaultIcon")?,
            favicon: xaml::find(&root, "Favicon")?,
            spinner: xaml::find(&root, "Spinner")?,
            title: xaml::find(&root, "Title")?,
            buttons: xaml::find(&root, "Buttons")?,
            pin: xaml::find(&root, "Pin")?,
            audio: xaml::find(&root, "Audio")?,
            audio_glyph: xaml::find(&root, "AudioGlyph")?,
            close: closable.then_some(close),
            root,
        })
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
        let _ =
            xaml::boxed(&look.title).and_then(|tip| ToolTipService::SetToolTip(&self.root, &tip));
        let favicon = !look.loading && look.favicon.is_some();
        let _ = self.favicon.SetSource(look.favicon.as_ref());
        let _ = self.spinner.SetIsActive(look.loading);
        let _ = xaml::set_visible(&self.spinner, look.loading);
        let _ = xaml::set_visible(&self.favicon, favicon);
        let _ = xaml::set_visible(&self.default_icon, !look.loading && !favicon);
        let _ = xaml::set_visible(&self.pin, look.pinned);
        let button = look.audio.button();
        let _ = xaml::set_visible(&self.audio, button.is_some());
        if let Some((glyph, tip)) = button {
            let _ = self.audio_glyph.SetGlyph(glyph);
            let _ = xaml::boxed(tip).and_then(|tip| ToolTipService::SetToolTip(&self.audio, &tip));
        }
        // A pinned tab closes from its menu or with Ctrl+W, as in Chrome.
        if let Some(close) = &self.close {
            let _ = xaml::set_visible(close, !look.pinned);
        }
    }

    /// Icon only: the collapsed vertical pane.
    pub fn set_compact(&self, compact: bool) {
        let _ = xaml::set_visible(&self.title, !compact);
        let _ = xaml::set_visible(&self.buttons, !compact);
    }
}
