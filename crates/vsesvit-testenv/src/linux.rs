//! Owned by W0.5.
//! Linux answers for the guard (the X lock file, `/proc`, the root window's token) and `PrivateInput`,
//! the one place XTEST is called (design section 7.3).

use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::envfile::ROOT_PROPERTY;

fn number(display: &str) -> Result<u32, String> {
    display
        .strip_prefix(':')
        .and_then(|n| n.split('.').next())
        .and_then(|n| n.parse().ok())
        .ok_or(format!("{display:?} is not a local display `:N`"))
}

pub(crate) fn lock_pid(display: &str) -> Result<u32, String> {
    let path = format!("/tmp/.X{}-lock", number(display)?);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    text.trim().parse().map_err(|_| format!("{path} holds {text:?}, not a pid"))
}

pub(crate) fn exe_name(pid: u32) -> Result<String, String> {
    let link = format!("/proc/{pid}/exe");
    let exe = std::fs::read_link(&link).map_err(|e| format!("{link}: {e}"))?;
    exe.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or(format!("{link} has no file name"))
}

fn connect(display: &str) -> Result<(RustConnection, usize), String> {
    x11rb::connect(Some(display)).map_err(|e| format!("connecting to {display}: {e}"))
}

pub(crate) fn root_token(display: &str) -> Result<Option<String>, String> {
    let (conn, screen) = connect(display)?;
    let root = conn.setup().roots.get(screen).ok_or("the server has no such screen")?.root;
    let failed = |e: &dyn std::fmt::Display| format!("reading {ROOT_PROPERTY} on {display}: {e}");
    let atom = conn
        .intern_atom(true, ROOT_PROPERTY.as_bytes())
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?
        .atom;
    if atom == x11rb::NONE {
        return Ok(None);
    }
    let reply = conn
        .get_property(false, root, atom, AtomEnum::STRING, 0, 64)
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?;
    if reply.value.is_empty() {
        return Ok(None);
    }
    String::from_utf8(reply.value).map(Some).map_err(|_| failed(&"not UTF-8"))
}

/// Key input into the private X server, through XTEST. Only `TestEnv::input` builds one, after
/// the guard confirmed the display is the launcher's Xvfb.
pub struct PrivateInput {
    conn: RustConnection,
}

impl PrivateInput {
    pub(crate) fn open(display: &str) -> Result<PrivateInput, String> {
        let (conn, _) = connect(display)?;
        conn.extension_information(xtest::X11_EXTENSION_NAME)
            .map_err(|e| e.to_string())?
            .ok_or(format!("{display} has no XTEST"))?;
        Ok(PrivateInput { conn })
    }

    /// Presses and releases `keycode`; the window with the input focus gets both.
    pub fn key(&self, keycode: u8) -> Result<(), String> {
        for kind in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
            #[allow(clippy::disallowed_methods)] // the one XTEST call, inside the private Xvfb (design section 7.3)
            let sent = self.conn.xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, x11rb::NONE, 0, 0, 0);
            sent.map_err(|e| e.to_string())?.check().map_err(|e| format!("XTEST key {keycode}: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn display_numbers() {
        assert_eq!(super::number(":101"), Ok(101));
        assert_eq!(super::number(":3.0"), Ok(3));
        assert!(super::number("localhost:1").is_err());
        assert!(super::number("").is_err());
    }
}
