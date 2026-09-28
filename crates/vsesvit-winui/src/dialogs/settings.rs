//! Settings, bound to vsesvit-core preferences. Every choice applies at once, in every window;
//! the home page is written when the dialog closes.

use std::rc::Rc;

use vsesvit_core::prefs::{Startup, TabsPosition, Theme, keys};
use vsesvit_core::search::{SearchEngineId, classify_url};
use windows_core::{Interface, Result};

use super::Wired;
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::Backdrop;
use crate::xaml;

pub(super) const MARKUP: &str = r#"
  <StackPanel Width="480" Spacing="16">
    <ComboBox x:Name="TabsPosition" Header="Tabs" MinWidth="320"/>
    <ComboBox x:Name="SearchEngine" Header="Search engine used in the address bar" MinWidth="320"/>
    <ComboBox x:Name="Startup" Header="On startup" MinWidth="320"/>
    <TextBox x:Name="Homepage" Header="Home page" PlaceholderText="https://"/>
    <ComboBox x:Name="Theme" Header="Theme" MinWidth="320"/>
    <ToggleSwitch x:Name="ShowBookmarksBar" Header="Show the bookmarks bar"/>
    <ToggleSwitch x:Name="CompactAddress" Header="Compact address bar, centered in the toolbar"/>
    <StackPanel Spacing="4">
      <ToggleSwitch x:Name="FullUrls" Header="Always show full URLs"/>
      <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                 Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                 Text="When off, the address bar leaves out https://, www. and a trailing slash until you click into it."/>
    </StackPanel>
    <StackPanel Spacing="4">
      <ToggleSwitch x:Name="Transparent" Header="Transparent window"/>
      <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                 Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                 Text="Shows a blur of the windows behind Vsesvit (acrylic). When off, the window is tinted by your desktop background (Mica), like other Windows 11 apps."/>
    </StackPanel>
    <StackPanel Spacing="4">
      <ToggleSwitch x:Name="UpdatesAutomatic" Header="Download and install updates automatically"/>
      <TextBlock x:Name="UpdatesUnavailable" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                 Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                 Text="This copy of Vsesvit was not installed with the Vsesvit installer, so it does not update itself."/>
    </StackPanel>
    <TextBlock x:Name="ProfilePath" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}" IsTextSelectionEnabled="True"/>
  </StackPanel>"#;

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

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<Wired> {
    let weak = Rc::downgrade(browser);

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

    let bar: ToggleSwitch = xaml::find(root, "ShowBookmarksBar")?;
    bar.SetIsOn(browser.bookmarks_bar_visible())?;
    let w = weak.clone();
    let source = bar.clone();
    bar.Toggled(move |_, _| {
        if let (Some(b), Ok(on)) = (w.upgrade(), source.IsOn()) {
            b.set_bookmarks_bar_visible(on);
        }
    })?
    .forget();

    let compact: ToggleSwitch = xaml::find(root, "CompactAddress")?;
    compact.SetIsOn(browser.compact_address())?;
    let w = weak.clone();
    let source = compact.clone();
    compact
        .Toggled(move |_, _| {
            if let (Some(b), Ok(on)) = (w.upgrade(), source.IsOn()) {
                b.set_compact_address(on);
            }
        })?
        .forget();

    let full_urls: ToggleSwitch = xaml::find(root, "FullUrls")?;
    full_urls.SetIsOn(browser.full_urls())?;
    let w = weak.clone();
    let source = full_urls.clone();
    full_urls
        .Toggled(move |_, _| {
            if let (Some(b), Ok(on)) = (w.upgrade(), source.IsOn()) {
                b.set_full_urls(on);
            }
        })?
        .forget();

    let transparent: ToggleSwitch = xaml::find(root, "Transparent")?;
    transparent.SetIsOn(browser.backdrop() == Backdrop::Acrylic)?;
    let w = weak.clone();
    let source = transparent.clone();
    transparent
        .Toggled(move |_, _| {
            if let (Some(b), Ok(on)) = (w.upgrade(), source.IsOn()) {
                b.set_backdrop(if on {
                    Backdrop::Acrylic
                } else {
                    Backdrop::Mica
                });
            }
        })?
        .forget();

    let updates: ToggleSwitch = xaml::find(root, "UpdatesAutomatic")?;
    let unavailable: UIElement = xaml::find(root, "UpdatesUnavailable")?;
    let self_updating = !browser.updates().is_disabled();
    updates.SetIsOn(self_updating && browser.updates_automatic())?;
    updates.cast::<Control>()?.SetIsEnabled(self_updating)?;
    xaml::set_visible(&unavailable, !self_updating)?;
    let w = weak.clone();
    let source = updates.clone();
    updates
        .Toggled(move |_, _| {
            if let (Some(b), Ok(on)) = (w.upgrade(), source.IsOn()) {
                b.set_updates_automatic(on);
            }
        })?
        .forget();

    let homepage: TextBox = xaml::find(root, "Homepage")?;
    let stored = browser.core(|p| p.prefs().get(&keys::HOMEPAGE));
    homepage.SetText(if stored == "about:home" { "" } else { &stored })?;
    let path: TextBlock = xaml::find(root, "ProfilePath")?;
    path.SetText(&format!(
        "Profile folder: {}",
        browser.profile_dir().display()
    ))?;

    let w = weak;
    Ok(Wired {
        _alive: Vec::new(),
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
    fn homepage_is_empty_or_an_address() {
        assert_eq!(homepage_value("  "), Some(HomepageValue::Default));
        assert_eq!(
            homepage_value("example.com"),
            Some(HomepageValue::Url("https://example.com/".into()))
        );
        assert_eq!(homepage_value("not an address"), None);
    }
}
