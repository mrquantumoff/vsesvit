//! Inject pointer/keyboard input into a (private) KWin through org_kde_kwin_fake_input.
//! KWin must run with KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 (restricted global).
//! Coordinates are KWin global coordinates (= X root coordinates for rootless Xwayland at scale 1).
//!
//!   kwin-input click X Y [left|right|middle]
//!   kwin-input move X Y
//!   kwin-input key ctrl+l | f11 | ctrl+shift+t | enter
//!   kwin-input type text
use anyhow::{Context as _, Result, bail};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle, globals::GlobalListContents, globals::registry_queue_init};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput as Fake;

struct S;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
    fn event(_: &mut S, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<S>) {}
}
impl Dispatch<Fake, ()> for S {
    fn event(_: &mut S, _: &Fake, _: <Fake as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<S>) {}
}

fn evdev(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    let letters = "qwertyuiop"; // 16..25
    let row2 = "asdfghjkl"; // 30..38
    let row3 = "zxcvbnm"; // 44..50
    Some(match n.as_str() {
        "ctrl" | "control" => 29,
        "shift" => 42,
        "alt" => 56,
        "super" | "meta" => 125,
        "enter" | "return" => 28,
        "esc" | "escape" => 1,
        "tab" => 15,
        "space" => 57,
        "backspace" => 14,
        "left" => 105,
        "right" => 106,
        "up" => 103,
        "down" => 108,
        "f1" => 59, "f2" => 60, "f3" => 61, "f4" => 62, "f5" => 63, "f6" => 64,
        "f7" => 65, "f8" => 66, "f9" => 67, "f10" => 68, "f11" => 87, "f12" => 88,
        "." => 52, "/" => 53, ":" => 39, "-" => 12, "1" => 2, "2" => 3, "3" => 4, "4" => 5, "5" => 6,
        "6" => 7, "7" => 8, "8" => 9, "9" => 10, "0" => 11,
        c if c.len() == 1 => {
            let ch = c.chars().next()?;
            if let Some(i) = letters.find(ch) { 16 + i as u32 }
            else if let Some(i) = row2.find(ch) { 30 + i as u32 }
            else if let Some(i) = row3.find(ch) { 44 + i as u32 }
            else { return None }
        }
        _ => return None,
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conn = Connection::connect_to_env().context("wayland connect")?;
    let (globals, mut queue) = registry_queue_init::<S>(&conn)?;
    let qh = queue.handle();
    let fake: Fake = globals.bind(&qh, 1..=5, ()).context("org_kde_kwin_fake_input not offered (KWIN_WAYLAND_NO_PERMISSION_CHECKS=1?)")?;
    fake.authenticate("vsesvit-spike".into(), "automated test".into());
    let flush = |q: &mut wayland_client::EventQueue<S>| -> Result<()> { q.roundtrip(&mut S)?; Ok(()) };
    let sleep = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
    let num = |i: usize| -> Result<f64> { Ok(args.get(i).context("arg")?.parse()?) };
    match args.first().map(String::as_str) {
        Some("move") => { fake.pointer_motion_absolute(num(1)?, num(2)?); flush(&mut queue)?; }
        Some("click") => {
            let b = match args.get(3).map(String::as_str) { Some("right") => 0x111, Some("middle") => 0x112, _ => 0x110 };
            fake.pointer_motion_absolute(num(1)?, num(2)?);
            flush(&mut queue)?; sleep(40);
            fake.button(b, 1); flush(&mut queue)?; sleep(40);
            fake.button(b, 0); flush(&mut queue)?;
        }
        Some("key") => {
            let codes: Vec<u32> = args.get(1).context("combo")?.split('+').map(|k| evdev(k).with_context(|| format!("key {k}"))).collect::<Result<_>>()?;
            for c in &codes { fake.keyboard_key(*c, 1); flush(&mut queue)?; sleep(15); }
            for c in codes.iter().rev() { fake.keyboard_key(*c, 0); flush(&mut queue)?; sleep(15); }
        }
        Some("type") => {
            for ch in args.get(1).context("text")?.chars() {
                let k = if ch == ' ' { "space".to_string() } else { ch.to_string() };
                let c = evdev(&k).with_context(|| format!("key {k}"))?;
                fake.keyboard_key(c, 1); flush(&mut queue)?; sleep(10);
                fake.keyboard_key(c, 0); flush(&mut queue)?; sleep(10);
            }
        }
        _ => bail!("usage: kwin-input move|click|key|type ..."),
    }
    Ok(())
}
