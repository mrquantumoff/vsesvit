//! Linux pieces the shared glue calls (`crate::platform::*`) and the startup guard.
use crate::xwin::XConn;
use std::sync::OnceLock;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _};

static X: OnceLock<XConn> = OnceLock::new();

fn x() -> &'static XConn {
    X.get_or_init(|| XConn::connect().expect("x11rb"))
}

/// The root-window property start-xvfb.sh sets on the spike's private Xvfb.
pub const TOKEN_PROP: &str = "_VSESVIT_SPIKE_TOKEN";
/// The only display the spike opens (start-xvfb.sh).
pub const PRIVATE_DISPLAY: &str = ":94";

/// Fail closed: Ok only on an X server whose root carries `_VSESVIT_SPIKE_TOKEN` equal to
/// `SPIKE_XTOKEN` (written by start-xvfb.sh into a file only the scripts read), and with no
/// Wayland display in the environment. Any other display (WSLg's :0, a desktop) has no token.
pub fn guard() -> Result<String, String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Err("WAYLAND_DISPLAY is set".into());
    }
    let want = std::env::var("SPIKE_XTOKEN").map_err(|_| "SPIKE_XTOKEN unset".to_string())?;
    if want.len() < 16 {
        return Err("SPIKE_XTOKEN too short".into());
    }
    let dpy = std::env::var("DISPLAY").unwrap_or_default();
    // Checked before connecting, so no other display (WSLg's :0) is ever even opened.
    if dpy != PRIVATE_DISPLAY {
        return Err(format!("DISPLAY {dpy:?} is not the spike's {PRIVATE_DISPLAY}"));
    }
    let (conn, screen) = x11rb::connect(None).map_err(|e| format!("connect {dpy}: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let atom = conn.intern_atom(true, TOKEN_PROP.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
    if atom == 0 {
        return Err(format!("display {dpy} has no {TOKEN_PROP}"));
    }
    let p = conn.get_property(false, root, atom, xproto::AtomEnum::ANY, 0, 64).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let got = String::from_utf8_lossy(&p.value).to_string();
    if got != want {
        return Err(format!("display {dpy} token mismatch"));
    }
    let vendor = String::from_utf8_lossy(conn.setup().vendor.as_slice()).to_string();
    Ok(format!("display={dpy} vendor={vendor:?} token ok"))
}

pub fn is_shown(handle: u64) -> bool {
    if handle == 0 {
        return false;
    }
    x().conn.get_window_attributes(handle as u32).ok().and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state != xproto::MapState::UNMAPPED)
}

/// Unmap a Chrome-created toplevel and park it in the hidden host, synchronously.
pub fn neutralize(handle: u64, hidden: u64) -> Result<String, String> {
    let xc = x();
    let w = handle as u32;
    if w == 0 {
        return Err("no window".into());
    }
    let parent = xc.parent_of(w).unwrap_or(0);
    let was = is_shown(handle);
    let _ = xc.conn.unmap_window(w);
    if hidden != 0 {
        xc.reparent_only(w, hidden as u32, 0, 0).map_err(|e| e.to_string())?;
    }
    let _ = xc.conn.flush();
    Ok(format!("was_mapped={was} old_parent=0x{parent:x} toplevel={} new_parent=0x{:x}", parent == xc.root, xc.parent_of(w).unwrap_or(0)))
}

/// An unmapped 24-bit window under the root: parent of hidden browsers and neutralized windows.
pub fn create_hidden_host() -> u32 {
    x().create_holder().expect("hidden host")
}
