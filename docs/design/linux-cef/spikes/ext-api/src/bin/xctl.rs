//! Tiny X11 test driver (no xdotool/xwininfo on the host): XTEST input, focus,
//! window tree, screenshots.
//!
//!   xctl find <substring>          print xid + root geometry of a toplevel by WM_NAME
//!   xctl click X Y [button]        move + click at root coordinates
//!   xctl move X Y
//!   xctl key ctrl+l | F11 | Return  (combo of modifiers + one key)
//!   xctl type "text"
//!   xctl focus                     print the X input focus window
//!   xctl tree [xid]                print the window tree (default root)
//!   xctl shot out.png [xid]        capture (with children pasted) to PNG
use anyhow::{Context as _, Result, bail};
use vsesvit_spike::xwin::{XConn, write_png};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;

fn keysym(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => 0xffe3,
        "shift" => 0xffe1,
        "alt" => 0xffe9,
        "super" => 0xffeb,
        "return" | "enter" => 0xff0d,
        "escape" | "esc" => 0xff1b,
        "tab" => 0xff09,
        "backspace" => 0xff08,
        "delete" => 0xffff,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "space" => 0x20,
        f if f.starts_with('f') && f.len() > 1 && f[1..].parse::<u32>().is_ok() => 0xffbe + f[1..].parse::<u32>().unwrap() - 1,
        c if c.chars().count() == 1 => c.chars().next().unwrap() as u32,
        _ => return None,
    })
}

struct Kbd {
    min: u8,
    per: u8,
    syms: Vec<u32>,
}

impl Kbd {
    fn new(x: &XConn) -> Result<Self> {
        let s = x.conn.setup();
        let (min, max) = (s.min_keycode, s.max_keycode);
        let m = x.conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
        Ok(Self { min, per: m.keysyms_per_keycode, syms: m.keysyms })
    }
    /// (keycode, needs_shift)
    fn code(&self, sym: u32) -> Option<(u8, bool)> {
        for (i, chunk) in self.syms.chunks(self.per as usize).enumerate() {
            if let Some(col) = chunk.iter().position(|&s| s == sym) {
                if col <= 1 {
                    return Some((self.min + i as u8, col == 1));
                }
            }
        }
        None
    }
}

/// Absolute XTEST motion is dropped by Xwayland's EI emulation under KWin's virtual
/// backend; relative motion works. Move relatively from the current pointer position.
fn move_to(x: &XConn, tx: i16, ty: i16) -> Result<()> {
    for _ in 0..4 {
        let q = x.conn.query_pointer(x.root)?.reply()?;
        let (dx, dy) = (tx - q.root_x, ty - q.root_y);
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        x.conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 1, x11rb::CURRENT_TIME, x11rb::NONE, dx, dy, 0)?;
        x.conn.sync()?;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

fn fake(x: &XConn, kind: u8, detail: u8, rx: i16, ry: i16) -> Result<()> {
    x.conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, x.root, rx, ry, 0)?;
    x.conn.sync()?;
    Ok(())
}

fn press_combo(x: &XConn, combo: &str) -> Result<()> {
    let kbd = Kbd::new(x)?;
    let parts: Vec<&str> = combo.split('+').collect();
    let mut codes = Vec::new();
    for p in &parts {
        let sym = keysym(p).with_context(|| format!("unknown key {p}"))?;
        let (code, shift) = kbd.code(sym).with_context(|| format!("no keycode for {p}"))?;
        if shift && parts.len() == 1 {
            codes.push(kbd.code(0xffe1).unwrap().0);
        }
        codes.push(code);
    }
    for &c in &codes {
        fake(x, xproto::KEY_PRESS_EVENT, c, 0, 0)?;
    }
    for &c in codes.iter().rev() {
        fake(x, xproto::KEY_RELEASE_EVENT, c, 0, 0)?;
    }
    Ok(())
}

fn find(x: &XConn, needle: &str) -> Result<Vec<(u32, String)>> {
    let mut found = Vec::new();
    let mut stack = vec![x.root];
    while let Some(w) = stack.pop() {
        let t = x.conn.query_tree(w)?.reply()?;
        for c in t.children {
            let name = x
                .conn
                .get_property(false, c, xproto::AtomEnum::WM_NAME, xproto::AtomEnum::ANY, 0, 256)?
                .reply()
                .map(|p| String::from_utf8_lossy(&p.value).into_owned())
                .unwrap_or_default();
            if !name.is_empty() && name.contains(needle) {
                found.push((c, name));
            }
            stack.push(c);
        }
    }
    Ok(found)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let x = XConn::connect()?;
    let num = |i: usize| -> Result<i16> { Ok(args.get(i).context("missing arg")?.parse()?) };
    match args.first().map(String::as_str) {
        Some("find") => {
            for (w, name) in find(&x, args.get(1).map(String::as_str).unwrap_or(""))? {
                let (rx, ry) = x.translate_to_root(w).unwrap_or_default();
                let g = x.geometry(w).unwrap_or(vsesvit_spike::xwin::XRect { x: 0, y: 0, w: 0, h: 0 });
                println!("0x{w:x} {}x{}+{rx}+{ry} {name:?}", g.w, g.h);
            }
        }
        Some("move") => move_to(&x, num(1)?, num(2)?)?,
        Some("click") => {
            let b: u8 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
            move_to(&x, num(1)?, num(2)?)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
            fake(&x, xproto::BUTTON_PRESS_EVENT, b, 0, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
            fake(&x, xproto::BUTTON_RELEASE_EVENT, b, 0, 0)?;
        }
        Some("key") => press_combo(&x, args.get(1).context("key combo")?)?,
        Some("type") => {
            for ch in args.get(1).context("text")?.chars() {
                let s = if ch == ' ' { "space".to_string() } else { ch.to_string() };
                press_combo(&x, &s)?;
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
        Some("focus") => println!("0x{:x}", x.focus()),
        Some(op @ ("raise" | "lower" | "unmap" | "setfocus")) => {
            let w = u32::from_str_radix(args.get(1).context("xid")?.trim_start_matches("0x"), 16)?;
            match op {
                "raise" => { x.conn.configure_window(w, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE))?; }
                "lower" => { x.conn.configure_window(w, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::BELOW))?; }
                "unmap" => { x.conn.unmap_window(w)?; }
                _ => { x.conn.set_input_focus(xproto::InputFocus::PARENT, w, x11rb::CURRENT_TIME)?; }
            }
            x.conn.flush()?;
            println!("{op} 0x{w:x}");
        }
        Some("close") => {
            // What a WM's close button does: WM_PROTOCOLS / WM_DELETE_WINDOW client message.
            let w = u32::from_str_radix(args.get(1).context("xid")?.trim_start_matches("0x"), 16)?;
            let protocols = x.conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
            let delete = x.conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
            let ev = xproto::ClientMessageEvent::new(32, w, protocols, [delete, 0, 0, 0, 0]);
            x.conn.send_event(false, w, xproto::EventMask::NO_EVENT, ev)?;
            x.conn.flush()?;
            println!("sent WM_DELETE_WINDOW to 0x{w:x}");
        }
        Some("grabtest") => {
            let r = x.conn.grab_pointer(false, x.root, xproto::EventMask::BUTTON_PRESS, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME)?.reply()?;
            println!("grab_pointer -> {:?}", r.status);
            x.conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
            let k = x.conn.grab_keyboard(false, x.root, x11rb::CURRENT_TIME, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC)?.reply()?;
            println!("grab_keyboard -> {:?}", k.status);
            x.conn.ungrab_keyboard(x11rb::CURRENT_TIME)?;
            let q = x.conn.query_pointer(x.root)?.reply()?;
            println!("pointer at {},{} child=0x{:x} mask={:?}", q.root_x, q.root_y, q.child, q.mask);
            x.conn.flush()?;
        }
        Some("tree") => {
            let w = args.get(1).map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16)).transpose()?.unwrap_or(x.root);
            let mut out = Vec::new();
            x.tree(w, 0, &mut out);
            println!("{}", out.join("\n"));
        }
        Some("shotscreen") => {
            // Compose every viewable toplevel (stacking order) on a root-sized canvas:
            // rootless Xwayland's root window cannot be read with GetImage (BadMatch).
            let path = args.get(1).context("path")?;
            let rg = x.conn.get_geometry(x.root)?.reply()?;
            let (cw, ch) = (rg.width as u32, rg.height as u32);
            let mut canvas = vec![0u8; (cw * ch * 4) as usize];
            for w in x.conn.query_tree(x.root)?.reply()?.children {
                let Ok(a) = x.conn.get_window_attributes(w)?.reply() else { continue };
                if a.map_state != xproto::MapState::VIEWABLE || a.class == xproto::WindowClass::INPUT_ONLY { continue; }
                let Ok(g) = x.conn.get_geometry(w)?.reply() else { continue };
                if g.width <= 1 || g.depth == 0 { continue; }
                let Ok((ww, wh, px)) = x.capture_mode(w, false) else { continue };
                for yy in 0..wh as i32 { for xx in 0..ww as i32 {
                    let (dx, dy) = (g.x as i32 + xx, g.y as i32 + yy);
                    if dx < 0 || dy < 0 || dx >= cw as i32 || dy >= ch as i32 { continue; }
                    let s = ((yy as u32 * ww + xx as u32) * 4) as usize; let d = ((dy as u32 * cw + dx as u32) * 4) as usize;
                    canvas[d..d + 4].copy_from_slice(&px[s..s + 4]);
                }}
            }
            write_png(path, cw, ch, &canvas)?;
            println!("{cw}x{ch} -> {path}");
        }
        Some("shot") => {
            let path = args.get(1).context("path")?;
            let w = args.get(2).map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16)).transpose()?.unwrap_or(x.root);
            let (wd, ht, px) = x.capture(w)?;
            write_png(path, wd, ht, &px)?;
            println!("{wd}x{ht} -> {path}");
        }
        Some("keywatch") => {
            // Select KeyPress on the given windows (non-exclusive) and print what arrives.
            let secs: u64 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(5);
            for w in args[2..].iter().filter_map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
                let aux = xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::KEY_PRESS);
                x.conn.change_window_attributes(w, &aux)?;
            }
            x.conn.flush()?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            while std::time::Instant::now() < end {
                while let Some(ev) = x.conn.poll_for_event()? {
                    match ev {
                        x11rb::protocol::Event::KeyPress(e) => println!("KeyPress code={} event=0x{:x} child=0x{:x} root=0x{:x}", e.detail, e.event, e.child, e.root),
                        x11rb::protocol::Event::Error(e) => println!("error {:?}", e.error_kind),
                        _ => {}
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Some("watch") => {
            // Print focus changes on the given windows and _NET_ACTIVE_WINDOW traffic on root.
            let secs: u64 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(5);
            let wins: Vec<u32> = args[2..].iter().filter_map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).collect();
            let net_active = x.conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
            let aux = xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::SUBSTRUCTURE_NOTIFY | xproto::EventMask::PROPERTY_CHANGE | xproto::EventMask::FOCUS_CHANGE);
            x.conn.change_window_attributes(x.root, &aux)?;
            for w in &wins {
                let aux = xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::FOCUS_CHANGE);
                x.conn.change_window_attributes(*w, &aux)?;
            }
            x.conn.flush()?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            let t0 = std::time::Instant::now();
            while std::time::Instant::now() < end {
                while let Some(ev) = x.conn.poll_for_event()? {
                    use x11rb::protocol::Event as E;
                    let t = t0.elapsed().as_secs_f32();
                    match ev {
                        E::FocusIn(e) => println!("{t:7.3} FocusIn  win=0x{:x} mode={:?} detail={:?}", e.event, e.mode, e.detail),
                        E::FocusOut(e) => println!("{t:7.3} FocusOut win=0x{:x} mode={:?} detail={:?}", e.event, e.mode, e.detail),
                        E::ClientMessage(e) if e.type_ == net_active => println!("{t:7.3} ClientMessage _NET_ACTIVE_WINDOW for 0x{:x} data={:?}", e.window, &e.data.as_data32()[..3]),
                        E::PropertyNotify(e) if e.atom == net_active => {
                            let v = x.conn.get_property(false, x.root, net_active, xproto::AtomEnum::WINDOW, 0, 1)?.reply()?;
                            println!("{t:7.3} root _NET_ACTIVE_WINDOW = {:?}", v.value32().and_then(|mut i| i.next()).map(|w| format!("0x{w:x}")));
                        }
                        _ => {}
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        _ => bail!("usage: xctl find|move|click|key|type|focus|tree|shot|watch ..."),
    }
    Ok(())
}
