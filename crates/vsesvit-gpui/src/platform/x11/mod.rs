//! Owned by W1.1.
//! The X11 backend. One x11rb connection per process, on the main thread, separate from GPUI's and
//! Chromium's: X11 lets any client act on any window. It selects no events, so everything it reads
//! from the event queue is an error from an unchecked request, which `drain` logs and drops (for
//! example a BadWindow for a child Chromium has just destroyed).

use std::cell::RefCell;
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection as _;
use x11rb::protocol::shape::ConnectionExt as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use super::PlatformError;

pub mod capture;
pub mod desktop;
pub mod focus;
pub mod instance;
pub mod scale;
pub mod surface;

pub use desktop::XDesktop;
pub use surface::XSurface;

/// The 24-bit host's background before a page maps: Chromium's own blank page colour.
const HOST_BACKGROUND: u32 = 0xff_ff_ff;

pub(crate) struct XConn {
    pub(crate) conn: RustConnection,
    pub(crate) root: xproto::Window,
    pub(crate) screen: usize,
}

thread_local! {
    static CONN: RefCell<Option<Rc<XConn>>> = const { RefCell::new(None) };
}

/// The process's connection, opened on first use. Main thread only, like everything in `platform`.
pub(crate) fn conn() -> Result<Rc<XConn>, PlatformError> {
    CONN.with(|cell| {
        if let Some(x) = cell.borrow().as_ref() {
            return Ok(x.clone());
        }
        let x = Rc::new(XConn::connect()?);
        *cell.borrow_mut() = Some(x.clone());
        Ok(x)
    })
}

pub(crate) fn failed(what: &str, e: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(format!("X11 {what}: {e}"))
}

impl XConn {
    fn connect() -> Result<XConn, PlatformError> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| failed("connect", e))?;
        let root = conn.setup().roots[screen].root;
        conn.shape_query_version()
            .map_err(|e| failed("SHAPE", e))?
            .reply()
            .map_err(|e| failed("SHAPE", e))?;
        Ok(XConn { conn, root, screen })
    }

    pub(crate) fn screen(&self) -> &xproto::Screen {
        &self.conn.setup().roots[self.screen]
    }

    /// Flushes, then logs and drops the errors of earlier unchecked requests.
    pub(crate) fn drain(&self) {
        let _ = self.conn.flush();
        while let Ok(Some(event)) = self.conn.poll_for_event() {
            if let x11rb::protocol::Event::Error(e) = event {
                log::debug!("x11: {:?} on 0x{:x} (request {})", e.error_kind, e.bad_value, e.major_opcode);
            }
        }
    }

    /// A 24-bit `InputOutput` window with the root visual and default colormap, border 0, selecting
    /// no events, so clicks on it reach GPUI. Chromium creates its child with CopyFromParent visual,
    /// colormap and border, which fails with BadMatch under GPUI's 32-bit window (spike-core 5.2).
    pub(crate) fn create_host(&self, parent: xproto::Window, rect: super::PhysRect, mapped: bool) -> Result<xproto::Window, PlatformError> {
        let s = self.screen();
        let wid = self.conn.generate_id().map_err(|e| failed("generate_id", e))?;
        let aux = xproto::CreateWindowAux::new()
            .background_pixel(HOST_BACKGROUND)
            .border_pixel(0)
            .colormap(s.default_colormap)
            .bit_gravity(xproto::Gravity::NORTH_WEST);
        let (x, y, w, h) = xrect(rect);
        self.conn
            .create_window(s.root_depth, wid, parent, x, y, w, h, 0, xproto::WindowClass::INPUT_OUTPUT, s.root_visual, &aux)
            .map_err(|e| failed("CreateWindow", e))?
            .check()
            .map_err(|e| failed("CreateWindow", e))?;
        if mapped {
            let _ = self.conn.map_window(wid);
        }
        self.drain();
        Ok(wid)
    }

    pub(crate) fn configure(&self, xid: xproto::Window, rect: super::PhysRect) {
        let aux = xproto::ConfigureWindowAux::new()
            .x(rect.x)
            .y(rect.y)
            .width(rect.width.max(1))
            .height(rect.height.max(1));
        let _ = self.conn.configure_window(xid, &aux);
        self.drain();
    }

    pub(crate) fn set_mapped(&self, xid: xproto::Window, mapped: bool) {
        let _ = if mapped { self.conn.map_window(xid) } else { self.conn.unmap_window(xid) };
        self.drain();
    }

    pub(crate) fn destroy(&self, xid: xproto::Window) {
        let _ = self.conn.destroy_window(xid);
        self.drain();
    }

    /// Unmap, then reparent at 0,0, both checked: when this returns the server has done both.
    pub(crate) fn unmap_and_reparent(&self, xid: xproto::Window, parent: xproto::Window) -> Result<(), PlatformError> {
        self.conn
            .unmap_window(xid)
            .map_err(|e| failed("UnmapWindow", e))?
            .check()
            .map_err(|e| failed(&format!("UnmapWindow 0x{xid:x}"), e))?;
        self.conn
            .reparent_window(xid, parent, 0, 0)
            .map_err(|e| failed("ReparentWindow", e))?
            .check()
            .map_err(|e| failed(&format!("ReparentWindow 0x{xid:x} into 0x{parent:x}"), e))?;
        self.drain();
        Ok(())
    }

    /// The direct children of `xid`, bottom to top.
    pub(crate) fn children(&self, xid: xproto::Window) -> Vec<xproto::Window> {
        self.conn
            .query_tree(xid)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|t| t.children)
            .unwrap_or_default()
    }

    pub(crate) fn parent(&self, xid: xproto::Window) -> Option<xproto::Window> {
        self.conn.query_tree(xid).ok()?.reply().ok().map(|t| t.parent)
    }

    pub(crate) fn intern(&self, name: &str) -> Result<xproto::Atom, PlatformError> {
        let reply = self.conn.intern_atom(false, name.as_bytes()).map_err(|e| failed("InternAtom", e))?;
        Ok(reply.reply().map_err(|e| failed("InternAtom", e))?.atom)
    }

    /// A 32-bit property's first value.
    pub(crate) fn property32(&self, xid: xproto::Window, name: &str, kind: xproto::AtomEnum) -> Option<u32> {
        let atom = self.intern(name).ok()?;
        let reply = self.conn.get_property(false, xid, atom, kind, 0, 1).ok()?.reply().ok()?;
        reply.value32()?.next()
    }
}

/// GPUI's X11 window id.
pub(crate) fn window_xid(window: &gpui::Window) -> Result<xproto::Window, PlatformError> {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xcb(h)) => Ok(h.window.get()),
        Ok(RawWindowHandle::Xlib(h)) => u32::try_from(h.window).map_err(|_| PlatformError::new("an Xlib window id past 32 bits")),
        Ok(other) => Err(PlatformError::new(format!("GPUI is not on X11 ({other:?})"))),
        Err(e) => Err(PlatformError::new(format!("GPUI has no window handle: {e}"))),
    }
}

/// `PhysRect` as X11's i16 position and u16 size, at least 1x1, clamped to what X can express.
pub(crate) fn xrect(r: super::PhysRect) -> (i16, i16, u16, u16) {
    let pos = |v: i32| v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
    let len = |v: u32| v.clamp(1, u16::MAX.into()) as u16;
    (pos(r.x), pos(r.y), len(r.width), len(r.height))
}
