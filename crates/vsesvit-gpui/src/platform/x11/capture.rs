//! Owned by W1.1.
//! A window's pixels, page included (spike-core section 11). Where the server composites (Xwayland,
//! or a compositing manager), one `GetImage` of the toplevel already holds the 24-bit children:
//! 5 to 9 ms. A plain Xorg or Xvfb leaves children of another depth "undefined" in their parent's
//! image, so there every mapped descendant is read on its own and pasted in, clipped by its own and
//! its ancestors' bounding SHAPE: 20 to 25 ms.

use image::{Rgba, RgbaImage};
use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};

use super::{XConn, failed};
use crate::platform::{PhysRect, PlatformError};

pub(crate) fn window(x: &XConn, xid: xproto::Window) -> Result<RgbaImage, PlatformError> {
    let g = x.conn.get_geometry(xid).map_err(|e| failed("GetGeometry", e))?.reply().map_err(|e| failed("GetGeometry", e))?;
    let mut out = read(x, xid, u32::from(g.width), u32::from(g.height))?;
    if !composited(x) {
        paste_children(x, xid, (0, 0), &mut out, None);
    }
    Ok(out)
}

/// Does the server put children of another depth into their toplevel's image?
fn composited(x: &XConn) -> bool {
    let xwayland = matches!(x.conn.extension_information("XWAYLAND"), Ok(Some(_)));
    let manager = x
        .intern(&format!("_NET_WM_CM_S{}", x.screen))
        .ok()
        .and_then(|atom| x.conn.get_selection_owner(atom).ok()?.reply().ok())
        .is_some_and(|r| r.owner != x11rb::NONE);
    xwayland || manager
}

/// Rectangles in capture coordinates; a pixel is drawn only inside all of them that apply.
type Clip = Vec<PhysRect>;

fn paste_children(x: &XConn, xid: xproto::Window, origin: (i32, i32), out: &mut RgbaImage, clip: Option<&Clip>) {
    for child in x.children(xid) {
        let Some(attrs) = x.conn.get_window_attributes(child).ok().and_then(|c| c.reply().ok()) else { continue };
        if attrs.map_state != xproto::MapState::VIEWABLE || attrs.class == xproto::WindowClass::INPUT_ONLY {
            continue;
        }
        let Some(g) = x.conn.get_geometry(child).ok().and_then(|c| c.reply().ok()) else { continue };
        let at = (origin.0 + i32::from(g.x), origin.1 + i32::from(g.y));
        let own: Option<Clip> = bounding(x, child).map(|rects| {
            rects.iter().map(|r| PhysRect { x: at.0 + i32::from(r.x), y: at.1 + i32::from(r.y), width: r.width.into(), height: r.height.into() }).collect()
        });
        let inside = |px: i32, py: i32| [own.as_ref(), clip].into_iter().flatten().all(|rects| rects.iter().any(|r| contains(*r, px, py)));
        if let Ok(pixels) = read(x, child, u32::from(g.width), u32::from(g.height)) {
            for (cx, cy, pixel) in pixels.enumerate_pixels() {
                let (dx, dy) = (at.0 + cx as i32, at.1 + cy as i32);
                if dx >= 0 && dy >= 0 && (dx as u32) < out.width() && (dy as u32) < out.height() && inside(dx, dy) {
                    out.put_pixel(dx as u32, dy as u32, *pixel);
                }
            }
        }
        // Approximation kept from the spike: a child's own shape replaces the inherited one, which
        // holds for our trees (only the content host is shaped).
        let inherited = own.as_ref().or(clip);
        paste_children(x, child, at, out, inherited);
    }
}

fn contains(r: PhysRect, px: i32, py: i32) -> bool {
    px >= r.x && py >= r.y && px < r.x + r.width as i32 && py < r.y + r.height as i32
}

fn bounding(x: &XConn, xid: xproto::Window) -> Option<Vec<xproto::Rectangle>> {
    let extents = x.conn.shape_query_extents(xid).ok()?.reply().ok()?;
    if !extents.bounding_shaped {
        return None;
    }
    Some(x.conn.shape_get_rectangles(xid, shape::SK::BOUNDING).ok()?.reply().ok()?.rectangles)
}

/// One `GetImage` as opaque RGBA. TrueColor at 24 or 32 bits per pixel, in the server's byte order.
fn read(x: &XConn, xid: xproto::Window, width: u32, height: u32) -> Result<RgbaImage, PlatformError> {
    let (w, h) = (u16::try_from(width).unwrap_or(u16::MAX), u16::try_from(height).unwrap_or(u16::MAX));
    let img = x
        .conn
        .get_image(xproto::ImageFormat::Z_PIXMAP, xid, 0, 0, w, h, !0)
        .map_err(|e| failed("GetImage", e))?
        .reply()
        .map_err(|e| failed(&format!("GetImage 0x{xid:x}"), e))?;
    let setup = x.conn.setup();
    let bpp = setup.pixmap_formats.iter().find(|f| f.depth == img.depth).map_or(32, |f| f.bits_per_pixel);
    if bpp != 32 {
        return Err(PlatformError::new(format!("GetImage 0x{xid:x}: {bpp} bits per pixel")));
    }
    let msb = setup.image_byte_order == xproto::ImageOrder::MSB_FIRST;
    let stride = img.data.len() / usize::from(h.max(1));
    let mut out = RgbaImage::new(width, height);
    for (px, py, pixel) in out.enumerate_pixels_mut() {
        let at = py as usize * stride + px as usize * 4;
        let Some(bytes) = img.data.get(at..at + 4) else { continue };
        let bytes: [u8; 4] = bytes.try_into().expect("four bytes");
        let value = if msb { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) };
        *pixel = Rgba([(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]);
    }
    Ok(out)
}
