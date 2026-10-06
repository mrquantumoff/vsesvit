//! Memory Saver's sweep as the profile sets it up: on by default, Balanced's delay, off when the
//! user turns it off, and tabs of sites allowed to notify kept awake.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use vsesvit_core::crdt::{DeviceId, TimeSource};
use vsesvit_core::memory_saver::{IdleClock, MemorySaverMode, Sweep, TabActivity};
use vsesvit_core::permissions::{Origin, Permission, Setting};
use vsesvit_core::prefs::keys;
use vsesvit_core::{OpenOptions, Profile};

const HOUR: Duration = Duration::from_secs(60 * 60);

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open() -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-memory-saver-{}", uuid::Uuid::new_v4())));
    let p = Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))), new_device_id: Some(DeviceId(1)), ..OpenOptions::default() },
    )
    .unwrap();
    (p, dir)
}

fn sleeps_after(p: &mut Profile, url: &str, idle: Duration) -> bool {
    let start = Instant::now();
    let mut clock = IdleClock::new(start);
    Sweep::new(p, start + idle).sleeps(&TabActivity { url, ..TabActivity::default() }, &mut clock)
}

#[test]
fn on_by_default_after_balanceds_four_hours() {
    let (mut p, _dir) = open();
    assert!(p.prefs().get(&keys::MEMORY_SAVER));
    assert_eq!(p.prefs().get(&keys::MEMORY_SAVER_MODE), MemorySaverMode::Balanced);
    assert!(!sleeps_after(&mut p, "https://example.com/", 3 * HOUR));
    assert!(sleeps_after(&mut p, "https://example.com/", 4 * HOUR));

    p.prefs().set(&keys::MEMORY_SAVER_MODE, &MemorySaverMode::Maximum).unwrap();
    assert!(sleeps_after(&mut p, "https://example.com/", 2 * HOUR));
    p.prefs().set(&keys::MEMORY_SAVER, &false).unwrap();
    assert!(!sleeps_after(&mut p, "https://example.com/", 100 * HOUR));
}

#[test]
fn tabs_of_sites_allowed_to_notify_stay_awake() {
    let (mut p, _dir) = open();
    let chat = Origin::parse("https://chat.example").unwrap();
    p.site_permissions().set(&chat, Permission::Notifications, Some(Setting::Allow)).unwrap();
    let blocked = Origin::parse("https://news.example").unwrap();
    p.site_permissions().set(&blocked, Permission::Notifications, Some(Setting::Block)).unwrap();
    assert!(!sleeps_after(&mut p, "https://chat.example/inbox", 10 * HOUR));
    assert!(sleeps_after(&mut p, "https://news.example/today", 10 * HOUR));
}
