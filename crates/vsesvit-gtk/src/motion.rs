//! The timing of the shell's own animations, so they move alike. Each one runs through
//! `AdwAnimation` or `GtkRevealer`, which skip to the end when the widget is not mapped or
//! the desktop turns animations off (`gtk-enable-animations`).

use adw::prelude::*;
use gtk::glib;

/// A tab row growing into or shrinking out of the tab list, in milliseconds.
pub(crate) const TAB_ROW_MS: u32 = 180;
pub(crate) const EASING: adw::Easing = adw::Easing::EaseOutCubic;

/// Fades `widget` from its current opacity to `to` over `duration_ms`, then runs `done`.
/// The caller keeps the animation for as long as it should run.
pub(crate) fn fade(
    widget: &impl IsA<gtk::Widget>,
    to: f64,
    duration_ms: u32,
    done: impl Fn() + 'static,
) -> adw::TimedAnimation {
    let widget = widget.upcast_ref::<gtk::Widget>();
    let target = adw::CallbackAnimationTarget::new(glib::clone!(
        #[weak]
        widget,
        move |value| widget.set_opacity(value)
    ));
    let animation = adw::TimedAnimation::builder()
        .widget(widget)
        .value_from(widget.opacity())
        .value_to(to)
        .duration(duration_ms)
        .easing(EASING)
        .target(&target)
        .build();
    animation.connect_done(move |_| done());
    animation
}
