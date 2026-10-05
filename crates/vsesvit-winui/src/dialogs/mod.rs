//! The Bookmarks, History, Downloads, Extensions, Settings and About dialogs and the welcome,
//! all on vsesvit-core data.
//!
//! Each dialog's content is built from markup, filled and wired by its module. Bookmarks,
//! History, Downloads and Settings show it in a window of their own (`windowed`); the others in
//! a `ContentDialog` over the browser window. Scripted runs show neither (showing them moves
//! keyboard focus); `preview` puts the same wired content over the browser window instead, so it
//! can be captured without taking focus.

mod about;
mod bookmarks;
mod default_browser;
mod downloads;
mod extensions;
mod history;
mod other_devices;
pub(crate) mod search_engines;
mod settings;
mod shortcut_settings;
mod site_permissions;
mod sync_settings;
mod welcome;
mod windowed;

use std::borrow::Cow;
use std::rc::Rc;

use vsesvit_core::prefs::Theme;
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::browser::Browser;
use crate::window::{Backdrop, BrowserWindow};
use crate::xaml;

pub(crate) use settings::CATEGORIES as SETTINGS_CATEGORIES;
pub(crate) use windowed::DialogWindow;
#[cfg(feature = "self-test")]
pub(crate) use {
    bookmarks::import_bookmarks,
    default_browser::describe as describe_default_browser,
    shortcut_settings::{
        Page as ShortcutsPage, extension_row_name as extension_shortcut_row_name,
        row_name as shortcut_row_name,
    },
    welcome::{PAGES as WELCOME_PAGES, Page as WelcomePage},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialog {
    Bookmarks,
    History,
    Downloads,
    Extensions,
    Settings,
    About,
    Welcome,
}

impl Dialog {
    /// Shown in a window of its own rather than over the browser window.
    pub fn windowed(self) -> bool {
        self.window_size().is_some()
    }

    /// The size of its own window, in view pixels, for the dialogs that have one.
    fn window_size(self) -> Option<(f64, f64)> {
        match self {
            Self::Bookmarks => Some((900.0, 640.0)),
            Self::History => Some((1000.0, 680.0)),
            Self::Downloads => Some((720.0, 600.0)),
            Self::Settings => Some((960.0, 720.0)),
            Self::Extensions | Self::About | Self::Welcome => None,
        }
    }

    /// The title over the dialog. The welcome has none: its pages have headings.
    fn heading(self) -> Option<&'static str> {
        match self {
            Self::Bookmarks => Some("Bookmarks"),
            Self::History => Some("History"),
            Self::Downloads => Some("Downloads"),
            Self::Extensions => Some("Extensions"),
            Self::Settings => Some("Settings"),
            Self::About => Some("About Vsesvit"),
            Self::Welcome => None,
        }
    }

    fn body(self) -> Cow<'static, str> {
        match self {
            Self::Bookmarks => bookmarks::MARKUP.into(),
            Self::History => history::MARKUP
                .replacen("{other_devices}", other_devices::PANEL, 1)
                .into(),
            Self::Downloads => downloads::MARKUP.into(),
            Self::Extensions => extensions::MARKUP.into(),
            Self::Settings => settings::MARKUP
                .replacen("{default_browser}", default_browser::MARKUP, 1)
                .replacen("{search_engines}", search_engines::MARKUP, 1)
                .replacen("{shortcuts}", shortcut_settings::PANEL, 1)
                .replacen("{sync}", &sync_settings::panel(), 1)
                .into(),
            Self::About => about::MARKUP.into(),
            Self::Welcome => welcome::MARKUP.into(),
        }
    }

    /// The title and the Close button, as attributes. The welcome's own buttons go from page to
    /// page and close it on the last.
    fn head(self) -> String {
        match self.heading() {
            Some(title) => {
                format!(r#"Title="{title}" CloseButtonText="Close" DefaultButton="Close""#)
            }
            None => String::new(),
        }
    }
}

const DIALOG_OPEN: &str = r#"<ContentDialog {ns} HEAD
    Style="{StaticResource DefaultContentDialogStyle}"BACKGROUND>
  <ContentDialog.Resources>
    <x:Double x:Key="ContentDialogMaxWidth">900</x:Double>
    <x:Double x:Key="ContentDialogMaxHeight">800</x:Double>
  </ContentDialog.Resources>"#;

/// With the transparent window, dialogs are acrylic too: a blur of the window under them.
fn markup(dialog: Dialog, backdrop: Backdrop) -> String {
    let background = match backdrop {
        Backdrop::Mica => "",
        Backdrop::Acrylic => r#" Background="{ThemeResource AcrylicInAppFillColorDefaultBrush}""#,
    };
    format!(
        "{}{}\n</ContentDialog>",
        DIALOG_OPEN
            .replacen("HEAD", &dialog.head(), 1)
            .replacen("BACKGROUND", background, 1),
        dialog.body()
    )
}

/// What a dialog's wiring needs to keep alive while it is open, and what it does on close.
#[derive(Default)]
pub(crate) struct Wired {
    pub _alive: Vec<Rc<dyn std::any::Any>>,
    pub on_close: Option<Box<dyn FnOnce()>>,
}

/// A built dialog, filled and wired.
pub(crate) struct Built {
    dialog: ContentDialog,
    kind: Dialog,
    wired: Wired,
}

/// Shows `dialog` over `window` until the user closes it.
pub(crate) async fn show(window: &Rc<BrowserWindow>, dialog: Dialog) -> Result<()> {
    let built = build(window, dialog)?;
    built.dialog.ShowAsync()?.await?;
    if let Some(on_close) = built.wired.on_close {
        on_close();
    }
    Ok(())
}

pub(crate) fn build(window: &Rc<BrowserWindow>, kind: Dialog) -> Result<Built> {
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let dialog: ContentDialog = xaml::load(&markup(kind, browser.backdrop()))?;
    dialog
        .cast::<UIElement>()?
        .SetXamlRoot(&window.xaml_root()?)?;
    dialog
        .cast::<FrameworkElement>()?
        .SetRequestedTheme(element_theme(browser.theme()))?;
    let root = dialog.cast::<FrameworkElement>()?;
    let wired = wire(kind, &root, &browser, window, window.xaml_window())?;
    Ok(Built {
        dialog,
        kind,
        wired,
    })
}

/// Fills and wires `kind`'s content under `root`. `window` is the browser window it acts on, and
/// `host` the window it shows in: its own, or `window` itself.
fn wire(
    kind: Dialog,
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    host: &Window,
) -> Result<Wired> {
    match kind {
        Dialog::Bookmarks => bookmarks::wire(root, browser, host),
        Dialog::History => history::wire(root, browser, window),
        Dialog::Downloads => downloads::wire(root, browser),
        Dialog::Extensions => extensions::wire(root, browser, window),
        Dialog::Settings => settings::wire(root, browser, window, host),
        Dialog::About => about::fill(root, browser),
        Dialog::Welcome => welcome::wire(root, browser, window),
    }
}

/// The id pickers opened over `window` take as their owner.
fn window_id(window: &Window) -> Result<WindowId> {
    window.cast::<IWindow2>()?.AppWindow()?.Id()
}

pub(super) fn element_theme(theme: Theme) -> ElementTheme {
    match theme {
        Theme::System => ElementTheme::Default,
        Theme::Light => ElementTheme::Light,
        Theme::Dark => ElementTheme::Dark,
    }
}

/// A dialog's content shown over the window without the modal dialog; removed on drop.
pub(crate) struct Preview {
    window: Rc<BrowserWindow>,
    body: UIElement,
    _built: Built,
}

impl Preview {
    pub fn kind(&self) -> Dialog {
        self._built.kind
    }

    /// What the dialog's wiring keeps alive of type `T`, such as a page's state.
    pub fn wired<T: 'static>(&self) -> Option<Rc<T>> {
        self._built
            .wired
            ._alive
            .iter()
            .find_map(|alive| alive.clone().downcast::<T>().ok())
    }

    /// A named element of the dialog's content (or of a row inside it), once laid out.
    pub fn find<T: Interface>(&self, name: &str) -> Result<T> {
        xaml::find_named(&self.body.cast()?, name)
            .ok_or_else(|| windows_core::Error::new(E_FAIL, format!("no element {name:?}")))
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Err(e) = self.window.set_overlay(None) {
            log::warn!("removing a dialog preview: {e}");
        }
    }
}

pub(crate) fn preview(window: &Rc<BrowserWindow>, kind: Dialog) -> Result<Preview> {
    let built = build(window, kind)?;
    let content = built.dialog.cast::<IContentControl>()?;
    let body: UIElement = content.Content()?.cast()?;
    content.SetContent(None::<&IInspectable>)?;
    if let Some((width, height)) = kind.window_size() {
        // Its own window would give the content this room, less the title bar and the margins.
        let body = body.cast::<FrameworkElement>()?;
        body.SetWidth(width - 2.0 * windowed::MARGIN)?;
        body.SetHeight(height - windowed::TITLE_BAR_HEIGHT - windowed::MARGIN)?;
    }
    window.set_overlay(Some((built.kind.heading().unwrap_or_default(), &body)))?;
    Ok(Preview {
        window: window.clone(),
        body,
        _built: built,
    })
}

/// Asks the user to confirm `action` over `window`; true if they chose it.
pub(crate) async fn confirm(
    window: &Rc<BrowserWindow>,
    title: &str,
    text: &str,
    action: &str,
) -> Result<bool> {
    ask(window, &confirm_markup(title, text, action, "Primary")).await
}

/// `confirm`, for something a page asked for: Cancel is the default button, so Enter, which the
/// page can time, does not accept it.
pub(crate) async fn confirm_for_page(
    window: &Rc<BrowserWindow>,
    title: &str,
    text: &str,
    action: &str,
) -> Result<bool> {
    ask(window, &confirm_markup(title, text, action, "Close")).await
}

/// `default` is the `DefaultButton`: `Primary` (`action`) or `Close` (Cancel).
fn confirm_markup(title: &str, text: &str, action: &str, default: &str) -> String {
    format!(
        r#"<ContentDialog {{ns}} Title="{}" PrimaryButtonText="{}" CloseButtonText="Cancel"
             DefaultButton="{default}" Style="{{StaticResource DefaultContentDialogStyle}}">
  <TextBlock TextWrapping="Wrap" Text="{}"/>
</ContentDialog>"#,
        xaml::escape(title),
        xaml::escape(action),
        xaml::escape(text)
    )
}

async fn ask(window: &Rc<BrowserWindow>, markup: &str) -> Result<bool> {
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let dialog: ContentDialog = xaml::load(markup)?;
    dialog
        .cast::<UIElement>()?
        .SetXamlRoot(&window.xaml_root()?)?;
    dialog
        .cast::<FrameworkElement>()?
        .SetRequestedTheme(element_theme(browser.theme()))?;
    Ok(dialog.ShowAsync()?.await? == ContentDialogResult::Primary)
}

/// An entry down the side of a dialog and the panel it shows.
pub(crate) struct Category {
    pub label: &'static str,
    /// Segoe Fluent Icons.
    pub glyph: &'static str,
    /// The `x:Name` of its panel in the dialog's markup.
    pub panel: &'static str,
}

/// Fills the list `list_name` down the side of a dialog from `entries` and shows the selected
/// entry's panel.
fn side_list(root: &FrameworkElement, list_name: &str, entries: &[Category]) -> Result<()> {
    let list: ListView = xaml::find(root, list_name)?;
    let items = list.cast::<ItemsControl>()?.Items()?;
    let mut panels = Vec::new();
    for entry in entries {
        let item: UIElement = xaml::load(&format!(
            r#"<StackPanel {{ns}} Orientation="Horizontal" Spacing="12">
  <FontIcon Glyph="{}" FontSize="16"/>
  <TextBlock Text="{}" VerticalAlignment="Center"/>
</StackPanel>"#,
            entry.glyph,
            xaml::escape(entry.label)
        ))?;
        items.Append(&item)?;
        panels.push(xaml::find::<UIElement>(root, entry.panel)?);
    }
    let selector = list.cast::<Selector>()?;
    let source = selector.clone();
    selector
        .SelectionChanged(move |_, _| {
            // Ctrl+click can leave nothing selected; the panel shown stays.
            let Some(selected) = selected_index(&source) else {
                return;
            };
            for (index, panel) in panels.iter().enumerate() {
                let _ = xaml::set_visible(panel, index == selected);
            }
        })?
        .forget();
    selector.SetSelectedIndex(0)
}

/// The selected row of a `Selector` (a ComboBox, a ListView, ...); `None` when nothing is.
pub(crate) fn selected_index(selector: &impl Interface) -> Option<usize> {
    let index = selector
        .cast::<Selector>()
        .and_then(|s| s.SelectedIndex())
        .ok()?;
    usize::try_from(index).ok()
}

/// Fills `bar` to `fraction` (0 to 1), or runs it indeterminate while that is unknown.
fn set_progress(bar: &ProgressBar, fraction: Option<f64>) -> Result<()> {
    let Some(fraction) = fraction else {
        return bar.SetIsIndeterminate(true);
    };
    bar.SetIsIndeterminate(false)?;
    let range = bar.cast::<RangeBase>()?;
    range.SetMaximum(1.0)?;
    range.SetValue(fraction)
}

/// Wires a button's click.
pub(crate) fn on_click(button: &impl Interface, handler: impl Fn() + 'static) -> Result<()> {
    button
        .cast::<ButtonBase>()?
        .Click(move |_, _| handler())?
        .forget();
    Ok(())
}
