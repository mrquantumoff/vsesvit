//! The Bookmarks, History, Downloads, Extensions, Settings and About dialogs and the welcome,
//! all on vsesvit-core data.
//!
//! Each dialog is a `ContentDialog` built from markup, filled and wired by its module. Scripted
//! runs never show the modal dialog (showing it moves keyboard focus); `preview` puts the same
//! wired content over the window instead, so it can be captured without taking focus.

mod about;
mod bookmarks;
mod default_browser;
mod downloads;
mod extensions;
mod history;
mod settings;
mod shortcut_settings;
mod site_permissions;
mod welcome;

use std::borrow::Cow;
use std::rc::Rc;

use vsesvit_core::prefs::Theme;
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::window::{Backdrop, BrowserWindow};
use crate::xaml;

pub(crate) use settings::CATEGORIES as SETTINGS_CATEGORIES;
#[cfg(feature = "self-test")]
pub(crate) use {
    bookmarks::import_bookmarks,
    default_browser::describe as describe_default_browser,
    shortcut_settings::{Page as ShortcutsPage, row_name as shortcut_row_name},
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
            Self::History => history::MARKUP.into(),
            Self::Downloads => downloads::MARKUP.into(),
            Self::Extensions => extensions::MARKUP.into(),
            Self::Settings => settings::MARKUP
                .replacen("{default_browser}", default_browser::MARKUP, 1)
                .replacen("{shortcuts}", shortcut_settings::PANEL, 1)
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
    let wired = match kind {
        Dialog::Bookmarks => bookmarks::wire(&root, &browser, window)?,
        Dialog::History => history::wire(&root, &browser, window)?,
        Dialog::Downloads => downloads::wire(&root, &browser)?,
        Dialog::Extensions => extensions::wire(&root, &browser, window)?,
        Dialog::Settings => settings::wire(&root, &browser, window)?,
        Dialog::About => about::fill(&root, &browser)?,
        Dialog::Welcome => welcome::wire(&root, &browser, window)?,
    };
    Ok(Built {
        dialog,
        kind,
        wired,
    })
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
    let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
    let dialog: ContentDialog = xaml::load(&format!(
        r#"<ContentDialog {{ns}} Title="{}" PrimaryButtonText="{}" CloseButtonText="Cancel"
             DefaultButton="Primary" Style="{{StaticResource DefaultContentDialogStyle}}">
  <TextBlock TextWrapping="Wrap" Text="{}"/>
</ContentDialog>"#,
        xaml::escape(title),
        xaml::escape(action),
        xaml::escape(text)
    ))?;
    dialog
        .cast::<UIElement>()?
        .SetXamlRoot(&window.xaml_root()?)?;
    dialog
        .cast::<FrameworkElement>()?
        .SetRequestedTheme(element_theme(browser.theme()))?;
    Ok(dialog.ShowAsync()?.await? == ContentDialogResult::Primary)
}

/// Wires a button's click.
pub(crate) fn on_click(button: &impl Interface, handler: impl Fn() + 'static) -> Result<()> {
    button
        .cast::<ButtonBase>()?
        .Click(move |_, _| handler())?
        .forget();
    Ok(())
}
