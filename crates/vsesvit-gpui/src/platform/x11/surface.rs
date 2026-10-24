//! Owned by W1.1.
//! A GPUI window's content host: a 24-bit child of GPUI's 32-bit toplevel that parents every page,
//! placed by `ConfigureWindow` from the content element's prepaint, with overlay holes cut by SHAPE
//! (spike-core sections 5 and 9). Only requests on our own connection; no GPUI state is touched, so
//! nothing here needs deferring out of a GPUI update.

use std::rc::Rc;

use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto;

use super::{XConn, capture, focus};
use crate::platform::{AuxHost, FocusOutcome, FocusTarget, Hole, NativeHandle, NativeSurface, PhysRect, PlatformError};

pub struct XSurface {
    x: Rc<XConn>,
    toplevel: xproto::Window,
    proxy: xproto::Window,
    host: xproto::Window,
    content: PhysRect,
    /// Pages in the host and whether each is shown.
    pages: Vec<(xproto::Window, bool)>,
    holes: Vec<Hole>,
    aux: Vec<xproto::Window>,
}

impl XSurface {
    fn page_size(&self) -> PhysRect {
        PhysRect { x: 0, y: 0, width: self.content.width, height: self.content.height }
    }

    fn forget(&mut self, page: xproto::Window) {
        self.pages.retain(|&(p, _)| p != page);
    }

    fn apply_holes(&self) {
        let kinds = [shape::SK::BOUNDING, shape::SK::INPUT];
        if self.holes.is_empty() {
            for kind in kinds {
                let _ = self.x.conn.shape_mask(shape::SO::SET, kind, self.host, 0, 0, x11rb::NONE);
            }
        } else {
            let holes: Vec<PhysRect> = self.holes.iter().map(|h| host_relative(h.rect, self.content)).collect();
            let rects = visible_rects(self.page_size(), &holes);
            for kind in kinds {
                let _ = self.x.conn.shape_rectangles(shape::SO::SET, kind, xproto::ClipOrdering::UNSORTED, self.host, 0, 0, &rects);
            }
        }
        self.x.drain();
    }
}

impl NativeSurface for XSurface {
    const TRANSPARENT_CONTENT: bool = false;

    fn attach(window: &gpui::Window) -> Result<XSurface, PlatformError> {
        let x = super::conn()?;
        let toplevel = super::window_xid(window)?;
        let proxy = focus::proxy_of(&x, toplevel)
            .ok_or_else(|| PlatformError::new(format!("GPUI window 0x{toplevel:x} has no focus proxy; is gpui-pre-linux patched?")))?;
        let content = PhysRect { x: 0, y: 0, width: 1, height: 1 };
        let host = x.create_host(toplevel, content, true)?;
        log::debug!("x11: content host 0x{host:x} in 0x{toplevel:x}, focus proxy 0x{proxy:x}");
        Ok(XSurface { x, toplevel, proxy, host, content, pages: Vec::new(), holes: Vec::new(), aux: Vec::new() })
    }

    fn host(&self) -> NativeHandle {
        NativeHandle::from_xid(self.host)
    }

    fn set_content_rect(&mut self, rect: PhysRect) {
        if rect == self.content {
            return;
        }
        self.content = rect;
        self.x.configure(self.host, rect);
        for &(page, shown) in &self.pages {
            if shown {
                self.x.configure(page, self.page_size());
            }
        }
        if !self.holes.is_empty() {
            self.apply_holes();
        }
    }

    fn show_page(&mut self, page: NativeHandle, visible: bool) {
        let page = page.xid();
        match self.pages.iter_mut().find(|(p, _)| *p == page) {
            Some(entry) => entry.1 = visible,
            None => self.pages.push((page, visible)),
        }
        if visible {
            self.x.configure(page, self.page_size());
        }
        self.x.set_mapped(page, visible);
    }

    fn set_holes(&mut self, holes: &[Hole]) {
        if holes == self.holes.as_slice() {
            return;
        }
        self.holes = holes.to_vec();
        self.apply_holes();
    }

    fn focus(&mut self, target: FocusTarget) -> FocusOutcome {
        let want = match target {
            FocusTarget::Chrome => self.proxy,
            FocusTarget::Page(page) => page.xid(),
        };
        let current = focus::current(&self.x);
        if current == want {
            return FocusOutcome::Done;
        }
        // Moving focus while another toplevel holds it would steal it from the window the WM
        // thinks is active and start a focus fight with the WM (spike-core 7.3).
        if !focus::is_within(&self.x, current, self.toplevel) {
            return FocusOutcome::Refused;
        }
        match focus::set(&self.x, want) {
            Ok(()) => FocusOutcome::Done,
            Err(e) => {
                log::warn!("x11: focus to 0x{want:x} failed: {e}");
                FocusOutcome::Refused
            }
        }
    }

    fn move_page_to(&mut self, page: NativeHandle, to: &mut XSurface) -> Result<(), PlatformError> {
        let xid = page.xid();
        self.x.unmap_and_reparent(xid, to.host)?;
        self.forget(xid);
        to.pages.push((xid, false));
        to.x.configure(xid, to.page_size());
        Ok(())
    }

    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError> {
        let xid = page.xid();
        self.x.unmap_and_reparent(xid, hidden_host.xid())?;
        self.forget(xid);
        Ok(())
    }

    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError> {
        // Created after the content host, so it stacks above it.
        let aux = self.x.create_host(self.toplevel, rect, true)?;
        self.aux.push(aux);
        Ok(AuxHost(NativeHandle::from_xid(aux)))
    }

    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect) {
        self.x.configure(aux.native().xid(), rect);
    }

    fn close_aux(&mut self, aux: AuxHost) {
        let xid = aux.native().xid();
        self.aux.retain(|&a| a != xid);
        self.x.destroy(xid);
    }

    fn capture(&self) -> Result<image::RgbaImage, PlatformError> {
        capture::window(&self.x, self.toplevel)
    }

    fn detach(self) {}
}

/// Destroys the hosts. A page still inside dies with its host, which is why the shell parks or
/// closes every page before `detach`.
impl Drop for XSurface {
    fn drop(&mut self) {
        for &aux in &self.aux {
            self.x.destroy(aux);
        }
        self.x.destroy(self.host);
    }
}

fn host_relative(r: PhysRect, content: PhysRect) -> PhysRect {
    PhysRect { x: r.x - content.x, y: r.y - content.y, ..r }
}

/// `area` minus every hole, as rectangles for `ShapeRectangles`.
pub(crate) fn visible_rects(area: PhysRect, holes: &[PhysRect]) -> Vec<xproto::Rectangle> {
    let mut rects = vec![area];
    for &hole in holes {
        rects = rects.into_iter().flat_map(|r| subtract(r, hole)).collect();
    }
    rects
        .into_iter()
        .map(|r| {
            let (x, y, width, height) = super::xrect(r);
            xproto::Rectangle { x, y, width, height }
        })
        .collect()
}

/// `r` minus `hole`, as up to four rectangles.
fn subtract(r: PhysRect, hole: PhysRect) -> Vec<PhysRect> {
    let (rx0, ry0) = (r.x, r.y);
    let (rx1, ry1) = (r.x + r.width as i32, r.y + r.height as i32);
    let (hx0, hy0) = (hole.x.max(rx0), hole.y.max(ry0));
    let (hx1, hy1) = ((hole.x + hole.width as i32).min(rx1), (hole.y + hole.height as i32).min(ry1));
    if hx0 >= hx1 || hy0 >= hy1 {
        return vec![r];
    }
    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| PhysRect { x: x0, y: y0, width: (x1 - x0) as u32, height: (y1 - y0) as u32 };
    let mut out = Vec::with_capacity(4);
    if hy0 > ry0 {
        out.push(rect(rx0, ry0, rx1, hy0));
    }
    if hy1 < ry1 {
        out.push(rect(rx0, hy1, rx1, ry1));
    }
    if hx0 > rx0 {
        out.push(rect(rx0, hy0, hx0, hy1));
    }
    if hx1 < rx1 {
        out.push(rect(hx1, hy0, rx1, hy1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(r: &[xproto::Rectangle]) -> u32 {
        r.iter().map(|r| u32::from(r.width) * u32::from(r.height)).sum()
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> PhysRect {
        PhysRect { x, y, width, height }
    }

    #[test]
    fn a_hole_inside_leaves_four_rects_around_it() {
        let rects = visible_rects(rect(0, 0, 100, 80), &[rect(10, 20, 30, 40)]);
        assert_eq!(rects.len(), 4);
        assert_eq!(area(&rects), 100 * 80 - 30 * 40);
    }

    #[test]
    fn holes_are_clipped_to_the_area() {
        let rects = visible_rects(rect(0, 0, 100, 80), &[rect(-10, -10, 30, 30)]);
        assert_eq!(area(&rects), 100 * 80 - 20 * 20);
    }

    #[test]
    fn a_hole_outside_changes_nothing_and_one_over_everything_leaves_nothing() {
        assert_eq!(visible_rects(rect(0, 0, 100, 80), &[rect(200, 0, 10, 10)]).len(), 1);
        assert!(visible_rects(rect(0, 0, 100, 80), &[rect(-1, -1, 200, 200)]).is_empty());
    }

    #[test]
    fn overlapping_holes_are_not_counted_twice() {
        let rects = visible_rects(rect(0, 0, 100, 100), &[rect(0, 0, 50, 50), rect(25, 25, 50, 50)]);
        assert_eq!(area(&rects), 100 * 100 - (50 * 50 + 50 * 50 - 25 * 25));
    }

    #[test]
    fn holes_are_relative_to_the_window_and_shapes_to_the_host() {
        let hole = host_relative(rect(110, 220, 30, 40), rect(100, 200, 500, 300));
        assert_eq!(hole, rect(10, 20, 30, 40));
    }
}
