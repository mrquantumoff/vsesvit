//! Owned by W2.1.
//! Windows of their own (UA section 1.8, dialogs section 1.1): Settings, Extensions, Bookmarks,
//! History, Downloads, About, Keyboard Shortcuts and Manage Profiles. Each kind has one window at a
//! time (Downloads one per `Browsing`); asking for it again brings the open one forward instead of
//! opening another. Each closes with the browser window that opened it.

use gpui::{
    AnyWindowHandle, App, Bounds, Entity, Global, Pixels, Render, Size, TitlebarOptions, Window, WindowBounds, WindowKind,
    WindowOptions,
};
use vsesvit_core::private::Browsing;

use crate::platform::Desktop as _;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    Settings,
    Extensions,
    Bookmarks,
    History,
    Downloads(Browsing),
    About,
    Shortcuts,
    ManageProfiles,
}

impl LibraryKind {
    fn title(self) -> &'static str {
        match self {
            LibraryKind::Settings => "Settings",
            LibraryKind::Extensions => "Extensions",
            LibraryKind::Bookmarks => "Bookmarks",
            LibraryKind::History => "History",
            LibraryKind::Downloads(Browsing::Normal) => "Downloads",
            LibraryKind::Downloads(Browsing::Private) => "Private Downloads",
            LibraryKind::About => "About Vsesvit",
            LibraryKind::Shortcuts => "Keyboard Shortcuts",
            LibraryKind::ManageProfiles => "Manage Profiles",
        }
    }

    /// About, Keyboard Shortcuts and Manage Profiles float over their opener (UA section 1.8).
    fn kind(self) -> WindowKind {
        match self {
            LibraryKind::About | LibraryKind::Shortcuts | LibraryKind::ManageProfiles => WindowKind::Floating,
            _ => WindowKind::Normal,
        }
    }
}

#[derive(Default)]
struct Library(Vec<(LibraryKind, AnyWindowHandle)>);

impl Global for Library {}

/// The window of `kind`: the open one brought forward, or a new one of `size` made by `build`, which
/// closes with `opener`. `None` when GPUI refused the window.
pub fn present_library<V: Render>(
    opener: AnyWindowHandle,
    kind: LibraryKind,
    size: Size<Pixels>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    if let Some(open) = open_window(kind, cx) {
        let desktop = cx.try_global::<crate::boot::start::Booted>().map(|b| b.desktop.clone());
        let _ = open.update(cx, |_, window, _| {
            if let Some(desktop) = desktop {
                desktop.raise_for_invocation(window, None);
            }
        });
        return Some(open);
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
        titlebar: Some(TitlebarOptions { title: Some(kind.title().into()), ..Default::default() }),
        kind: kind.kind(),
        app_id: Some(crate::window::frame::APP_ID.to_owned()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        crate::theme::follow_desktop(window);
        build(window, cx)
    });
    match opened {
        Ok(handle) => {
            let handle: AnyWindowHandle = handle.into();
            cx.default_global::<Library>().0.push((kind, handle));
            super::host::close_with(handle, opener, cx);
            Some(handle)
        }
        Err(e) => {
            log::error!("dialogs: GPUI did not open {}: {e}", kind.title());
            None
        }
    }
}

/// The window of `kind`, if one is open.
pub fn open_window(kind: LibraryKind, cx: &mut App) -> Option<AnyWindowHandle> {
    let handle = cx.try_global::<Library>()?.0.iter().find(|(k, _)| *k == kind).map(|&(_, h)| h)?;
    if handle.update(cx, |_, _, _| ()).is_ok() {
        return Some(handle);
    }
    cx.global_mut::<Library>().0.retain(|(k, _)| *k != kind);
    None
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, Context, IntoElement, TestAppContext, div, px, size};

    use super::*;

    struct Plain;

    impl Render for Plain {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn browser_window(cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(|cx| cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Plain)).unwrap().into())
    }

    fn present(cx: &mut TestAppContext, opener: AnyWindowHandle, kind: LibraryKind) -> AnyWindowHandle {
        let handle = cx.update(|cx| present_library(opener, kind, size(px(640.), px(680.)), |_, cx| cx.new(|_| Plain), cx)).unwrap();
        cx.run_until_parked();
        handle
    }

    #[gpui::test]
    fn a_window_of_its_own_opens_once_and_closes_with_its_browser_window(cx: &mut TestAppContext) {
        cx.update(crate::theme::init);
        let browser = browser_window(cx);
        let history = present(cx, browser, LibraryKind::History);
        assert_eq!(present(cx, browser, LibraryKind::History), history, "asked again, the same window");
        assert_eq!(cx.windows().len(), 2);
        let normal = present(cx, browser, LibraryKind::Downloads(Browsing::Normal));
        let private = present(cx, browser, LibraryKind::Downloads(Browsing::Private));
        assert_ne!(normal, private, "private downloads have their own window");
        assert_eq!(cx.windows().len(), 4);

        history.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        let again = present(cx, browser, LibraryKind::History);
        assert_ne!(again, history, "closed, it opens anew");

        browser.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0, "every window it opened went with it");
        cx.update(|cx| assert!(open_window(LibraryKind::History, cx).is_none()));
    }
}
