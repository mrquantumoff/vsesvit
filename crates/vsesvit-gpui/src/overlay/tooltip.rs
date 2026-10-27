//! Owned by W2.1.
//! Tooltips that may cover the page (UA section 1.7): GPUI's own `.tooltip()` draws inside the window
//! but under the page on X11, so a tooltip that can reach the page's rectangle goes through the host.
//! One shows at a time, under its anchor; it is paint only and never takes the keyboard.

use gpui::{AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled as _, Window, div};
use gpui_component::ActiveTheme as _;

use super::{Align, Anchor, Dismiss, OverlayHost, OverlayId, OverlayKind, OverlaySpec, Placement};
use crate::theme::tokens;

impl OverlayHost {
    /// Shows `text` under `anchor` (a widget's bounds, window pixels), replacing any tooltip shown.
    pub fn show_tooltip(&mut self, text: impl Into<SharedString>, anchor: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> OverlayId {
        self.close_kind(OverlayKind::Tooltip, window, cx);
        let view = cx.new(|_| TooltipView { text: text.into() });
        let spec = OverlaySpec::new(OverlayKind::Tooltip, Anchor::Rect(anchor), Placement::Below(Align::Centre), view)
            .dismiss(Dismiss { outside_press: true, window_change: true, ..Dismiss::OWNER })
            .pass_through();
        self.open(spec, window, cx)
    }

    /// The pointer left the anchor.
    pub fn hide_tooltip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_kind(OverlayKind::Tooltip, window, cx);
    }
}

struct TooltipView {
    text: SharedString,
}

impl Render for TooltipView {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .text_size(tokens::TEXT_CAPTION)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(self.text.clone())
    }
}
