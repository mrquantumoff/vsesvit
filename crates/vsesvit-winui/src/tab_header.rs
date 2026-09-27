//! How one tab looks in a tab list: favicon (or a spinner while loading), title, and in the
//! vertical pane a close button. Each list builds its own header per tab from a `TabLook`.

use windows_core::Result;

use crate::bindings::*;
use crate::xaml;

/// What a tab list shows for a tab.
#[derive(Clone, Default)]
pub(crate) struct TabLook {
    pub title: String,
    pub favicon: Option<ImageSource>,
    pub loading: bool,
}

const HEADER_XAML: &str = r#"
<Grid {ns} ColumnSpacing="8" Background="Transparent">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="16"/>
    <ColumnDefinition Width="*"/>
    <ColumnDefinition Width="Auto"/>
  </Grid.ColumnDefinitions>
  <FontIcon x:Name="DefaultIcon" Glyph="&#xE774;" FontSize="14" VerticalAlignment="Center"/>
  <Image x:Name="Favicon" Width="16" Height="16" VerticalAlignment="Center" Visibility="Collapsed"/>
  <ProgressRing x:Name="Spinner" Width="16" Height="16" MinWidth="16" MinHeight="16"
                VerticalAlignment="Center" IsActive="False" Visibility="Collapsed"/>
  <TextBlock x:Name="Title" Grid.Column="1" Text="New tab" TextTrimming="CharacterEllipsis"
             TextWrapping="NoWrap" VerticalAlignment="Center"/>
  <Button x:Name="Close" Grid.Column="2" Width="24" Height="24" Padding="0" Background="Transparent"
          BorderThickness="0" VerticalAlignment="Center" Visibility="Collapsed"
          ToolTipService.ToolTip="Close tab (Ctrl+W)" AutomationProperties.Name="Close tab">
    <FontIcon Glyph="&#xE711;" FontSize="10"/>
  </Button>
</Grid>"#;

pub(crate) struct TabHeader {
    root: FrameworkElement,
    default_icon: UIElement,
    favicon: Image,
    spinner: ProgressRing,
    title: TextBlock,
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
    }

    /// Icon only: the collapsed vertical pane.
    pub fn set_compact(&self, compact: bool) {
        let _ = xaml::set_visible(&self.title, !compact);
        if let Some(close) = &self.close {
            let _ = xaml::set_visible(close, !compact);
        }
    }
}
