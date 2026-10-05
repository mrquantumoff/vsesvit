//! Settings, bound to vsesvit-core preferences, in categories down the side as in Windows
//! Settings. Every choice applies at once, in every window, except the engine's startup
//! switches (at the next start), tracking protection (from each page's next load), the home page
//! (written when the dialog closes) and the sync server (written when its box loses focus).

use std::rc::Rc;

use vsesvit_core::prefs::{
    HomepageValue, Pref, Startup, TabsPosition, Theme, UpdateChannel, homepage_input, keys,
};
use vsesvit_core::search::SearchEngineId;
use vsesvit_core::sync::Changed;
use vsesvit_core::trackers::TrackingProtection;
use windows_core::{Interface, Result};

use super::{Category, Wired, on_click, side_list};
use crate::bindings::*;
use crate::browser::Browser;
use crate::sync::Applied;
use crate::updates::StatusButton;
use crate::window::{Backdrop, BrowserWindow};
use crate::{exec, pickers, xaml};

/// Fills the window, which keeps its size from one category to the next; each category scrolls
/// on its own.
pub(super) const MARKUP: &str = r#"
  <Grid ColumnSpacing="16">
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

    {sync}

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
        <StackPanel Spacing="4">
          <ToggleSwitch x:Name="ShowMediaPlayer" Header="Show media player"/>
          <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                     Text="Controls what the last tab to play sound is playing, at the foot of the vertical tab list."/>
        </StackPanel>
        <StackPanel Spacing="4">
          <ToggleSwitch x:Name="PictureInPicture" Header="Picture-in-picture"/>
          <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                     Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                     Text="Shows the playing video in the media player while you look at other tabs, on sites where you turn it on with the picture-in-picture button in the address bar."/>
        </StackPanel>
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
            <ComboBox x:Name="TrackingProtection" Header="Tracking protection" MinWidth="320"/>
            <TextBlock x:Name="TrackingProtectionDescription" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
          </StackPanel>
          <StackPanel Spacing="4">
            <ToggleSwitch x:Name="BlockPopups" Header="Block pop-ups"/>
            <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                       Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                       Text="Sites can still open windows when you click a link or button."/>
          </StackPanel>
          <ToggleSwitch x:Name="AutofillForms" Header="Save and fill form entries such as addresses"/>
        </StackPanel>
        <StackPanel x:Name="PasswordsNotice" Spacing="4">
          <TextBlock Text="Passwords" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <TextBlock TextWrapping="Wrap"
                     Text="Vsesvit doesn't save passwords. Use a password manager such as Bitwarden or Proton Pass through its browser extension."/>
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

pub(crate) const CATEGORIES: [Category; 7] = [
    Category {
        label: "General",
        glyph: "\u{E713}",
        panel: "GeneralPanel",
    },
    Category {
        label: "Sync",
        glyph: "\u{E895}",
        panel: "SyncPanel",
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

/// Reads a setting's current state.
type Getter = fn(&Browser) -> bool;

/// Shows a setting's stored value again in its control, after a sync changed it.
type Shown = Box<dyn Fn(&Browser)>;

type Follow = Vec<Shown>;

/// Switches bound straight to a preference, and what follows once it is written. What the
/// others change reads its preference when it needs it: the address bar's suggestions as the
/// user types, a page's pop-up as it opens, and the engine's startup switches at the next start.
const PREF_SWITCHES: [(&str, &Pref<bool>, Written); 7] = [
    ("DownloadsAsk", &keys::DOWNLOADS_ASK, |_| {}),
    ("SmoothScrolling", &keys::SMOOTH_SCROLLING, |_| {}),
    ("HardwareAcceleration", &keys::HARDWARE_ACCELERATION, |_| {}),
    ("SuggestHistory", &keys::SUGGEST_HISTORY, |_| {}),
    ("SuggestBookmarks", &keys::SUGGEST_BOOKMARKS, |_| {}),
    ("BlockPopups", &keys::BLOCK_POPUPS, |_| {}),
    (
        "AutofillForms",
        &keys::AUTOFILL_FORMS,
        Browser::apply_autofill,
    ),
];

/// Switches for settings every window shows at once: each one's name, state and setter.
const WINDOW_SWITCHES: [(&str, Getter, Setter); 7] = [
    (
        "ShowBookmarksBar",
        Browser::bookmarks_bar_visible,
        Browser::set_bookmarks_bar_visible,
    ),
    (
        "ShowHomeButton",
        Browser::home_button_visible,
        Browser::set_home_button_visible,
    ),
    (
        "CompactAddress",
        Browser::compact_address,
        Browser::set_compact_address,
    ),
    ("FullUrls", Browser::full_urls, Browser::set_full_urls),
    (
        "ShowMediaPlayer",
        Browser::media_player_visible,
        Browser::set_media_player_visible,
    ),
    (
        "PictureInPicture",
        Browser::pip_enabled,
        Browser::set_pip_enabled,
    ),
    (
        "Transparent",
        |b| b.backdrop() == Backdrop::Acrylic,
        |b, on| {
            b.set_backdrop(if on {
                Backdrop::Acrylic
            } else {
                Backdrop::Mica
            })
        },
    ),
];

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    host: &Window,
) -> Result<Wired> {
    let weak = Rc::downgrade(browser);
    side_list(root, "SettingsCategories", &CATEGORIES)?;
    wire_downloads(root, browser, super::window_id(host)?)?;
    wire_clear_browsing_data(root, browser)?;
    super::site_permissions::wire(root, browser)?;
    let shortcuts = super::shortcut_settings::wire(root, browser, window, host)?;
    let sync = super::sync_settings::wire(root, browser, window)?;
    let mut follow: Follow = Vec::new();

    let tabs: ComboBox = xaml::find(root, "TabsPosition")?;
    let w = weak.clone();
    let show = choices(
        &tabs,
        &TAB_POSITIONS,
        browser.tabs_position(),
        move |position| {
            if let Some(b) = w.upgrade() {
                b.set_tabs_position(position);
            }
        },
    )?;
    follow.push(Box::new(move |b| show(b.tabs_position())));

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
    let show = choices(
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
    follow.push(Box::new(move |b| {
        if let Some(default) = b.core(|p| p.search_engines().default_engine().ok()) {
            show(default.id);
        }
    }));

    let startup: ComboBox = xaml::find(root, "Startup")?;
    let current = browser.core(|p| p.prefs().get(&keys::STARTUP));
    let w = weak.clone();
    let show = choices(&startup, &STARTUP, current, move |choice| {
        if let Some(b) = w.upgrade() {
            b.write_pref(&keys::STARTUP, &choice);
        }
    })?;
    follow.push(Box::new(move |b| {
        show(b.core(|p| p.prefs().get(&keys::STARTUP)));
    }));

    let tracking: ComboBox = xaml::find(root, "TrackingProtection")?;
    let description: TextBlock = xaml::find(root, "TrackingProtectionDescription")?;
    let levels = TrackingProtection::ALL.map(|level| (level, level.label()));
    let current = browser.core(|p| p.prefs().get(&keys::TRACKING_PROTECTION));
    description.SetText(current.description())?;
    let (w, described) = (weak.clone(), description.clone());
    let show = choices(&tracking, &levels, current, move |level| {
        let _ = described.SetText(level.description());
        if let Some(b) = w.upgrade() {
            b.write_pref(&keys::TRACKING_PROTECTION, &level);
        }
    })?;
    follow.push(Box::new(move |b| {
        let level = b.core(|p| p.prefs().get(&keys::TRACKING_PROTECTION));
        let _ = description.SetText(level.description());
        show(level);
    }));

    let theme: ComboBox = xaml::find(root, "Theme")?;
    let w = weak.clone();
    let show = choices(&theme, &THEMES, browser.theme(), move |theme| {
        if let Some(b) = w.upgrade() {
            b.set_theme(theme);
        }
    })?;
    follow.push(Box::new(move |b| show(b.theme())));

    for (name, pref, written) in PREF_SWITCHES {
        let current = move |b: &Browser| b.core(|p| p.prefs().get(pref));
        let set = move |b: &Browser, on| {
            b.write_pref(pref, &on);
            written(b);
        };
        follow.push(followed_switch(root, browser, name, current, set)?);
    }
    for (name, current, set) in WINDOW_SWITCHES {
        follow.push(followed_switch(root, browser, name, current, set)?);
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
    let (w, shown) = (weak.clone(), homepage.clone());
    let synced: Rc<Applied> = Rc::new(move |changed: &Changed| {
        let Some(b) = w.upgrade() else { return };
        if !changed.prefs.is_empty() || changed.search_engines {
            for show in &follow {
                show(&b);
            }
        }
        if changed.prefs.iter().any(|k| k == keys::HOMEPAGE.key) {
            let stored = b.core(|p| p.prefs().get(&keys::HOMEPAGE));
            let _ = shown.SetText(if stored == "about:home" { "" } else { &stored });
        }
    });
    browser.sync().on_applied(&synced);
    let w = weak;
    Ok(Wired {
        _alive: vec![
            default_browser,
            updates,
            shortcuts,
            sync.clone(),
            Rc::new(synced),
        ],
        on_close: Some(Box::new(move || {
            sync.server.apply();
            let Some(b) = w.upgrade() else { return };
            let text = homepage.Text().unwrap_or_default();
            match homepage_input(&text) {
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

/// A switch bound to a setting that a sync can change: set when the user flips it to what
/// `current` does not already say, and shown again by the returned `Follow` entry.
fn followed_switch(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    name: &str,
    current: impl Fn(&Browser) -> bool + Clone + 'static,
    set: impl Fn(&Browser, bool) + 'static,
) -> Result<Shown> {
    let read = current.clone();
    let on = current(browser);
    let switch = switch(root, browser, name, on, move |b, on| {
        if on != read(b) {
            set(b, on);
        }
    })?;
    Ok(Box::new(move |b| {
        let _ = switch.SetIsOn(current(b));
    }))
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
    let _ = choices(
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
/// Form entries stay, as with Chrome's defaults.
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

/// The Downloads group's folder, with Change and Reset. The folder picker opens over `owner`.
fn wire_downloads(root: &FrameworkElement, browser: &Rc<Browser>, owner: WindowId) -> Result<()> {
    let folder = Rc::new(DownloadFolder {
        path: xaml::find(root, "DownloadFolder")?,
        reset: xaml::find(root, "DownloadFolderReset")?,
    });
    folder.show(browser);

    let (b, f) = (Rc::downgrade(browser), folder.clone());
    on_click(
        &xaml::find::<Button>(root, "DownloadFolderChange")?,
        move || {
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

/// Fills `combo` with the labels of `options`, selects `current`, and calls `chosen` when the
/// user picks another. Returns what selects another value without calling `chosen`.
fn choices<T: Clone + PartialEq + 'static>(
    combo: &ComboBox,
    options: &[(T, &str)],
    current: T,
    chosen: impl Fn(T) + 'static,
) -> Result<impl Fn(T) + 'static> {
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
    let shown = Rc::new(std::cell::RefCell::new(current));
    let (source, seen, listed) = (selector.clone(), shown.clone(), values.clone());
    selector
        .SelectionChanged(move |_, _| {
            let index = super::selected_index(&source);
            if let Some(value) = index.and_then(|i| listed.get(i))
                && *value != *seen.borrow()
            {
                *seen.borrow_mut() = value.clone();
                chosen(value.clone());
            }
        })?
        .forget();
    Ok(move |value: T| {
        if let Some(index) = values.iter().position(|v| *v == value) {
            *shown.borrow_mut() = value;
            let _ = selector.SetSelectedIndex(i32::try_from(index).unwrap_or(-1));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_update_channel_has_a_label() {
        assert_eq!(CHANNELS.map(|(channel, _)| channel), UpdateChannel::ALL);
    }
}
