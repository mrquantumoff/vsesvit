//! The Bookmarks, History, Extensions, Settings and About dialogs.
//!
//! Layouts are complete; lists whose data lives in vsesvit-core (bookmark tree, history,
//! installed extensions, preferences) are named parts the core wiring fills: `BookmarksTree`,
//! `HistoryList`, `ExtensionsList`, `SearchEngine`, `Homepage`.

use std::rc::Rc;

use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::{engine, xaml};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialog {
    Bookmarks,
    History,
    Extensions,
    Settings,
    About,
}

impl Dialog {
    pub const ALL: [Self; 5] = [
        Self::Bookmarks,
        Self::History,
        Self::Extensions,
        Self::Settings,
        Self::About,
    ];
}

const DIALOG_OPEN: &str = r#"<ContentDialog {ns} Title="TITLE" CloseButtonText="Close" DefaultButton="Close"
    Style="{StaticResource DefaultContentDialogStyle}">
  <ContentDialog.Resources>
    <x:Double x:Key="ContentDialogMaxWidth">900</x:Double>
  </ContentDialog.Resources>"#;

const BOOKMARKS: &str = r#"
  <Grid Width="640" RowSpacing="12">
    <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="360"/></Grid.RowDefinitions>
    <Grid ColumnSpacing="8">
      <Grid.ColumnDefinitions><ColumnDefinition/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
      <AutoSuggestBox x:Name="BookmarksSearch" PlaceholderText="Search bookmarks" QueryIcon="Find"/>
      <Button x:Name="BookmarksAddFolder" Grid.Column="1" Content="Add folder" IsEnabled="False"/>
    </Grid>
    <Grid Grid.Row="1">
      <TreeView x:Name="BookmarksTree" CanReorderItems="True" CanDragItems="True" AllowDrop="True"/>
      <TextBlock x:Name="BookmarksEmpty" Text="No bookmarks yet" HorizontalAlignment="Center"
                 VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    </Grid>
  </Grid>"#;

const HISTORY: &str = r#"
  <Grid Width="640" RowSpacing="12">
    <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="360"/></Grid.RowDefinitions>
    <Grid ColumnSpacing="8">
      <Grid.ColumnDefinitions><ColumnDefinition/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
      <AutoSuggestBox x:Name="HistorySearch" PlaceholderText="Search history" QueryIcon="Find"/>
      <Button x:Name="HistoryClear" Grid.Column="1" Content="Clear browsing data" IsEnabled="False"/>
    </Grid>
    <Grid Grid.Row="1">
      <ListView x:Name="HistoryList" SelectionMode="Extended"/>
      <TextBlock x:Name="HistoryEmpty" Text="No history yet" HorizontalAlignment="Center"
                 VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    </Grid>
  </Grid>"#;

const EXTENSIONS: &str = r#"
  <StackPanel Width="560" Spacing="12">
    <TextBlock Text="Add an extension" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <Grid ColumnSpacing="8">
      <Grid.ColumnDefinitions><ColumnDefinition/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
      <TextBox x:Name="InstallSource" PlaceholderText="Chrome Web Store link, extension ID, .crx file or folder"/>
      <Button x:Name="InstallButton" Grid.Column="1" Content="Install" Style="{StaticResource AccentButtonStyle}"/>
    </Grid>
    <TextBlock x:Name="InstallStatus" TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    <TextBlock Text="Installed" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <ListView x:Name="ExtensionsList" MaxHeight="280" SelectionMode="None"/>
    <TextBlock x:Name="ExtensionsEmpty" Text="No extensions installed"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
  </StackPanel>"#;

const SETTINGS: &str = r#"
  <StackPanel Width="480" Spacing="16">
    <ToggleSwitch x:Name="ShowBookmarksBar" Header="Show the bookmarks bar"/>
    <ComboBox x:Name="SearchEngine" Header="Search engine used in the address bar" MinWidth="320"
              PlaceholderText="Default" IsEnabled="False"/>
    <TextBox x:Name="Homepage" Header="Home page" PlaceholderText="https://" IsEnabled="False"/>
    <StackPanel Spacing="4">
      <ToggleSwitch x:Name="UpdatesAutomatic" Header="Download and install updates automatically"/>
      <TextBlock x:Name="UpdatesUnavailable" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                 Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                 Text="This copy of Vsesvit was not installed with the Vsesvit installer, so it does not update itself."/>
    </StackPanel>
    <TextBlock x:Name="ProfilePath" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}" IsTextSelectionEnabled="True"/>
  </StackPanel>"#;

const ABOUT: &str = r#"
  <StackPanel Spacing="6" MinWidth="360">
    <TextBlock x:Name="AboutVersion" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <TextBlock x:Name="AboutEngine" IsTextSelectionEnabled="True"/>
    <TextBlock x:Name="AboutProfile" TextWrapping="Wrap" IsTextSelectionEnabled="True"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
  </StackPanel>"#;

fn markup(dialog: Dialog) -> String {
    let (title, body) = match dialog {
        Dialog::Bookmarks => ("Bookmarks", BOOKMARKS),
        Dialog::History => ("History", HISTORY),
        Dialog::Extensions => ("Extensions", EXTENSIONS),
        Dialog::Settings => ("Settings", SETTINGS),
        Dialog::About => ("About Vsesvit", ABOUT),
    };
    format!(
        "{}{body}\n</ContentDialog>",
        DIALOG_OPEN.replacen("TITLE", title, 1)
    )
}

/// Shows `dialog` over `window` until the user closes it.
pub(crate) async fn show(window: &Rc<BrowserWindow>, dialog: Dialog) -> Result<()> {
    build(window, dialog)?.ShowAsync()?.await?;
    Ok(())
}

/// The dialog, filled and wired, ready to show over `window`.
pub(crate) fn build(window: &Rc<BrowserWindow>, dialog: Dialog) -> Result<ContentDialog> {
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let content: ContentDialog = xaml::load(&markup(dialog))?;
    content
        .cast::<UIElement>()?
        .SetXamlRoot(&window.xaml_root()?)?;
    let root = content.cast::<FrameworkElement>()?;
    match dialog {
        Dialog::Extensions => wire_extensions(&root, &browser, window)?,
        Dialog::Settings => wire_settings(&root, &browser)?,
        Dialog::About => fill_about(&root, &browser)?,
        Dialog::Bookmarks | Dialog::History => {}
    }
    Ok(content)
}

fn wire_extensions(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<()> {
    let source: TextBox = xaml::find(root, "InstallSource")?;
    let status: TextBlock = xaml::find(root, "InstallStatus")?;
    let install: ButtonBase = xaml::find(root, "InstallButton")?;
    let weak = Rc::downgrade(browser);
    install
        .Click(move |_, _| {
            let Some(browser) = weak.upgrade() else {
                return;
            };
            let text = source.Text().unwrap_or_default();
            let message = match browser.install_extension(text.trim()) {
                Ok(()) => "Installing…".to_owned(),
                Err(e) => e,
            };
            let _ = status.SetText(&message);
        })?
        .forget();

    let list: ItemsControl = xaml::find(root, "ExtensionsList")?;
    let empty: UIElement = xaml::find(root, "ExtensionsEmpty")?;
    let window = window.clone();
    crate::exec::spawn(async move {
        let Some(profile) = window.engine_profile().await else {
            return;
        };
        match engine::extensions(&profile).await {
            Ok(extensions) => {
                let Ok(items) = list.Items() else { return };
                for extension in extensions.iter().filter(|e| !e.is_builtin()) {
                    let state = if extension.enabled { "on" } else { "off" };
                    let line = format!("{}  ·  {}  ·  {state}", extension.name, extension.id);
                    if let Ok(item) = xaml::boxed(&line) {
                        let _ = items.Append(&item);
                    }
                }
                let any = items.Size().unwrap_or(0) > 0;
                let _ = xaml::set_visible(&empty, !any);
            }
            Err(e) => log::warn!("listing engine extensions: {e}"),
        }
    });
    Ok(())
}

fn wire_settings(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<()> {
    let toggle: ToggleSwitch = xaml::find(root, "ShowBookmarksBar")?;
    toggle.SetIsOn(browser.bookmarks_bar_visible())?;
    let weak = Rc::downgrade(browser);
    let source = toggle.clone();
    toggle
        .Toggled(move |_, _| {
            if let (Some(browser), Ok(on)) = (weak.upgrade(), source.IsOn()) {
                browser.set_bookmarks_bar_visible(on);
            }
        })?
        .forget();
    let updates: ToggleSwitch = xaml::find(root, "UpdatesAutomatic")?;
    let unavailable: UIElement = xaml::find(root, "UpdatesUnavailable")?;
    let self_updating = !browser.updates().is_disabled();
    updates.SetIsOn(self_updating && browser.updates_automatic())?;
    updates.cast::<Control>()?.SetIsEnabled(self_updating)?;
    xaml::set_visible(&unavailable, !self_updating)?;
    let weak = Rc::downgrade(browser);
    let source = updates.clone();
    updates
        .Toggled(move |_, _| {
            if let (Some(browser), Ok(on)) = (weak.upgrade(), source.IsOn()) {
                browser.set_updates_automatic(on);
            }
        })?
        .forget();

    let path: TextBlock = xaml::find(root, "ProfilePath")?;
    path.SetText(&format!(
        "Profile folder: {}",
        browser.profile_dir().display()
    ))?;
    Ok(())
}

fn fill_about(root: &FrameworkElement, browser: &Browser) -> Result<()> {
    let version: TextBlock = xaml::find(root, "AboutVersion")?;
    version.SetText(&format!("Vsesvit {}", env!("CARGO_PKG_VERSION")))?;
    let engine: TextBlock = xaml::find(root, "AboutEngine")?;
    engine.SetText(&format!(
        "Microsoft Edge WebView2 {}",
        browser.engine().browser_version()
    ))?;
    let profile: TextBlock = xaml::find(root, "AboutProfile")?;
    profile.SetText(&format!("Profile: {}", browser.profile_dir().display()))?;
    Ok(())
}
