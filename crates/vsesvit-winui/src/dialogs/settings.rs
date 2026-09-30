//! Settings, bound to vsesvit-core preferences, in categories down the side as in Windows
//! Settings. Every choice applies at once, in every window, except the engine's startup
//! switches (at the next start) and the home page (written when the dialog closes).

use std::rc::Rc;

use vsesvit_core::prefs::{Pref, Startup, TabsPosition, Theme, UpdateChannel, keys};
use vsesvit_core::search::{SearchEngineId, classify_url};
use windows_core::{Interface, Result};

use super::{Wired, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::updates::StatusButton;
use crate::window::{Backdrop, BrowserWindow};
use crate::{exec, pickers, xaml};

/// A fixed height, so the dialog keeps its size from one category to the next; each category
/// scrolls on its own.
pub(super) const MARKUP: &str = r#"
  <Grid Width="760" Height="560" ColumnSpacing="16">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="200"/>
      <ColumnDefinition Width="*"/>
    </Grid.ColumnDefinitions>
    <ListView x:Name="SettingsCategories" AutomationProperties.Name="Settings categories"/>

    <ScrollViewer x:Name="GeneralPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto">
      <StackPanel Spacing="28" Padding="0,0,0,12">
        <StackPanel Spacing="12">
          <TextBlock Text="On startup" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <ComboBox x:Name="Startup" MinWidth="320" AutomationProperties.Name="On startup"/>
          <TextBox x:Name="Homepage" Header="Home page" PlaceholderText="https://"/>
        </StackPanel>
        <StackPanel Spacing="8">
          <TextBlock Text="Downloads" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <StackPanel Spacing="4">
            <TextBlock Text="Download folder"/>
            <TextBlock x:Name="DownloadFolder" TextWrapping="Wrap" IsTextSelectionEnabled="True"
                       Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
            <StackPanel Orientation="Horizontal" Spacing="8" Margin="0,4,0,0">
              <Button x:Name="DownloadFolderChange" Content="Change…"/>
              <Button x:Name="DownloadFolderReset" Content="Use the default"/>
            </StackPanel>
          </StackPanel>
          <ToggleSwitch x:Name="DownloadsAsk" Header="Ask where to save each file"/>
        </StackPanel>
        <StackPanel Spacing="8">
          <TextBlock Text="Default browser" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          {default_browser}
        </StackPanel>
        <StackPanel Spacing="12">
          <TextBlock Text="System" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="SmoothScrolling" Header="Use smooth scrolling"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="Takes effect when you restart Vsesvit."/>
          </StackPanel>
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="HardwareAcceleration" Header="Use graphics acceleration when available"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="Takes effect when you restart Vsesvit."/>
          </StackPanel>
          <TextBlock x:Name="ProfilePath" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}" IsTextSelectionEnabled="True"/>
        </StackPanel>
        <StackPanel Spacing="12">
          <TextBlock Text="Updates" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <StackPanel Spacing="4">
            <TextBlock x:Name="UpdatesStatus" TextWrapping="Wrap"/>
            <TextBlock x:Name="UpdatesVersion" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}" IsTextSelectionEnabled="True"/>
            <Button x:Name="UpdatesButton" Margin="0,4,0,0"/>
          </StackPanel>
          <StackPanel Spacing="4">
            <ComboBox x:Name="UpdatesChannel" Header="Update channel" MinWidth="320"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="Vsesvit moves to a steadier channel once that channel has a version newer than this one."/>
          </StackPanel>
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="UpdatesAutomatic" Header="Download and install updates automatically"/>
            <TextBlock x:Name="UpdatesUnavailable" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="This copy of Vsesvit was not installed with the Vsesvit installer, so it does not update itself."/>
          </StackPanel>
        </StackPanel>
      </StackPanel>
    </ScrollViewer>

    <ScrollViewer x:Name="AppearancePanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="16" Padding="0,0,0,12">
        <ComboBox x:Name="Theme" Header="Theme" MinWidth="320"/>
        <ComboBox x:Name="TabsPosition" Header="Tabs" MinWidth="320"/>
        <StackPanel Spacing="4">
          <ToggleSwitch x:Name="Transparent" Header="Transparent window"/>
          <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                     Text="Shows a blur of the windows behind Vsesvit (acrylic). When off, the window is tinted by your desktop background (Mica), like other Windows 11 apps."/>
        </StackPanel>
        <ToggleSwitch x:Name="ShowBookmarksBar" Header="Show the bookmarks bar"/>
        <ToggleSwitch x:Name="ShowHomeButton" Header="Show the Home button"/>
      </StackPanel>
    </ScrollViewer>

    <ScrollViewer x:Name="SearchPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="28" Padding="0,0,0,12">
        <ComboBox x:Name="SearchEngine" Header="Search engine used in the address bar" MinWidth="320"/>
        <StackPanel Spacing="12">
          <TextBlock Text="Address bar" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <ToggleSwitch x:Name="CompactAddress" Header="Compact address bar, centered in the toolbar"/>
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="FullUrls" Header="Always show full URLs"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="When off, the address bar leaves out https://, www. and a trailing slash until you click into it."/>
          </StackPanel>
        </StackPanel>
        <StackPanel Spacing="12">
          <TextBlock Text="Suggest while typing" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <ToggleSwitch x:Name="SuggestHistory" Header="Browsing history"/>
          <ToggleSwitch x:Name="SuggestBookmarks" Header="Bookmarks"/>
        </StackPanel>
      </StackPanel>
    </ScrollViewer>

    <ScrollViewer x:Name="SitePermissionsPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="16" Padding="0,0,0,12">
        <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                   Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                   Text="What you allowed or blocked for each site. Other sites ask before they use your camera, microphone, location and more."/>
        <TextBlock x:Name="SitePermissionsEmpty" Text="Sites you allow or block show here." Visibility="Collapsed"/>
        <StackPanel x:Name="SitePermissionsList" Spacing="20"/>
      </StackPanel>
    </ScrollViewer>

    <ScrollViewer x:Name="PrivacyPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="28" Padding="0,0,0,12">
        <StackPanel Spacing="16">
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="BlockPopups" Header="Block pop-ups"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="Sites can still open windows when you click a link or button."/>
          </StackPanel>
          <ToggleSwitch x:Name="SavePasswords" Header="Offer to save passwords"/>
          <ToggleSwitch x:Name="AutofillForms" Header="Save and fill form entries such as addresses"/>
        </StackPanel>
        <StackPanel Spacing="8">
          <TextBlock Text="Browsing data" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                     Text="History, cookies, site data and cached files."/>
          <Button x:Name="ClearBrowsingData" Content="Clear browsing data…" Margin="0,4,0,0">
            <Button.Flyout>
              <Flyout x:Name="ClearBrowsingDataFlyout" Placement="BottomEdgeAlignedLeft">
                <StackPanel Width="320" Spacing="12">
                  <TextBlock Text="Clear browsing data?" Style="{StaticResource BodyStrongTextBlockStyle}"/>
                  <TextBlock TextWrapping="Wrap"
                             Text="Deletes your history on every device you sync with, and this device's cookies, site data and cached files."/>
                  <Button x:Name="ClearBrowsingDataConfirm" Content="Clear" Style="{StaticResource AccentButtonStyle}"/>
                </StackPanel>
              </Flyout>
            </Button.Flyout>
          </Button>
          <TextBlock x:Name="ClearBrowsingDataStatus" TextWrapping="Wrap" Visibility="Collapsed"
                     Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
        </StackPanel>
      </StackPanel>
    </ScrollViewer>
    {shortcuts}
  </Grid>"#;

/// A category down the side of the dialog and the panel of settings it shows.
pub(crate) struct Category {
    pub label: &'static str,
    /// Segoe Fluent Icons.
    pub glyph: &'static str,
    /// The `x:Name` of its `ScrollViewer` in `MARKUP`.
    pub panel: &'static str,
}

pub(crate) const CATEGORIES: [Category; 6] = [
    Category {
        label: "General",
        glyph: "\u{E713}",
        panel: "GeneralPanel",
    },
    Category {
        label: "Appearance",
        glyph: "\u{E790}",
        panel: "AppearancePanel",
    },
    Category {
        label: "Search",
        glyph: "\u{E721}",
        panel: "SearchPanel",
    },
    Category {
        label: "Keyboard shortcuts",
        glyph: "\u{E765}",
        panel: "ShortcutsPanel",
    },
    Category {
        label: "Site permissions",
        glyph: "\u{E8D7}",
        panel: "SitePermissionsPanel",
    },
    Category {
        label: "Privacy and security",
        glyph: "\u{E72E}",
        panel: "PrivacyPanel",
    },
];

pub(crate) const TAB_POSITIONS: [(TabsPosition, &str); 3] = [
    (TabsPosition::Left, "Vertical, on the left"),
    (TabsPosition::Right, "Vertical, on the right"),
    (TabsPosition::Top, "Horizontal, in the title bar"),
];

const STARTUP: [(Startup, &str); 3] = [
    (Startup::RestoreSession, "Continue where you left off"),
    (Startup::Homepage, "Open the home page"),
    (Startup::NewTab, "Open a new tab"),
];

const THEMES: [(Theme, &str); 3] = [
    (Theme::System, "Use the Windows setting"),
    (Theme::Light, "Light"),
    (Theme::Dark, "Dark"),
];

const CHANNELS: [(UpdateChannel, &str); 4] = [
    (UpdateChannel::Stable, "Stable"),
    (UpdateChannel::Beta, "Beta"),
    (UpdateChannel::Weekly, "Weekly, built every Monday"),
    (UpdateChannel::Nightly, "Nightly, built every day"),
];

/// What follows once a switch's preference is written.
type Written = fn(&Browser);

/// Applies a setting in every window.
type Setter = fn(&Browser, bool);

/// Switches bound straight to a preference, and what follows once it is written. What the
/// others change reads its preference when it needs it: the address bar's suggestions as the
/// user types, a page's pop-up as it opens, and the engine's startup switches at the next start.
const PREF_SWITCHES: [(&str, &Pref<bool>, Written); 8] = [
    ("DownloadsAsk", &keys::DOWNLOADS_ASK, |_| {}),
    ("SmoothScrolling", &keys::SMOOTH_SCROLLING, |_| {}),
    ("HardwareAcceleration", &keys::HARDWARE_ACCELERATION, |_| {}),
    ("SuggestHistory", &keys::SUGGEST_HISTORY, |_| {}),
    ("SuggestBookmarks", &keys::SUGGEST_BOOKMARKS, |_| {}),
    ("BlockPopups", &keys::BLOCK_POPUPS, |_| {}),
    (
        "SavePasswords",
        &keys::SAVE_PASSWORDS,
        Browser::apply_autofill,
    ),
    (
        "AutofillForms",
        &keys::AUTOFILL_FORMS,
        Browser::apply_autofill,
    ),
];

/// Switches for settings every window shows at once: each one's name, current state and setter.
fn window_switches(browser: &Browser) -> [(&'static str, bool, Setter); 5] {
    [
        (
            "ShowBookmarksBar",
            browser.bookmarks_bar_visible(),
            Browser::set_bookmarks_bar_visible,
        ),
        (
            "ShowHomeButton",
            browser.home_button_visible(),
            Browser::set_home_button_visible,
        ),
        (
            "CompactAddress",
            browser.compact_address(),
            Browser::set_compact_address,
        ),
        ("FullUrls", browser.full_urls(), Browser::set_full_urls),
        (
            "Transparent",
            browser.backdrop() == Backdrop::Acrylic,
            |b, on| {
                b.set_backdrop(if on {
                    Backdrop::Acrylic
                } else {
                    Backdrop::Mica
                })
            },
        ),
    ]
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Wired> {
    let weak = Rc::downgrade(browser);
    wire_categories(root)?;
    wire_downloads(root, browser, window)?;
    wire_clear_browsing_data(root, browser)?;
    super::site_permissions::wire(root, browser)?;
    let shortcuts = super::shortcut_settings::wire(root, browser, window)?;

    let tabs: ComboBox = xaml::find(root, "TabsPosition")?;
    let w = weak.clone();
    choices(
        &tabs,
        &TAB_POSITIONS,
        browser.tabs_position(),
        move |position| {
            if let Some(b) = w.upgrade() {
                b.set_tabs_position(position);
            }
        },
    )?;

    let (engines, default) = browser.core(|p| {
        let engines = p.search_engines().list().unwrap_or_default();
        let default = p.search_engines().default_engine().ok().map(|e| e.id);
        (engines, default)
    });
    let engine_choices: Vec<(SearchEngineId, String)> =
        engines.into_iter().map(|e| (e.id, e.name)).collect();
    let search: ComboBox = xaml::find(root, "SearchEngine")?;
    let named: Vec<(SearchEngineId, &str)> = engine_choices
        .iter()
        .map(|(id, name)| (id.clone(), name.as_str()))
        .collect();
    let w = weak.clone();
    choices(
        &search,
        &named,
        default.unwrap_or_else(SearchEngineId::builtin_default),
        move |id| {
            if let Some(b) = w.upgrade()
                && let Err(e) = b.core(|p| p.search_engines().set_default(&id))
            {
                log::warn!("default search engine: {e}");
            }
        },
    )?;

    let startup: ComboBox = xaml::find(root, "Startup")?;
    let current = browser.core(|p| p.prefs().get(&keys::STARTUP));
    let w = weak.clone();
    choices(&startup, &STARTUP, current, move |choice| {
        if let Some(b) = w.upgrade() {
            b.write_pref(&keys::STARTUP, &choice);
        }
    })?;

    let theme: ComboBox = xaml::find(root, "Theme")?;
    let w = weak.clone();
    choices(&theme, &THEMES, browser.theme(), move |theme| {
        if let Some(b) = w.upgrade() {
            b.set_theme(theme);
        }
    })?;

    for (name, pref, written) in PREF_SWITCHES {
        let on = browser.core(|p| p.prefs().get(pref));
        switch(root, browser, name, on, move |b, on| {
            b.write_pref(pref, &on);
            written(b);
        })?;
    }
    for (name, on, set) in window_switches(browser) {
        switch(root, browser, name, on, set)?;
    }

    let updates = wire_updates(root, browser)?;

    let homepage: TextBox = xaml::find(root, "Homepage")?;
    let stored = browser.core(|p| p.prefs().get(&keys::HOMEPAGE));
    homepage.SetText(if stored == "about:home" { "" } else { &stored })?;
    let path: TextBlock = xaml::find(root, "ProfilePath")?;
    path.SetText(&format!(
        "Profile folder: {}",
        browser.profile_dir().display()
    ))?;

    let default_browser = super::default_browser::wire(root)?;
    let w = weak;
    Ok(Wired {
        _alive: vec![default_browser, updates, shortcuts],
        on_close: Some(Box::new(move || {
            let Some(b) = w.upgrade() else { return };
            let text = homepage.Text().unwrap_or_default();
            match homepage_value(&text) {
                Some(HomepageValue::Default) => {
                    if let Err(e) = b.core(|p| p.prefs().reset(&keys::HOMEPAGE)) {
                        log::warn!("home page: {e}");
                    }
                }
                Some(HomepageValue::Url(url)) => b.write_pref(&keys::HOMEPAGE, &url),
                None => log::info!("home page {text:?} is not a web address; kept the old one"),
            }
        })),
    })
}

/// Fills the category list from `CATEGORIES` and shows the selected one's panel.
fn wire_categories(root: &FrameworkElement) -> Result<()> {
    let list: ListView = xaml::find(root, "SettingsCategories")?;
    let items = list.cast::<ItemsControl>()?.Items()?;
    let mut panels = Vec::new();
    for category in &CATEGORIES {
        let item: UIElement = xaml::load(&format!(
            r#"<StackPanel {{ns}} Orientation="Horizontal" Spacing="12">
  <FontIcon Glyph="{}" FontSize="16"/>
  <TextBlock Text="{}" VerticalAlignment="Center"/>
</StackPanel>"#,
            category.glyph,
            xaml::escape(category.label)
        ))?;
        items.Append(&item)?;
        panels.push(xaml::find::<UIElement>(root, category.panel)?);
    }
    let selector = list.cast::<Selector>()?;
    let source = selector.clone();
    selector
        .SelectionChanged(move |_, _| {
            // Ctrl+click can leave nothing selected; the panel shown stays.
            let Some(selected) = source
                .SelectedIndex()
                .ok()
                .and_then(|i| usize::try_from(i).ok())
            else {
                return;
            };
            for (index, panel) in panels.iter().enumerate() {
                let _ = xaml::set_visible(panel, index == selected);
            }
        })?
        .forget();
    selector.SetSelectedIndex(0)
}

/// Sets the switch `name` to `on`, and calls `toggled` when the user flips it.
fn switch(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    name: &str,
    on: bool,
    toggled: impl Fn(&Browser, bool) + 'static,
) -> Result<ToggleSwitch> {
    let switch: ToggleSwitch = xaml::find(root, name)?;
    switch.SetIsOn(on)?;
    let (b, source) = (Rc::downgrade(browser), switch.clone());
    switch
        .Toggled(move |_, _| {
            if let (Some(b), Ok(on)) = (b.upgrade(), source.IsOn()) {
                toggled(&b, on);
            }
        })?
        .forget();
    Ok(switch)
}

/// The Updates group. Its status line and button follow the update state while the dialog is
/// open; the returned listener is what keeps them following, so the dialog must keep it.
fn wire_updates(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<Rc<dyn std::any::Any>> {
    let self_updating = !browser.updates().is_disabled();
    let automatic = switch(
        root,
        browser,
        "UpdatesAutomatic",
        self_updating && browser.updates_automatic(),
        Browser::set_updates_automatic,
    )?;
    automatic.cast::<Control>()?.SetIsEnabled(self_updating)?;
    let unavailable: UIElement = xaml::find(root, "UpdatesUnavailable")?;
    xaml::set_visible(&unavailable, !self_updating)?;

    let channel: ComboBox = xaml::find(root, "UpdatesChannel")?;
    let w = Rc::downgrade(browser);
    choices(
        &channel,
        &CHANNELS,
        browser.updates_channel(),
        move |channel| {
            if let Some(b) = w.upgrade() {
                b.set_updates_channel(channel);
            }
        },
    )?;
    channel.cast::<Control>()?.SetIsEnabled(self_updating)?;

    xaml::find::<TextBlock>(root, "UpdatesVersion")?
        .SetText(&format!("Version {}", env!("CARGO_PKG_VERSION")))?;
    let text: TextBlock = xaml::find(root, "UpdatesStatus")?;
    let button: Button = xaml::find(root, "UpdatesButton")?;
    let w = Rc::downgrade(browser);
    on_click(&button, move || {
        let Some(b) = w.upgrade() else { return };
        match b.updates().status().button {
            StatusButton::Check { .. } => b.check_for_updates(),
            StatusButton::Banner(action) => b.update_action(action),
        }
    })?;
    let w = Rc::downgrade(browser);
    let show: Rc<dyn Fn()> = Rc::new(move || {
        let Some(b) = w.upgrade() else { return };
        let status = b.updates().status();
        let _ = text.SetText(status.text.as_deref().unwrap_or_default());
        let _ = xaml::set_visible(&text, status.text.is_some());
        let _ = xaml::boxed(status.button.label())
            .and_then(|label| button.cast::<IContentControl>()?.SetContent(&label));
        let _ = button
            .cast::<Control>()
            .and_then(|c| c.SetIsEnabled(status.button.is_enabled()));
    });
    show();
    browser.updates().on_change(&show);
    Ok(Rc::new(show))
}

/// Clear browsing data: the button's flyout asks first (no second dialog can open over this
/// one), and the outcome shows under the button.
fn wire_clear_browsing_data(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<()> {
    let flyout: FlyoutBase = xaml::find(root, "ClearBrowsingDataFlyout")?;
    let status: TextBlock = xaml::find(root, "ClearBrowsingDataStatus")?;
    let b = Rc::downgrade(browser);
    on_click(
        &xaml::find::<Button>(root, "ClearBrowsingDataConfirm")?,
        move || {
            let _ = flyout.Hide();
            let Some(b) = b.upgrade() else { return };
            let status = status.clone();
            exec::spawn(async move {
                let text = match clear_browsing_data(&b).await {
                    Ok(()) => "Browsing data cleared.".to_owned(),
                    Err(e) => {
                        log::warn!("clear browsing data: {e}");
                        format!("Could not clear all browsing data: {e}")
                    }
                };
                let _ = status.SetText(&text);
                let _ = xaml::set_visible(&status, true);
            });
        },
    )
}

/// Deletes history on every synced device, then this device's cookies, site data and cache.
/// Saved passwords and form entries stay, as with Chrome's defaults.
async fn clear_browsing_data(browser: &Browser) -> std::result::Result<(), String> {
    browser
        .core(|p| p.history().delete_range(0, super::history::now_ms()))
        .map_err(|e| format!("history: {e}"))?;
    let profile = browser
        .engine_profile()
        .await
        .ok_or("the web engine is not ready")?;
    let kinds = CoreWebView2BrowsingDataKinds(
        CoreWebView2BrowsingDataKinds::AllSite.0
            | CoreWebView2BrowsingDataKinds::DiskCache.0
            | CoreWebView2BrowsingDataKinds::BrowsingHistory.0,
    );
    let cleared: Result<()> = async {
        profile
            .cast::<ICoreWebView2Profile2>()?
            .ClearBrowsingDataAsync(kinds)?
            .await
    }
    .await;
    cleared.map_err(|e| e.message())
}

/// The download folder's path and the button that goes back to the default.
struct DownloadFolder {
    path: TextBlock,
    reset: UIElement,
}

impl DownloadFolder {
    fn show(&self, browser: &Browser) {
        let _ = self.path.SetText(&browser.download_dir().to_string_lossy());
        let _ = xaml::set_visible(&self.reset, browser.custom_download_dir().is_some());
    }
}

/// The Downloads group's folder, with Change and Reset.
fn wire_downloads(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<()> {
    let folder = Rc::new(DownloadFolder {
        path: xaml::find(root, "DownloadFolder")?,
        reset: xaml::find(root, "DownloadFolderReset")?,
    });
    folder.show(browser);

    let (b, w, f) = (
        Rc::downgrade(browser),
        Rc::downgrade(window),
        folder.clone(),
    );
    on_click(
        &xaml::find::<Button>(root, "DownloadFolderChange")?,
        move || {
            let Some(window) = w.upgrade() else { return };
            let owner = match window.window_id() {
                Ok(owner) => owner,
                Err(e) => {
                    log::warn!("picker owner: {e}");
                    return;
                }
            };
            let (b, f) = (b.clone(), f.clone());
            exec::spawn(async move {
                let picked = pickers::pick_folder(owner).await;
                let Some(b) = b.upgrade() else { return };
                match picked {
                    Ok(Some(dir)) => {
                        b.set_download_dir(Some(&dir));
                        f.show(&b);
                    }
                    Ok(None) => {}
                    Err(e) => log::warn!("download folder picker: {e}"),
                }
            });
        },
    )?;
    let (b, f) = (Rc::downgrade(browser), folder);
    on_click(
        &xaml::find::<Button>(root, "DownloadFolderReset")?,
        move || {
            if let Some(b) = b.upgrade() {
                b.set_download_dir(None);
                f.show(&b);
            }
        },
    )
}

#[derive(Debug, PartialEq, Eq)]
enum HomepageValue {
    /// The default: a new tab.
    Default,
    Url(String),
}

/// What the home page box holds: nothing (the default) or a web address.
fn homepage_value(text: &str) -> Option<HomepageValue> {
    let text = text.trim();
    if text.is_empty() {
        return Some(HomepageValue::Default);
    }
    classify_url(text).map(|target| HomepageValue::Url(target.url().to_string()))
}

/// Fills `combo` with the labels of `options`, selects `current`, and calls `chosen` when the
/// user picks another.
fn choices<T: Clone + PartialEq + 'static>(
    combo: &ComboBox,
    options: &[(T, &str)],
    current: T,
    chosen: impl Fn(T) + 'static,
) -> Result<()> {
    let items = combo.cast::<ItemsControl>()?.Items()?;
    for (_, label) in options {
        items.Append(&xaml::boxed(label)?)?;
    }
    let selector = combo.cast::<Selector>()?;
    if let Some(index) = options.iter().position(|(value, _)| *value == current) {
        selector.SetSelectedIndex(i32::try_from(index).unwrap_or(-1))?;
    }
    let values: Vec<T> = options.iter().map(|(value, _)| value.clone()).collect();
    // A combo box re-raises SelectionChanged for its initial selection when it loads; only a
    // different choice is the user's.
    let shown = std::cell::RefCell::new(current);
    let source = selector.clone();
    selector
        .SelectionChanged(move |_, _| {
            let index = source
                .SelectedIndex()
                .ok()
                .and_then(|i| usize::try_from(i).ok());
            if let Some(value) = index.and_then(|i| values.get(i))
                && *value != *shown.borrow()
            {
                *shown.borrow_mut() = value.clone();
                chosen(value.clone());
            }
        })?
        .forget();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_update_channel_has_a_label() {
        assert_eq!(CHANNELS.map(|(channel, _)| channel), UpdateChannel::ALL);
    }

    #[test]
    fn homepage_is_empty_or_an_address() {
        assert_eq!(homepage_value("  "), Some(HomepageValue::Default));
        assert_eq!(
            homepage_value("example.com"),
            Some(HomepageValue::Url("https://example.com/".into()))
        );
        assert_eq!(homepage_value("not an address"), None);
    }
}
