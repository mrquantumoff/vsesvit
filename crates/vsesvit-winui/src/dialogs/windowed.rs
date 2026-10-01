//! Bookmarks, History, Downloads and Settings in windows of their own, one of each at a time.
//! Each belongs to the browser window it was opened from: what it opens goes there, and it
//! closes with it.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use vsesvit_core::prefs::Theme;
use windows_core::{Interface, Result};

use super::{Dialog, Wired};
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::{self, Backdrop, BrowserWindow};
use crate::{platform, xaml};

/// The title bar's height, which the caption buttons' standard height sets.
pub(super) const TITLE_BAR_HEIGHT: f64 = 32.0;

/// Between the content and the window's edges.
pub(super) const MARGIN: f64 = 24.0;

/// Smaller than this, the content's side panes squeeze its lists to nothing.
const MIN_SIZE: (f64, f64) = (560.0, 420.0);

const WINDOW_XAML: &str = r#"<Grid {ns}>
  <Grid.RowDefinitions><RowDefinition Height="32"/><RowDefinition Height="*"/></Grid.RowDefinitions>
  <Grid x:Name="DialogTitleBar" Background="Transparent">
    <TextBlock x:Name="DialogTitle" Margin="16,0,0,0" VerticalAlignment="Center"
               Style="{StaticResource CaptionTextBlockStyle}"/>
  </Grid>
  <Grid Grid.Row="1" Padding="24,0,24,24">BODY</Grid>
</Grid>"#;

pub(crate) struct DialogWindow {
    kind: Dialog,
    window: Window,
    root: FrameworkElement,
    browser: Weak<Browser>,
    opener: Weak<BrowserWindow>,
    /// Taken when the window closes.
    wired: RefCell<Option<Wired>>,
}

impl DialogWindow {
    pub fn open(
        browser: &Rc<Browser>,
        opener: &Rc<BrowserWindow>,
        kind: Dialog,
    ) -> Result<Rc<Self>> {
        let (width, height) = kind.window_size().ok_or_else(windows_core::Error::empty)?;
        let title = kind.heading().unwrap_or_default();
        let root: FrameworkElement = xaml::load(&WINDOW_XAML.replacen("BODY", &kind.body(), 1))?;
        let window = Window::new()?;
        window.SetTitle(title)?;
        window.SetContent(&root)?;
        window.SetExtendsContentIntoTitleBar(true)?;
        window.SetTitleBar(&xaml::find::<UIElement>(&root, "DialogTitleBar")?)?;
        xaml::find::<TextBlock>(&root, "DialogTitle")?.SetText(title)?;
        let hwnd = platform::window_handle(&window)?;
        platform::set_window_icon(hwnd);
        window::set_backdrop(&window, browser.backdrop());
        window::set_theme(&window, &root, browser.theme());
        let wired = super::wire(kind, &root, browser, opener, &window)?;

        let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0;
        let pixels = |view: f64| (view * scale) as i32;
        let app = window.cast::<IWindow2>()?.AppWindow()?;
        app.Resize(SizeInt32 {
            width: pixels(width),
            height: pixels(height),
        })?;
        let minimum = app
            .Presenter()?
            .cast::<IOverlappedPresenter3>()
            .and_then(|p| {
                p.SetPreferredMinimumWidth(Some(pixels(MIN_SIZE.0)))?;
                p.SetPreferredMinimumHeight(Some(pixels(MIN_SIZE.1)))
            });
        if let Err(e) = minimum {
            log::debug!("{kind:?} window's minimum size: {e}");
        }

        let this = Rc::new(Self {
            kind,
            window,
            root,
            browser: Rc::downgrade(browser),
            opener: Rc::downgrade(opener),
            wired: RefCell::new(Some(wired)),
        });
        let me = Rc::downgrade(&this);
        this.window
            .Closed(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.closed();
                }
            })?
            .forget();
        this.window.Activate()?;
        Ok(this)
    }

    pub fn kind(&self) -> Dialog {
        self.kind
    }

    pub fn opened_from(&self, window: &BrowserWindow) -> bool {
        std::ptr::eq(self.opener.as_ptr(), window)
    }

    /// Brings the window forward, restored if it was minimized.
    pub fn raise(&self) {
        let restored = self
            .window
            .cast::<IWindow2>()
            .and_then(|w| w.AppWindow())
            .and_then(|a| a.Presenter())
            .and_then(|p| p.cast::<OverlappedPresenter>())
            .and_then(|p| {
                if p.State()? == OverlappedPresenterState::Minimized {
                    p.Restore()?;
                }
                Ok(())
            });
        if let Err(e) = restored {
            log::debug!("restoring the {:?} window: {e}", self.kind);
        }
        if let Err(e) = self.window.Activate() {
            log::warn!("activating the {:?} window: {e}", self.kind);
        }
    }

    pub fn close(&self) {
        if let Err(e) = self.window.Close() {
            log::warn!("closing the {:?} window: {e}", self.kind);
        }
    }

    pub fn apply_theme(&self, theme: Theme) {
        window::set_theme(&self.window, &self.root, theme);
    }

    pub fn apply_backdrop(&self, backdrop: Backdrop) {
        window::set_backdrop(&self.window, backdrop);
    }

    fn closed(&self) {
        let Some(wired) = self.wired.take() else {
            return;
        };
        if let Some(on_close) = wired.on_close {
            on_close();
        }
        // As for the browser window: release the content while the window still exists.
        let _ = self.window.SetContent(None::<&UIElement>);
        drop(wired._alive);
        if let Some(browser) = self.browser.upgrade() {
            browser.dialog_window_closed(self);
        }
    }
}
