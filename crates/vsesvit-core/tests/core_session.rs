//! Session save/restore, restore-blob handling, and other devices' tabs.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Hlc, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::session::{DeviceSessionRecord, SessionSnapshot, TabId, TabSnapshot, WindowSnapshot};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{OpenOptions, Profile, Url};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T0: u64 = 1_780_000_000_000;
const DAY: u64 = 86_400_000;

fn open(dir: &TempDir, time: &Rc<Cell<u64>>) -> Profile {
    Profile::open(
        &dir.0,
        OpenOptions { time: TimeSource::Manual(time.clone()), new_device_id: Some(DeviceId(5)), ..OpenOptions::default() },
    )
    .unwrap()
}

fn tab(n: u128, blob: Option<Vec<u8>>) -> TabSnapshot {
    TabSnapshot {
        id: TabId(uuid::Uuid::from_u128(n)),
        url: Url::parse(&format!("https://t{n}.example/")).unwrap(),
        title: format!("tab {n}"),
        pinned: n == 1,
        last_active_ms: n as i64,
        group: None,
        restore_state: blob,
    }
}

fn snapshot(tabs: Vec<TabSnapshot>) -> SessionSnapshot {
    SessionSnapshot {
        device_name: "laptop".into(),
        windows: vec![WindowSnapshot { tabs, active_tab: 0, bounds: Some((1, 2, 3, 4)), maximized: true }],
        active_window: 0,
    }
}

fn upto(p: &mut Profile) -> Seq {
    p.sync().changes_since(Kind::Sessions, Seq::ZERO, usize::MAX).unwrap().upto
}

#[test]
fn save_and_restore_round_trip_with_blobs() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-sess-{}", uuid::Uuid::new_v4())));
    let time = Rc::new(Cell::new(T0));
    {
        let mut p = open(&dir, &time);
        assert!(p.session().restore().unwrap().is_none());
        p.session().save(&snapshot(vec![tab(1, Some(vec![9, 9])), tab(2, None)])).unwrap();
    }
    let mut p = open(&dir, &time);
    let restored = p.session().restore().unwrap().unwrap();
    assert_eq!(restored, snapshot(vec![tab(1, None), tab(2, None)]), "equality ignores blobs");
    assert_eq!(restored.windows[0].tabs[0].restore_state.as_deref(), Some(&[9u8, 9][..]));
    assert_eq!(restored.windows[0].tabs[1].restore_state, None);
    assert_eq!(restored.windows[0].bounds, Some((1, 2, 3, 4)));

    let body = p.sync().changes_since(Kind::Sessions, Seq::ZERO, usize::MAX).unwrap().records.remove(0);
    assert_eq!(body.id, DeviceId(5).to_hex());
    assert!(!String::from_utf8(body.body).unwrap().contains("restore_state"));
}

#[test]
fn a_blob_only_change_mints_no_stamp_or_seq() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-sess-{}", uuid::Uuid::new_v4())));
    let time = Rc::new(Cell::new(T0));
    let mut p = open(&dir, &time);
    p.session().save(&snapshot(vec![tab(1, Some(vec![1]))])).unwrap();
    let first = upto(&mut p);
    p.session().save(&snapshot(vec![tab(1, Some(vec![2]))])).unwrap();
    assert_eq!(upto(&mut p), first, "restore state is local and not part of the record");
    assert_eq!(p.session().restore().unwrap().unwrap().windows[0].tabs[0].restore_state, Some(vec![2]));
    p.session().save(&snapshot(vec![tab(1, Some(vec![2]))])).unwrap();
    assert_eq!(upto(&mut p), first, "an identical save is a no-op");
    p.session().save(&snapshot(vec![tab(1, Some(vec![2])), tab(3, None)])).unwrap();
    assert!(upto(&mut p) > first);
}

#[test]
fn other_devices_are_listed_newest_first_and_expire_after_14_days() {
    let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-sess-{}", uuid::Uuid::new_v4())));
    let time = Rc::new(Cell::new(T0 + 30 * DAY));
    let mut p = open(&dir, &time);
    p.session().save(&snapshot(vec![tab(1, None)])).unwrap();
    let remote = |device: u64, name: &str, age_days: u64| {
        let rec = DeviceSessionRecord {
            device: DeviceId(device),
            session: Lww::new(
                Some(SessionSnapshot { device_name: name.into(), windows: vec![], active_window: 0 }),
                Stamp { hlc: Hlc((T0 + (30 - age_days) * DAY) << 16), device: DeviceId(device) },
            ),
        };
        WireRecord { kind: Kind::Sessions, id: DeviceId(device).to_hex(), body: serde_json::to_vec(&rec).unwrap() }
    };
    let forgotten = DeviceSessionRecord { device: DeviceId(40), session: Lww::new(None, Stamp { hlc: Hlc(T0 << 16), device: DeviceId(40) }) };
    let report = p
        .sync()
        .apply(vec![
            remote(10, "phone", 1),
            remote(20, "desk", 0),
            remote(30, "old", 20),
            WireRecord { kind: Kind::Sessions, id: DeviceId(40).to_hex(), body: serde_json::to_vec(&forgotten).unwrap() },
        ])
        .unwrap();
    assert_eq!(report.merged, 4);
    assert!(report.changed.sessions);
    let others = p.session().other_devices().unwrap();
    assert_eq!(others.iter().map(|d| d.device_name.as_str()).collect::<Vec<_>>(), ["desk", "phone"]);
    assert_eq!(others[1].updated_ms as u64, T0 + 29 * DAY);
    // our own row is untouched by remote records for other devices
    assert_eq!(p.session().restore().unwrap().unwrap().device_name, "laptop");
}
