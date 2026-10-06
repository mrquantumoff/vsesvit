//! Extensions' private-window switch: off by default, on per extension, and synced.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Seq, TimeSource};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::extensions::private::ALLOWED_IN_PRIVATE;
use vsesvit_core::prefs::Scope;
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(device: u64) -> (Profile, TempDir) {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-ext-private-{}", uuid::Uuid::new_v4().simple())));
    let options = OpenOptions { time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000))), new_device_id: Some(DeviceId(device)), ..OpenOptions::default() };
    (Profile::open(&dir.0, options).unwrap(), dir)
}

fn id(s: &str) -> ExtensionId {
    ExtensionId::parse(s).unwrap()
}

fn uploaded_up_to(p: &mut Profile) -> Seq {
    p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().upto
}

#[test]
fn extensions_stay_out_of_private_windows_until_the_user_allows_one() {
    let (mut p, _dir) = open(2);
    let (a, b) = (id("a@vsesvit.test"), id("b@vsesvit.test"));
    assert!(!p.extensions().allowed_in_private(&a));
    assert!(!p.extensions().allowed_in_private(&b));

    p.extensions().set_allowed_in_private(&a, true).unwrap();
    assert!(p.extensions().allowed_in_private(&a));
    assert!(!p.extensions().allowed_in_private(&b), "the switch is per extension");

    let before = uploaded_up_to(&mut p);
    p.extensions().set_allowed_in_private(&a, true).unwrap();
    p.extensions().set_allowed_in_private(&b, false).unwrap();
    assert_eq!(uploaded_up_to(&mut p), before, "an unchanged switch writes nothing");

    p.extensions().set_allowed_in_private(&a, false).unwrap();
    assert!(!p.extensions().allowed_in_private(&a));
    assert!(uploaded_up_to(&mut p) > before);
}

#[test]
fn the_switch_syncs_to_another_device() {
    assert_eq!(ALLOWED_IN_PRIVATE.scope, Scope::Synced);
    let (mut first, _d1) = open(2);
    let (mut second, _d2) = open(3);
    let ext = id("aaaabbbbccccddddeeeeffffgggghhhh");
    first.extensions().set_allowed_in_private(&ext, true).unwrap();

    let upload = first.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap();
    assert_eq!(second.sync().apply(upload.records).unwrap().merged, 1);
    assert!(second.extensions().allowed_in_private(&ext), "on for the other device, which does not have the extension");

    second.extensions().set_allowed_in_private(&ext, false).unwrap();
    let upload = second.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap();
    first.sync().apply(upload.records).unwrap();
    assert!(!first.extensions().allowed_in_private(&ext));
}
