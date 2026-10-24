//! Owned by W1.1.
//! X keyboard focus inside one of our toplevels (spike-core section 7). Chrome's keyboard goes to the
//! focus proxy the patched GPUI creates in every toplevel, never to the toplevel itself: with focus
//! on an ancestor, X delivers keys to the deepest window under the pointer, which is the page.

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _};

use super::XConn;

/// `WM_NAME` of the patched GPUI's InputOnly proxy (third_party/gpui-pre-linux.patch).
pub(crate) const PROXY_NAME: &[u8] = b"gpui-focus-proxy";
/// Deeper than any window tree we build (toplevel, host, page, Chromium's own child).
const MAX_DEPTH: usize = 32;

/// The focus proxy among `toplevel`'s children.
pub(crate) fn proxy_of(x: &XConn, toplevel: xproto::Window) -> Option<xproto::Window> {
    x.children(toplevel).into_iter().find(|&child| is_proxy(x, child))
}

pub(crate) fn is_proxy(x: &XConn, xid: xproto::Window) -> bool {
    x.conn
        .get_property(false, xid, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::ANY, 0, 8)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|p| p.value == PROXY_NAME)
}

/// The window with the X input focus; `None` (0) and `PointerRoot` (1) are not windows.
pub(crate) fn current(x: &XConn) -> xproto::Window {
    x.conn.get_input_focus().ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.focus)
}

/// Is `w` `ancestor` or one of its descendants?
pub(crate) fn is_within(x: &XConn, mut w: xproto::Window, ancestor: xproto::Window) -> bool {
    for _ in 0..MAX_DEPTH {
        if w == ancestor {
            return true;
        }
        if w == x.root || w <= 1 {
            return false;
        }
        match x.parent(w) {
            Some(parent) => w = parent,
            None => return false,
        }
    }
    false
}

/// Checked: an unviewable or destroyed window is an error, not a silent no-op.
pub(crate) fn set(x: &XConn, w: xproto::Window) -> Result<(), String> {
    let cookie = x.conn.set_input_focus(xproto::InputFocus::PARENT, w, x11rb::CURRENT_TIME).map_err(|e| e.to_string())?;
    let result = cookie.check().map_err(|e| e.to_string());
    let _ = x.conn.flush();
    result
}
