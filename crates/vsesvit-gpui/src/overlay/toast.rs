//! Owned by W2.1.
//! Toasts (UA section 1.7): plain text, never markup, at the bottom centre of the window, with an
//! optional button. One at a time; a new one replaces the last. It closes on its timeout or when its
//! button runs, and never takes the keyboard.

use std::time::Duration;

use gpui::{
    AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, point,
};
use gpui_component::ActiveTheme as _;

use super::{Anchor, Dismiss, WindowAction, DismissReason, OverlayHost, OverlayId, OverlayKind, OverlaySpec, Placement, Spot};
use crate::theme::tokens;

pub struct Toast {
    pub text: SharedString,
    /// A button's label and what it does; the toast closes after it runs.
    pub action: Option<(SharedString, WindowAction)>,
    pub timeout: Duration,
}

impl Toast {
    /// Chrome's toasts stay about this long.
    pub const TIMEOUT: Duration = Duration::from_secs(4);

    pub fn new(text: impl Into<SharedString>) -> Toast {
        Toast { text: text.into(), action: None, timeout: Toast::TIMEOUT }
    }
}

impl OverlayHost {
    pub fn toast(&mut self, toast: Toast, window: &mut Window, cx: &mut Context<Self>) -> OverlayId {
        if let Some(old) = self.open_overlays().into_iter().find(|o| o.kind == OverlayKind::Toast) {
            self.close_with(old.id, DismissReason::Replaced, window, cx);
        }
        let host = cx.entity().downgrade();
        let view = cx.new(|_| ToastView { text: toast.text, action: toast.action, host, me: None });
        let whole = Bounds { origin: point(gpui::px(0.), gpui::px(0.)), size: window.viewport_size() };
        let spec = OverlaySpec::new(OverlayKind::Toast, Anchor::Rect(whole), Placement::Inside { spot: Spot::BottomCentre, inset: tokens::TOAST_BOTTOM }, view.clone())
            .dismiss(Dismiss { timeout: Some(toast.timeout), ..Dismiss::OWNER });
        let id = self.open(spec, window, cx);
        view.update(cx, |view, _| view.me = Some(id));
        id
    }
}

struct ToastView {
    text: SharedString,
    action: Option<(SharedString, WindowAction)>,
    host: WeakEntity<OverlayHost>,
    me: Option<OverlayId>,
}

impl Render for ToastView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let button = self.action.clone().map(|(label, run)| {
            let (host, me) = (self.host.clone(), self.me);
            div()
                .id("toast-action")
                .px_2()
                .rounded(theme.radius)
                .text_color(theme.accent_foreground)
                .bg(theme.accent)
                .child(label)
                .on_click(move |_, window, cx| {
                    if let Some(me) = me {
                        let _ = host.update(cx, |host, cx| host.close(me, window, cx));
                    }
                    run(window, cx);
                })
        });
        div()
            .max_w(tokens::TOAST_MAX_W)
            .flex()
            .items_center()
            .gap_3()
            .py(tokens::TOAST_PADDING.top)
            .px(tokens::TOAST_PADDING.left)
            .rounded(tokens::TOAST_RADIUS)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .child(self.text.clone())
            .children(button)
    }
}
