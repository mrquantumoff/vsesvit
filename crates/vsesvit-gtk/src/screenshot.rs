//! In-app capture of a widget, normally a whole browser window, to a PNG.
//!
//! The widget's rendering is replayed through its window's own GSK renderer, so nothing outside
//! the app is read and the window needs neither focus nor to be on top.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use gtk::{gdk, gio, glib, graphene, prelude::*};

#[derive(Debug)]
pub enum CaptureError {
    /// The widget is not in a realized window or has no size yet.
    NotRealized,
    /// The widget has no current rendering: it was never drawn, or a redraw is pending.
    NothingDrawn,
    Save(glib::BoolError),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::NotRealized => write!(f, "the widget is not shown in a window"),
            CaptureError::NothingDrawn => write!(f, "the widget has no current rendering"),
            CaptureError::Save(e) => write!(f, "could not write the PNG: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Renders `widget` as it was last drawn, at the display's scale.
///
/// GTK drops a widget's rendering as soon as it is invalidated, so this fails with
/// [`CaptureError::NothingDrawn`] between an invalidation and the next frame, which is most of
/// the time while a page loads or animates. [`capture_next_frame`] does not have that problem.
pub fn capture(widget: &impl IsA<gtk::Widget>) -> Result<gdk::Texture, CaptureError> {
    let widget = widget.as_ref();
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .ok_or(CaptureError::NotRealized)?;
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let (width, height) = (paintable.intrinsic_width(), paintable.intrinsic_height());
    if width <= 0 || height <= 0 {
        return Err(CaptureError::NotRealized);
    }
    let scale = widget.scale_factor() as f32;
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(scale, scale);
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot.to_node().ok_or(CaptureError::NothingDrawn)?;
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32 * scale, height as f32 * scale);
    Ok(renderer.render_texture(&node, Some(&viewport)))
}

/// Captures `widget` right after its window paints the next frame, while its rendering is
/// current. Frames only come while the compositor shows the window, so bound the wait (for
/// example with `glib::future_with_timeout`).
pub async fn capture_next_frame(
    widget: &impl IsA<gtk::Widget>,
) -> Result<gdk::Texture, CaptureError> {
    let widget = widget.as_ref().clone();
    let clock = widget.frame_clock().ok_or(CaptureError::NotRealized)?;
    gio::GioFuture::new(&widget, move |widget, _cancellable, result| {
        let result = RefCell::new(Some(result));
        let handler = Rc::new(RefCell::new(None));
        let id = clock.connect_after_paint(glib::clone!(
            #[strong]
            widget,
            #[strong]
            handler,
            move |clock| {
                if let Some(result) = result.take() {
                    result.resolve(capture(&widget));
                }
                if let Some(id) = handler.take() {
                    clock.disconnect(id);
                }
            }
        ));
        handler.replace(Some(id));
        widget.queue_draw();
    })
    .await
}

/// [`capture_next_frame`] written to `path` as PNG.
pub async fn save_png(widget: &impl IsA<gtk::Widget>, path: &Path) -> Result<(), CaptureError> {
    capture_next_frame(widget)
        .await?
        .save_to_png(path)
        .map_err(CaptureError::Save)
}

/// `window` with `popovers` drawn over it where the compositor placed them, written to
/// `path` as PNG. Popovers are surfaces of their own, which a capture of the window leaves
/// out. The image grows to take in popovers that reach past the window.
pub async fn save_png_with_popovers(window: &gtk::Window, popovers: &[gtk::Popover], path: &Path) -> Result<(), CaptureError> {
    let base = capture_next_frame(window).await?;
    let window_origin = origin(window.upcast_ref()).ok_or(CaptureError::NotRealized)?;
    let scale = window.scale_factor() as f32;
    let mut layers = vec![(base, (0.0, 0.0))];
    for popover in popovers {
        if !popover.is_mapped() {
            return Err(CaptureError::NotRealized);
        }
        let at = origin(popover.upcast_ref()).ok_or(CaptureError::NotRealized)?;
        let texture = capture_next_frame(popover).await?;
        layers.push((texture, ((at.0 - window_origin.0) as f32 * scale, (at.1 - window_origin.1) as f32 * scale)));
    }
    let bounds = layers.iter().fold(graphene::Rect::zero(), |bounds, (texture, (x, y))| {
        bounds.union(&graphene::Rect::new(*x, *y, texture.width() as f32, texture.height() as f32))
    });
    let snapshot = gtk::Snapshot::new();
    for (texture, (x, y)) in &layers {
        snapshot.append_texture(texture, &graphene::Rect::new(*x, *y, texture.width() as f32, texture.height() as f32));
    }
    let node = snapshot.to_node().ok_or(CaptureError::NothingDrawn)?;
    let renderer = window.native().and_then(|n| n.renderer()).ok_or(CaptureError::NotRealized)?;
    renderer.render_texture(&node, Some(&bounds)).save_to_png(path).map_err(CaptureError::Save)
}

/// Where `native`'s widget starts, in logical pixels from its toplevel's surface: popup
/// positions are relative to their parent surface, so they add up along the chain.
fn origin(native: &gtk::Widget) -> Option<(f64, f64)> {
    let native = native.native()?;
    let (dx, dy) = native.surface_transform();
    let (sx, sy) = surface_origin(&native.surface()?);
    Some((sx + dx, sy + dy))
}

fn surface_origin(surface: &gdk::Surface) -> (f64, f64) {
    match surface.downcast_ref::<gdk::Popup>() {
        Some(popup) => {
            let parent = popup.parent().map_or((0.0, 0.0), |p| surface_origin(&p));
            (parent.0 + f64::from(popup.position_x()), parent.1 + f64::from(popup.position_y()))
        }
        None => (0.0, 0.0),
    }
}
