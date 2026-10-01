//! The media player at the foot of the vertical tab pane: the tab that played sound last, its
//! track, play/pause, previous/next and mute, and above them a picture-in-picture box that holds
//! the tab's own web view while another tab is selected, or for sound alone the page's artwork.
//! Collapsed, the pane keeps only play/pause.

use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::dialogs::on_click;
use crate::media::MediaAction;
use crate::xaml;

/// What the player reports to its window.
pub(crate) struct PlayerEvents {
    pub go_to_tab: Box<dyn Fn()>,
    pub action: Box<dyn Fn(MediaAction)>,
    pub toggle_muted: Box<dyn Fn()>,
}

/// What the player shows.
#[derive(Clone, Default)]
pub(crate) struct PlayerLook {
    pub title: String,
    pub artist: String,
    pub favicon: Option<ImageSource>,
    pub playing: bool,
    pub muted: bool,
    pub previous: bool,
    pub next: bool,
}

/// The gap under the picture-in-picture box.
const PIP_GAP: f64 = 6.0;

const PLAYER_XAML: &str = r#"
<StackPanel {ns} Padding="0,6,0,0">
  <Grid x:Name="PipHost" Height="130" Margin="0,0,0,6" CornerRadius="6" Background="Black"
        Visibility="Collapsed">
    <Image x:Name="Artwork" Stretch="Uniform" Visibility="Collapsed"/>
    <Border x:Name="PipShield" Background="Transparent" ToolTipService.ToolTip="Go to tab"
            AutomationProperties.Name="Picture in picture"/>
  </Grid>
  <Grid x:Name="Controls" CornerRadius="6" Padding="4" RowSpacing="2"
        Background="{ThemeResource SubtleFillColorSecondaryBrush}">
    <Grid.RowDefinitions>
      <RowDefinition Height="Auto"/>
      <RowDefinition Height="Auto"/>
    </Grid.RowDefinitions>
    <Button x:Name="MediaTab" HorizontalAlignment="Stretch" HorizontalContentAlignment="Left"
            Padding="6,2" Background="Transparent" BorderThickness="0"
            ToolTipService.ToolTip="Go to tab" AutomationProperties.Name="Go to the playing tab">
      <Grid ColumnSpacing="8">
        <Grid.ColumnDefinitions>
          <ColumnDefinition Width="16"/>
          <ColumnDefinition Width="*"/>
        </Grid.ColumnDefinitions>
        <FontIcon x:Name="MediaGlyph" Glyph="&#xE8D6;" FontSize="14" VerticalAlignment="Center"/>
        <Image x:Name="MediaIcon" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
        <StackPanel Grid.Column="1" VerticalAlignment="Center">
          <TextBlock x:Name="MediaTitle" TextTrimming="CharacterEllipsis" TextWrapping="NoWrap"/>
          <TextBlock x:Name="MediaArtist" TextTrimming="CharacterEllipsis" TextWrapping="NoWrap"
                     Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
        </StackPanel>
      </Grid>
    </Button>
    <StackPanel Grid.Row="1" Orientation="Horizontal" HorizontalAlignment="Center" Spacing="4">
      <Button x:Name="Previous" Width="36" Height="32" Padding="0" Background="Transparent" BorderThickness="0"
              ToolTipService.ToolTip="Previous track" AutomationProperties.Name="Previous track">
        <FontIcon Glyph="&#xE892;" FontSize="14"/>
      </Button>
      <Button x:Name="PlayPause" Width="36" Height="32" Padding="0" Background="Transparent" BorderThickness="0"
              AutomationProperties.Name="Play or pause">
        <FontIcon x:Name="PlayPauseGlyph" Glyph="&#xE768;" FontSize="14"/>
      </Button>
      <Button x:Name="Next" Width="36" Height="32" Padding="0" Background="Transparent" BorderThickness="0"
              ToolTipService.ToolTip="Next track" AutomationProperties.Name="Next track">
        <FontIcon Glyph="&#xE893;" FontSize="14"/>
      </Button>
      <Button x:Name="Mute" Width="36" Height="32" Padding="0" Background="Transparent" BorderThickness="0"
              AutomationProperties.Name="Mute or unmute tab">
        <FontIcon x:Name="MuteGlyph" Glyph="&#xE767;" FontSize="14"/>
      </Button>
    </StackPanel>
  </Grid>
</StackPanel>"#;

pub(crate) struct Player {
    root: FrameworkElement,
    pip_host: Panel,
    artwork: Image,
    media_tab: UIElement,
    glyph: UIElement,
    icon: Image,
    title: TextBlock,
    artist: TextBlock,
    previous: Button,
    play_pause: Button,
    play_pause_glyph: FontIcon,
    next: Button,
    mute: Button,
    mute_glyph: FontIcon,
}

impl Player {
    pub fn new(events: PlayerEvents) -> Result<Self> {
        let root: FrameworkElement = xaml::load(PLAYER_XAML)?;
        let this = Self {
            pip_host: xaml::find(&root, "PipHost")?,
            artwork: xaml::find(&root, "Artwork")?,
            media_tab: xaml::find(&root, "MediaTab")?,
            glyph: xaml::find(&root, "MediaGlyph")?,
            icon: xaml::find(&root, "MediaIcon")?,
            title: xaml::find(&root, "MediaTitle")?,
            artist: xaml::find(&root, "MediaArtist")?,
            previous: xaml::find(&root, "Previous")?,
            play_pause: xaml::find(&root, "PlayPause")?,
            play_pause_glyph: xaml::find(&root, "PlayPauseGlyph")?,
            next: xaml::find(&root, "Next")?,
            mute: xaml::find(&root, "Mute")?,
            mute_glyph: xaml::find(&root, "MuteGlyph")?,
            root,
        };
        xaml::set_visible(&this.root, false)?;
        let events = std::rc::Rc::new(events);
        let e = events.clone();
        on_click(&this.media_tab, move || (e.go_to_tab)())?;
        for (button, action) in [
            (&this.previous, MediaAction::Previous),
            (&this.play_pause, MediaAction::PlayPause),
            (&this.next, MediaAction::Next),
        ] {
            let e = events.clone();
            on_click(button, move || (e.action)(action))?;
        }
        let e = events.clone();
        on_click(&this.mute, move || (e.toggle_muted)())?;
        let host = this.pip_host.cast::<FrameworkElement>()?;
        let root = this.root.clone();
        this.root
            .SizeChanged(move |_, _| {
                if let Ok(width) = root.ActualWidth() {
                    let _ = host.SetHeight(pip_height(width));
                }
            })?
            .forget();
        let shield: UIElement = xaml::find(&this.root, "PipShield")?;
        let e = events;
        shield
            .PointerReleased(move |_, _| (e.go_to_tab)())?
            .forget();
        Ok(this)
    }

    pub fn element(&self) -> &FrameworkElement {
        &self.root
    }

    /// The box a picture-in-picture web view goes in, under its click shield.
    pub fn pip_host(&self) -> &Panel {
        &self.pip_host
    }

    /// The height the picture-in-picture box and its gap take while shown.
    pub fn pip_block(&self) -> f64 {
        let width = self.root.ActualWidth().unwrap_or(0.0);
        pip_height(width) + PIP_GAP
    }

    /// Shows the artwork at `url` in the picture-in-picture box, or none.
    pub fn set_artwork(&self, url: Option<&str>) {
        let source = url.and_then(|url| {
            let image = BitmapImage::new().ok()?;
            image.SetUriSource(&Uri::CreateUri(url).ok()?).ok()?;
            image.cast::<ImageSource>().ok()
        });
        let _ = self.artwork.SetSource(source.as_ref());
        let _ = xaml::set_visible(&self.artwork, source.is_some());
    }

    pub fn set_pip_visible(&self, visible: bool) {
        let _ = xaml::set_visible(&self.pip_host, visible);
    }

    /// Shows `look`, or hides the player.
    pub fn show(&self, look: Option<&PlayerLook>) {
        let _ = xaml::set_visible(&self.root, look.is_some());
        let Some(look) = look else { return };
        let _ = self.title.SetText(&look.title);
        let _ = self.artist.SetText(&look.artist);
        let _ = xaml::set_visible(&self.artist, !look.artist.is_empty());
        let _ = self.icon.SetSource(look.favicon.as_ref());
        let _ = xaml::set_visible(&self.icon, look.favicon.is_some());
        let _ = xaml::set_visible(&self.glyph, look.favicon.is_none());
        let (glyph, tip) = if look.playing {
            ("\u{E769}", "Pause")
        } else {
            ("\u{E768}", "Play")
        };
        let _ = self.play_pause_glyph.SetGlyph(glyph);
        let _ = xaml::set_tip(&self.play_pause, tip);
        let (glyph, tip) = if look.muted {
            ("\u{E74F}", "Unmute tab")
        } else {
            ("\u{E767}", "Mute tab")
        };
        let _ = self.mute_glyph.SetGlyph(glyph);
        let _ = xaml::set_tip(&self.mute, tip);
        let _ = self.previous.cast::<Control>().and_then(|c| c.SetIsEnabled(look.previous));
        let _ = self.next.cast::<Control>().and_then(|c| c.SetIsEnabled(look.next));
    }

    /// The collapsed pane has room for play/pause only.
    pub fn set_compact(&self, compact: bool) {
        let _ = xaml::set_visible(&self.media_tab, !compact);
        let _ = xaml::set_visible(&self.previous, !compact);
        let _ = xaml::set_visible(&self.next, !compact);
        let _ = xaml::set_visible(&self.mute, !compact);
    }
}

/// 16:9, as most video is.
fn pip_height(width: f64) -> f64 {
    (width * 9.0 / 16.0).round()
}
