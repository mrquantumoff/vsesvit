//! Our own X11 connection (x11rb) for managing CEF child windows: geometry,
//! visibility, SHAPE cut-outs, input focus and screenshots. GPUI's connection is
//! private, and Chromium has its own; X11 lets any client act on any window.
use anyhow::{Context as _, Result};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

pub struct XConn {
    pub conn: RustConnection,
    pub root: u32,
    pub screen: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl XConn {
    pub fn connect() -> Result<Self> {
        let (conn, screen) = x11rb::connect(None).context("x11rb connect")?;
        let root = conn.setup().roots[screen].root;
        conn.shape_query_version()?.reply().context("SHAPE extension")?;
        Ok(Self { conn, root, screen })
    }

    fn drain(&self) {
        // Unchecked errors (e.g. BadWindow for a child Chromium just destroyed) arrive
        // as events on this connection; log and drop them.
        while let Ok(Some(ev)) = self.conn.poll_for_event() {
            if let x11rb::protocol::Event::Error(e) = ev {
                crate::log(&format!("x11: async error {:?} on 0x{:x}", e.error_kind, e.bad_value));
            }
        }
    }

    /// Create the 24-bit "content host" window inside GPUI's (32-bit ARGB) toplevel.
    /// Chromium creates its child with CopyFromParent visual/colormap/border, which
    /// fails with BadMatch under a parent of a different depth; under this host it works.
    pub fn create_host(&self, parent: u32, r: XRect, bg: u32) -> Result<u32> {
        let s = &self.conn.setup().roots[self.screen];
        let wid = self.conn.generate_id()?;
        let aux = xproto::CreateWindowAux::new()
            .background_pixel(bg)
            .border_pixel(0)
            .colormap(s.default_colormap)
            .bit_gravity(xproto::Gravity::NORTH_WEST);
        self.conn
            .create_window(s.root_depth, wid, parent, r.x as i16, r.y as i16, r.w.max(1) as u16, r.h.max(1) as u16, 0,
                xproto::WindowClass::INPUT_OUTPUT, s.root_visual, &aux)?
            .check()
            .context("create host window")?;
        self.conn.map_window(wid)?;
        self.conn.flush()?;
        Ok(wid)
    }

    /// The keyboard-focus proxy the patched GPUI creates inside each toplevel
    /// (InputOnly, WM_NAME "gpui-focus-proxy"). X focus goes there, not to the toplevel,
    /// while GPUI chrome owns the keyboard: with focus on the toplevel X would deliver
    /// keys to whatever descendant is under the pointer (a CEF child over the page), and
    /// key events never propagate past the focus window, so the proxy must be selected
    /// by GPUI's own connection.
    pub fn find_child_named(&self, parent: u32, name: &str) -> Option<u32> {
        let t = self.conn.query_tree(parent).ok()?.reply().ok()?;
        t.children.into_iter().find(|&c| {
            self.conn
                .get_property(false, c, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::ANY, 0, 64)
                .ok()
                .and_then(|r| r.reply().ok())
                .is_some_and(|p| p.value == name.as_bytes())
        })
    }

    pub fn destroy(&self, xid: u32) {
        let _ = self.conn.destroy_window(xid);
        let _ = self.conn.flush();
        self.drain();
    }

    pub fn configure(&self, xid: u32, r: XRect) {
        let aux = xproto::ConfigureWindowAux::new().x(r.x).y(r.y).width(r.w.max(1)).height(r.h.max(1));
        let _ = self.conn.configure_window(xid, &aux);
        let _ = self.conn.flush();
        self.drain();
    }

    pub fn map(&self, xid: u32, mapped: bool) {
        let _ = if mapped { self.conn.map_window(xid) } else { self.conn.unmap_window(xid) };
        let _ = self.conn.flush();
        self.drain();
    }

    pub fn reparent(&self, xid: u32, parent: u32, x: i32, y: i32, w: u32, h: u32) -> Result<()> {
        self.conn.unmap_window(xid)?;
        self.conn.reparent_window(xid, parent, x as i16, y as i16)?;
        let aux = xproto::ConfigureWindowAux::new().x(x).y(y).width(w.max(1)).height(h.max(1));
        self.conn.configure_window(xid, &aux)?;
        self.conn.map_window(xid)?;
        self.conn.flush()?;
        self.drain();
        Ok(())
    }

    pub fn parent_of(&self, xid: u32) -> Option<u32> {
        self.conn.query_tree(xid).ok()?.reply().ok().map(|t| t.parent)
    }

    /// An unmapped 24-bit window under the root: anything reparented into it is never viewable.
    pub fn create_holder(&self) -> Result<u32> {
        let s = &self.conn.setup().roots[self.screen];
        let wid = self.conn.generate_id()?;
        let aux = xproto::CreateWindowAux::new().border_pixel(0).colormap(s.default_colormap).override_redirect(1);
        self.conn
            .create_window(s.root_depth, wid, s.root, -20000, -20000, 1920, 1080, 0, xproto::WindowClass::INPUT_OUTPUT, s.root_visual, &aux)?
            .check()?;
        self.conn.flush()?;
        Ok(wid)
    }

    pub fn reparent_only(&self, xid: u32, parent: u32, x: i32, y: i32) -> Result<()> {
        self.conn.reparent_window(xid, parent, x as i16, y as i16)?.check()?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn raise(&self, xid: u32) {
        let aux = xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE);
        let _ = self.conn.configure_window(xid, &aux);
        let _ = self.conn.flush();
    }

    /// Give X keyboard focus to `xid` (e.g. back to the GPUI toplevel from a CEF child).
    pub fn set_focus(&self, xid: u32) {
        let _ = self.conn.set_input_focus(xproto::InputFocus::PARENT, xid, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
        self.drain();
    }

    /// Is `w` equal to or a descendant of `ancestor`?
    pub fn is_within(&self, mut w: u32, ancestor: u32) -> bool {
        for _ in 0..32 {
            if w == ancestor {
                return true;
            }
            if w == self.root || w <= 1 {
                return false;
            }
            match self.conn.query_tree(w).ok().and_then(|c| c.reply().ok()) {
                Some(t) => w = t.parent,
                None => return false,
            }
        }
        false
    }

    pub fn focus(&self) -> u32 {
        self.conn.get_input_focus().ok().and_then(|c| c.reply().ok()).map(|r| r.focus).unwrap_or(0)
    }

    /// Cut `holes` (window-relative) out of `xid`'s bounding (and so input) shape.
    pub fn shape_exclude(&self, xid: u32, size: (u32, u32), holes: &[XRect]) {
        let mut rects = vec![xproto::Rectangle { x: 0, y: 0, width: size.0 as u16, height: size.1 as u16 }];
        for h in holes {
            rects = rects.into_iter().flat_map(|r| subtract(r, *h)).collect();
        }
        for kind in [shape::SK::BOUNDING, shape::SK::INPUT] {
            let _ = self.conn.shape_rectangles(shape::SO::SET, kind, xproto::ClipOrdering::UNSORTED, xid, 0, 0, &rects);
        }
        let _ = self.conn.flush();
        self.drain();
    }

    pub fn shape_reset(&self, xid: u32) {
        for kind in [shape::SK::BOUNDING, shape::SK::INPUT] {
            let _ = self.conn.shape_mask(shape::SO::SET, kind, xid, 0, 0, x11rb::NONE);
        }
        let _ = self.conn.flush();
        self.drain();
    }

    pub fn translate_to_root(&self, xid: u32) -> Option<(i32, i32)> {
        let r = self.conn.translate_coordinates(xid, self.root, 0, 0).ok()?.reply().ok()?;
        Some((r.dst_x as i32, r.dst_y as i32))
    }

    pub fn geometry(&self, xid: u32) -> Option<XRect> {
        let g = self.conn.get_geometry(xid).ok()?.reply().ok()?;
        Some(XRect { x: g.x as i32, y: g.y as i32, w: g.width as u32, h: g.height as u32 })
    }

    /// One line per window in the subtree: xid, geometry, depth, map state, WM_NAME.
    pub fn tree(&self, xid: u32, depth: usize, out: &mut Vec<String>) {
        let Ok(Ok(t)) = self.conn.query_tree(xid).map(|c| c.reply()) else { return };
        for child in t.children {
            let g = self.conn.get_geometry(child).ok().and_then(|c| c.reply().ok());
            let a = self.conn.get_window_attributes(child).ok().and_then(|c| c.reply().ok());
            let name = self
                .conn
                .get_property(false, child, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::ANY, 0, 256)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|p| String::from_utf8_lossy(&p.value).into_owned())
                .unwrap_or_default();
            let (gs, d) = g.map(|g| (format!("{}x{}+{}+{}", g.width, g.height, g.x, g.y), g.depth)).unwrap_or_default();
            let mapped = a.map(|a| format!("{:?}", a.map_state)).unwrap_or_default();
            out.push(format!("{}0x{child:x} {gs} depth={d} {mapped} {name:?}", "  ".repeat(depth)));
            self.tree(child, depth + 1, out);
        }
    }

    /// Capture `xid` and every mapped descendant into one RGBA image. Children of a
    /// different depth (Chromium's 24-bit window inside GPUI's 32-bit one) are
    /// "undefined" in a parent GetImage, so each is read on its own and pasted.
    pub fn capture(&self, xid: u32) -> Result<(u32, u32, Vec<u8>)> {
        self.capture_mode(xid, true)
    }

    /// `paste = false`: one GetImage of `xid` only (what the X server composites).
    pub fn capture_mode(&self, xid: u32, paste: bool) -> Result<(u32, u32, Vec<u8>)> {
        let g = self.conn.get_geometry(xid)?.reply()?;
        let (w, h) = (g.width as u32, g.height as u32);
        let mut out = self.get_rgba(xid, w, h)?;
        if paste {
            self.paste_children(xid, 0, 0, w, h, &mut out, None);
        }
        Ok((w, h, out))
    }

    /// `clip`: inherited SHAPE region of an ancestor, in capture coordinates.
    fn paste_children(&self, xid: u32, ox: i32, oy: i32, w: u32, h: u32, out: &mut [u8], clip: Option<&[(i32, i32, i32, i32)]>) {
        let Ok(Ok(t)) = self.conn.query_tree(xid).map(|c| c.reply()) else { return };
        for child in t.children {
            let Some(a) = self.conn.get_window_attributes(child).ok().and_then(|c| c.reply().ok()) else { continue };
            if a.map_state != xproto::MapState::VIEWABLE || a.class == xproto::WindowClass::INPUT_ONLY {
                continue;
            }
            let Some(g) = self.conn.get_geometry(child).ok().and_then(|c| c.reply().ok()) else { continue };
            let (cx, cy) = (ox + g.x as i32, oy + g.y as i32);
            // Respect SHAPE cut-outs (this window's and its ancestors'): skip pixels outside.
            let own: Option<Vec<(i32, i32, i32, i32)>> = self.bounding_rects(child).map(|rs| {
                rs.iter().map(|r| (cx + r.x as i32, cy + r.y as i32, cx + r.x as i32 + r.width as i32, cy + r.y as i32 + r.height as i32)).collect()
            });
            let inside = |px: i32, py: i32| {
                own.as_ref().is_none_or(|rs| rs.iter().any(|&(a, b, c, d)| px >= a && py >= b && px < c && py < d))
                    && clip.is_none_or(|rs| rs.iter().any(|&(a, b, c, d)| px >= a && py >= b && px < c && py < d))
            };
            if let Ok(px) = self.get_rgba(child, g.width as u32, g.height as u32) {
                for y in 0..g.height as i32 {
                    for x in 0..g.width as i32 {
                        let (dx, dy) = (cx + x, cy + y);
                        if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                            continue;
                        }
                        if !inside(dx, dy) {
                            continue;
                        }
                        let s = ((y as u32 * g.width as u32 + x as u32) * 4) as usize;
                        let d = ((dy as u32 * w + dx as u32) * 4) as usize;
                        out[d..d + 4].copy_from_slice(&px[s..s + 4]);
                    }
                }
            }
            let child_clip: Option<Vec<(i32, i32, i32, i32)>> = match (&own, clip) {
                (Some(o), _) => Some(o.clone()), // (approximation: own shape replaces the inherited one)
                (None, Some(c)) => Some(c.to_vec()),
                (None, None) => None,
            };
            self.paste_children(child, cx, cy, w, h, out, child_clip.as_deref());
        }
    }

    fn bounding_rects(&self, xid: u32) -> Option<Vec<xproto::Rectangle>> {
        let ext = self.conn.shape_query_extents(xid).ok()?.reply().ok()?;
        if !ext.bounding_shaped {
            return None;
        }
        let r = self.conn.shape_get_rectangles(xid, shape::SK::BOUNDING).ok()?.reply().ok()?;
        Some(r.rectangles)
    }

    fn get_rgba(&self, xid: u32, w: u32, h: u32) -> Result<Vec<u8>> {
        let img = self.conn.get_image(xproto::ImageFormat::Z_PIXMAP, xid, 0, 0, w as u16, h as u16, !0)?.reply()?;
        // 24/32-bit TrueColor, LSBFirst: bytes are B G R X.
        let mut out = vec![0u8; (w * h * 4) as usize];
        let stride = img.data.len() / h.max(1) as usize;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let s = y * stride + x * 4;
                let d = (y * w as usize + x) * 4;
                if s + 3 < img.data.len() {
                    out[d] = img.data[s + 2];
                    out[d + 1] = img.data[s + 1];
                    out[d + 2] = img.data[s];
                    out[d + 3] = 255;
                }
            }
        }
        Ok(out)
    }
}

pub fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// r minus hole, as up to four rectangles.
fn subtract(r: xproto::Rectangle, hole: XRect) -> Vec<xproto::Rectangle> {
    let (rx0, ry0, rx1, ry1) = (r.x as i32, r.y as i32, r.x as i32 + r.width as i32, r.y as i32 + r.height as i32);
    let (hx0, hy0, hx1, hy1) = (hole.x.max(rx0), hole.y.max(ry0), (hole.x + hole.w as i32).min(rx1), (hole.y + hole.h as i32).min(ry1));
    if hx0 >= hx1 || hy0 >= hy1 {
        return vec![r];
    }
    let mk = |x0: i32, y0: i32, x1: i32, y1: i32| xproto::Rectangle { x: x0 as i16, y: y0 as i16, width: (x1 - x0) as u16, height: (y1 - y0) as u16 };
    let mut v = Vec::new();
    if hy0 > ry0 { v.push(mk(rx0, ry0, rx1, hy0)); }
    if hy1 < ry1 { v.push(mk(rx0, hy1, rx1, ry1)); }
    if hx0 > rx0 { v.push(mk(rx0, hy0, hx0, hy1)); }
    if hx1 < rx1 { v.push(mk(hx1, hy0, rx1, hy1)); }
    v
}
