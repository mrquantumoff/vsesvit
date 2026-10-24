//! Owned by W1.1.
//! Process-wide X11 work: the hidden host, dialog parents, the browser window under a point, pointer
//! presses for outside-click dismissal, raising a window for a forwarded launch, and neutralizing
//! windows Chromium opens on its own.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::wrapper::ConnectionExt as _;

use super::{XConn, failed, focus};
use crate::platform::{Desktop, NativeHandle, PhysPoint, PlatformError};

/// How deep under a root child (a WM frame) our toplevel may sit: openbox frames it directly,
/// KWin through a wrapper.
const FRAME_DEPTH: usize = 3;

type Subscribers = Arc<Mutex<Vec<UnboundedSender<PhysPoint>>>>;

pub struct XDesktop {
    x: Rc<XConn>,
    hidden: xproto::Window,
    presses: Subscribers,
}

impl Desktop for XDesktop {
    fn init() -> Result<XDesktop, PlatformError> {
        let x = super::conn()?;
        let hidden = create_hidden_host(&x)?;
        log::debug!("x11: hidden host 0x{hidden:x}");
        Ok(XDesktop { x, hidden, presses: Arc::default() })
    }

    fn adopt_popup(&self, _popup: &gpui::Window, _owner: &gpui::Window) {}

    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window) {
        let (Ok(dialog), Ok(parent)) = (super::window_xid(dialog), super::window_xid(parent)) else { return };
        let _ = self.x.conn.change_property32(
            xproto::PropMode::REPLACE,
            dialog,
            xproto::AtomEnum::WM_TRANSIENT_FOR,
            xproto::AtomEnum::WINDOW,
            &[parent],
        );
        self.x.drain();
    }

    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle> {
        let x = &*self.x;
        let pid = std::process::id();
        for top in x.children(x.root).into_iter().rev() {
            let Some(attrs) = x.conn.get_window_attributes(top).ok().and_then(|c| c.reply().ok()) else { continue };
            if attrs.map_state != xproto::MapState::VIEWABLE || attrs.class == xproto::WindowClass::INPUT_ONLY || !contains_point(x, top, at) {
                continue;
            }
            let ours = gpui_window_in(x, top, pid, FRAME_DEPTH);
            // Our own popups (override-redirect, never framed) are not browser windows; look under them.
            if attrs.override_redirect && ours.is_some() {
                continue;
            }
            return ours.map(NativeHandle::from_xid);
        }
        None
    }

    fn pointer_presses(&self) -> UnboundedReceiver<PhysPoint> {
        let (tx, rx) = unbounded();
        let mut subscribers = self.presses.lock().unwrap_or_else(|e| e.into_inner());
        let start = subscribers.is_empty();
        subscribers.push(tx);
        drop(subscribers);
        if start {
            let presses = self.presses.clone();
            let spawned = std::thread::Builder::new().name("x11-pointer-watch".into()).spawn(move || {
                let Err(e) = watch_presses(&presses);
                log::warn!("x11: pointer presses are not watched: {e}");
                // Their receivers end; the next call starts a new watcher.
                presses.lock().unwrap_or_else(|e| e.into_inner()).clear();
            });
            if let Err(e) = spawned {
                log::warn!("x11: no pointer watcher thread: {e}");
            }
        }
        rx
    }

    fn raise_for_invocation(&self, window: &gpui::Window, token: Option<&str>) {
        let Ok(xid) = super::window_xid(window) else { return };
        let x = &*self.x;
        if let Some(token) = token
            && let (Ok(id), Ok(utf8)) = (x.intern("_NET_STARTUP_ID"), x.intern("UTF8_STRING"))
        {
            let _ = x.conn.change_property8(xproto::PropMode::REPLACE, xid, id, utf8, token.as_bytes());
        }
        let Ok(active) = x.intern("_NET_ACTIVE_WINDOW") else { return };
        // Source 1: a normal application. The startup id's `_TIME<n>` gives the WM a user time to
        // judge the request by; without one, CurrentTime.
        let time = token.and_then(startup_time).unwrap_or(x11rb::CURRENT_TIME);
        let event = xproto::ClientMessageEvent::new(32, xid, active, [1, time, 0, 0, 0]);
        let mask = xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = x.conn.send_event(false, x.root, mask, event);
        x.drain();
    }

    fn hidden_host(&self) -> NativeHandle {
        NativeHandle::from_xid(self.hidden)
    }

    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError> {
        self.x.unmap_and_reparent(window.xid(), self.hidden)
    }
}

/// After the last browser closed (design section 3.6). Whatever is still parked dies with it.
impl Drop for XDesktop {
    fn drop(&mut self) {
        self.x.destroy(self.hidden);
    }
}

/// The holder WX.0 ran its hidden browsers in on Linux (spike-ext-win 5.1): a 24-bit toplevel,
/// override-redirect so no WM ever manages it, off screen and never mapped, so nothing in it is
/// viewable or takes focus. Unlike Windows (ext-win decision 3) it belongs to no browser window, so
/// it lives from `init` to drop whatever windows open and close. Screen-sized, for the hidden pages.
fn create_hidden_host(x: &XConn) -> Result<xproto::Window, PlatformError> {
    let s = x.screen();
    let wid = x.conn.generate_id().map_err(|e| failed("generate_id", e))?;
    let aux = xproto::CreateWindowAux::new().border_pixel(0).colormap(s.default_colormap).override_redirect(1);
    x.conn
        .create_window(s.root_depth, wid, x.root, -20000, -20000, 1920, 1080, 0, xproto::WindowClass::INPUT_OUTPUT, s.root_visual, &aux)
        .map_err(|e| failed("CreateWindow", e))?
        .check()
        .map_err(|e| failed("CreateWindow (hidden host)", e))?;
    Ok(wid)
}

fn contains_point(x: &XConn, xid: xproto::Window, at: PhysPoint) -> bool {
    let Some(g) = x.conn.get_geometry(xid).ok().and_then(|c| c.reply().ok()) else { return false };
    let (bx, by) = (i32::from(g.x), i32::from(g.y));
    let border = i32::from(g.border_width) * 2;
    at.x >= bx && at.y >= by && at.x < bx + i32::from(g.width) + border && at.y < by + i32::from(g.height) + border
}

/// A GPUI window of this process at or below `xid`: it has the patched GPUI's focus proxy and our
/// `_NET_WM_PID`. Chromium's own toplevels carry our pid but no proxy.
fn gpui_window_in(x: &XConn, xid: xproto::Window, pid: u32, depth: usize) -> Option<xproto::Window> {
    let children = x.children(xid);
    if children.iter().any(|&c| focus::is_proxy(x, c)) {
        let theirs = x.property32(xid, "_NET_WM_PID", xproto::AtomEnum::CARDINAL);
        return (theirs == Some(pid)).then_some(xid);
    }
    if depth == 0 {
        return None;
    }
    children.into_iter().rev().find_map(|c| gpui_window_in(x, c, pid, depth - 1))
}

/// `…_TIME<n>` at the end of a startup id (the startup-notification spec's convention).
fn startup_time(token: &str) -> Option<u32> {
    token.rsplit_once("_TIME").and_then(|(_, n)| n.parse().ok())
}

/// XI2 raw button presses on the root of our own connection, for the life of the process. Raw
/// events carry no position, so each press is followed by a `QueryPointer`.
fn watch_presses(subscribers: &Subscribers) -> Result<std::convert::Infallible, String> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn.setup().roots[screen].root;
    let version = conn.xinput_xi_query_version(2, 2).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    if version.major_version < 2 {
        return Err(format!("XInput {}.{}", version.major_version, version.minor_version));
    }
    // Every device, kept to slave events (deviceid == sourceid) below: one event per physical press
    // whether or not the server also reports it for the master.
    let mask = xinput::EventMask { deviceid: xinput::Device::ALL.into(), mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS] };
    conn.xinput_xi_select_events(root, &[mask]).map_err(|e| e.to_string())?.check().map_err(|e| e.to_string())?;
    loop {
        let event = conn.wait_for_event().map_err(|e| e.to_string())?;
        let Event::XinputRawButtonPress(press) = event else { continue };
        if press.deviceid != press.sourceid || !is_click(press.detail) {
            continue;
        }
        let Ok(pointer) = conn.query_pointer(root).map_err(|e| e.to_string())?.reply() else { continue };
        let at = PhysPoint { x: pointer.root_x.into(), y: pointer.root_y.into() };
        subscribers.lock().unwrap_or_else(|e| e.into_inner()).retain(|tx| tx.unbounded_send(at).is_ok());
    }
}

/// Buttons 4 to 7 are the wheel; a scroll is not a click.
fn is_click(button: u32) -> bool {
    !(4..=7).contains(&button)
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_ids_carry_their_time() {
        assert_eq!(super::startup_time("kwin-123-host_TIME4567"), Some(4567));
        assert_eq!(super::startup_time("no-time-here"), None);
        assert_eq!(super::startup_time("bad_TIMEx"), None);
    }

    #[test]
    fn the_wheel_is_not_a_click() {
        assert!(super::is_click(1) && super::is_click(3) && super::is_click(8));
        assert!(!super::is_click(4) && !super::is_click(7));
    }
}
