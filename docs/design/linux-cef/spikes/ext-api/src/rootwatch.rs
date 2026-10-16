//! Test instrumentation: a thread with its own X connection that logs every toplevel
//! (child of the root window) being created, mapped, unmapped, reparented or destroyed,
//! ClientMessages sent to the root (_NET_ACTIVE_WINDOW requests reach SubstructureNotify
//! selectors), and changes of the X input focus. Used to see Chrome-native windows that
//! extensions open, and how long they are visible.
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

fn name(conn: &RustConnection, w: u32) -> String {
    let get = |atom: u32| {
        conn.get_property(false, w, atom, xproto::AtomEnum::ANY, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|p| String::from_utf8_lossy(&p.value).replace('\0', "|"))
            .unwrap_or_default()
    };
    format!("name={:?} class={:?}", get(xproto::AtomEnum::WM_NAME.into()), get(xproto::AtomEnum::WM_CLASS.into()))
}

fn atom_name(conn: &RustConnection, a: u32) -> String {
    conn.get_atom_name(a).ok().and_then(|c| c.reply().ok()).map(|r| String::from_utf8_lossy(&r.name).into_owned()).unwrap_or_default()
}

pub fn spawn() {
    if std::env::var("SPIKE_NO_ROOTWATCH").is_ok() {
        return;
    }
    let _ = std::thread::Builder::new().name("rootwatch".into()).spawn(|| {
        let Ok((conn, screen)) = x11rb::connect(None) else { return };
        let root = conn.setup().roots[screen].root;
        let mask = xproto::EventMask::SUBSTRUCTURE_NOTIFY;
        if conn.change_window_attributes(root, &xproto::ChangeWindowAttributesAux::new().event_mask(mask)).is_err() {
            return;
        }
        let _ = conn.flush();
        let mut last_focus = 0u32;
        loop {
            while let Ok(Some(ev)) = conn.poll_for_event() {
                match ev {
                    Event::CreateNotify(e) => crate::log(&format!("rootwatch: CreateNotify 0x{:x} {}x{}+{}+{} override={}", e.window, e.width, e.height, e.x, e.y, e.override_redirect)),
                    Event::MapNotify(e) => {
                        let g = conn.get_geometry(e.window).ok().and_then(|c| c.reply().ok()).map(|g| format!("{}x{}+{}+{}", g.width, g.height, g.x, g.y)).unwrap_or_default();
                        crate::log(&format!("rootwatch: MapNotify 0x{:x} {g} override={} {}", e.window, e.override_redirect, name(&conn, e.window)))
                    }
                    Event::UnmapNotify(e) => crate::log(&format!("rootwatch: UnmapNotify 0x{:x}", e.window)),
                    Event::DestroyNotify(e) => crate::log(&format!("rootwatch: DestroyNotify 0x{:x}", e.window)),
                    Event::ReparentNotify(e) => crate::log(&format!("rootwatch: ReparentNotify 0x{:x} -> parent 0x{:x}", e.window, e.parent)),
                    Event::ClientMessage(e) => {
                        let d = e.data.as_data32();
                        crate::log(&format!("rootwatch: ClientMessage {} window=0x{:x} data={:?}", atom_name(&conn, e.type_), e.window, d))
                    }
                    _ => {}
                }
            }
            let focus = conn.get_input_focus().ok().and_then(|c| c.reply().ok()).map(|r| r.focus).unwrap_or(0);
            if focus != last_focus {
                crate::log(&format!("rootwatch: X focus 0x{last_focus:x} -> 0x{focus:x}"));
                last_focus = focus;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    });
}
