//! Owned by W2.1.
//! Dialog windows (UA section 1.8): a prompt raised by a window opens as a GPUI `Dialog` window over
//! it, centred on it, made its child natively (`Desktop::set_dialog_parent`: `WM_TRANSIENT_FOR` on
//! X11, the owner on Windows; GPUI picks no parent while the page has the keyboard), and closed when
//! it closes. Feature code writes a dialog's view once and hands it to [`present`].

use gpui::{
    AnyWindowHandle, App, Bounds, Entity, Global, Pixels, Render, SharedString, Size, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, point,
};

use crate::platform::Desktop as _;

/// A dialog window to open.
pub struct DialogSpec {
    pub title: SharedString,
    pub size: Size<Pixels>,
}

/// Opens a dialog window over `parent` with the view `build` makes. `None` when the parent is gone or
/// GPUI refused the window.
pub fn present<V: Render>(
    parent: AnyWindowHandle,
    spec: DialogSpec,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    cx: &mut App,
) -> Option<WindowHandle<V>> {
    let opened = parent.update(cx, |_, parent_window, cx| {
        let centre = parent_window.bounds().center();
        let origin = point(centre.x - spec.size.width / 2., centre.y - spec.size.height / 2.);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size: spec.size })),
            titlebar: Some(TitlebarOptions { title: Some(spec.title.clone()), ..Default::default() }),
            kind: WindowKind::Dialog,
            is_resizable: false,
            app_id: Some(crate::window::frame::APP_ID.to_owned()),
            ..Default::default()
        };
        cx.open_window(options, |dialog, cx| {
            if let Some(booted) = cx.try_global::<crate::boot::start::Booted>() {
                booted.desktop.set_dialog_parent(dialog, parent_window);
            }
            crate::theme::follow_desktop(dialog);
            build(dialog, cx)
        })
    });
    match opened {
        Ok(Ok(handle)) => {
            close_with(handle.into(), parent, cx);
            Some(handle)
        }
        Ok(Err(e)) => {
            log::error!("dialogs: GPUI did not open \"{}\": {e}", spec.title);
            None
        }
        Err(_) => None,
    }
}

/// Windows that close when another does: dialogs and library windows with their opener.
struct Children {
    pairs: Vec<(gpui::WindowId, AnyWindowHandle)>,
    _closed: Subscription,
}

impl Global for Children {}

/// `child` closes when `parent` does.
pub fn close_with(child: AnyWindowHandle, parent: AnyWindowHandle, cx: &mut App) {
    if !cx.has_global::<Children>() {
        let closed = cx.on_window_closed(|cx, gone| {
            let children = &mut cx.global_mut::<Children>().pairs;
            children.retain(|(_, child)| child.window_id() != gone);
            let doomed: Vec<AnyWindowHandle> = children.iter().filter(|(parent, _)| *parent == gone).map(|&(_, child)| child).collect();
            children.retain(|(parent, _)| *parent != gone);
            for child in doomed {
                let _ = child.update(cx, |_, window, _| window.remove_window());
            }
        });
        cx.set_global(Children { pairs: Vec::new(), _closed: closed });
    }
    cx.global_mut::<Children>().pairs.push((parent.window_id(), child));
}
