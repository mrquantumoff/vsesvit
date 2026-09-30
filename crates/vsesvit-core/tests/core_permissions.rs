//! Site permissions: stored settings, decisions, prompt answers, one-time grants, the prompt's
//! words, the in-use indicator, the v5 migration and sync.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use vsesvit_core::crdt::{DeviceId, Hlc, Lww, Seq, Stamp, TimeSource};
use vsesvit_core::permissions::{
    prompt, Answer, Capturing, Decision, Origin, Permission, Setting, SitePermissionRecord, SiteSetting, TabGrants,
};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{Error, OpenOptions, Profile, Url};

use Permission::{Camera, ClipboardRead, Location, Microphone, Midi, Notifications, ScreenShare};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-perm-{}", uuid::Uuid::new_v4())))
}

fn open_at(dir: &TempDir, device: u64) -> Profile {
    Profile::open(
        &dir.0,
        OpenOptions {
            time: TimeSource::Manual(Rc::new(Cell::new(1_780_000_000_000 + device))),
            new_device_id: Some(DeviceId(device)),
            ..OpenOptions::default()
        },
    )
    .unwrap()
}

fn open(device: u64) -> (Profile, TempDir) {
    let dir = tmp();
    (open_at(&dir, device), dir)
}

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap()
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn exported(p: &mut Profile) -> Vec<WireRecord> {
    p.sync().changes_since(Kind::SitePermissions, Seq::ZERO, usize::MAX).unwrap().records
}

fn upto(p: &mut Profile) -> Seq {
    p.sync().changes_since(Kind::SitePermissions, Seq::ZERO, usize::MAX).unwrap().upto
}

#[test]
fn origins_are_normalized_and_shown_by_host() {
    let meet = origin("HTTPS://Meet.Example.com:443/call?id=1#x");
    assert_eq!(meet.as_str(), "https://meet.example.com");
    assert_eq!(meet.host_for_display(), "meet.example.com");
    assert_eq!(origin("https://meet.example.com"), meet, "an origin string parses to itself");
    assert_eq!(Origin::of(&url("https://meet.example.com/other")), Some(meet.clone()));

    let local = origin("http://localhost:8080/app");
    assert_eq!(local.as_str(), "http://localhost:8080");
    assert_eq!(local.host_for_display(), "localhost:8080");
    assert_eq!(origin("http://[::1]:3000/").host_for_display(), "[::1]:3000");
    assert_eq!(origin("https://xn--e1afmkfd.xn--j1amh/").host_for_display(), "пример.укр");

    for opaque in ["file:///C:/page.html", "data:text/html,hi", "about:blank"] {
        assert_eq!(Origin::of(&url(opaque)), None, "{opaque}");
    }
    assert_eq!(Origin::parse("not a url"), None);

    assert_eq!(serde_json::to_string(&meet).unwrap(), "\"https://meet.example.com\"");
    assert_eq!(serde_json::from_str::<Origin>("\"https://meet.example.com\"").unwrap(), meet);
    for bad in ["\"https://Meet.example.com\"", "\"https://meet.example.com/\"", "\"file:///x\"", "\"meet\""] {
        assert!(serde_json::from_str::<Origin>(bad).is_err(), "{bad} is not a canonical origin");
    }
}

#[test]
fn permission_keys_and_labels() {
    let keys: Vec<&str> = Permission::ALL.iter().map(|p| p.key()).collect();
    assert_eq!(keys, ["camera", "microphone", "location", "notifications", "screen_share", "clipboard_read", "midi"]);
    for &p in Permission::ALL {
        assert_eq!(Permission::from_key(p.key()), Some(p));
        assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.key()));
    }
    assert_eq!(Permission::from_key("usb"), None);
    let labels: Vec<&str> = Permission::ALL.iter().map(|p| p.label()).collect();
    assert_eq!(labels, ["Camera", "Microphone", "Location", "Notifications", "Screen sharing", "Clipboard", "MIDI devices"]);
    assert!(Permission::ALL.iter().all(|p| p.remembers_allow() == (*p != ScreenShare)));
}

#[test]
fn get_set_and_reset() {
    let (mut p, _dir) = open(1);
    let site = origin("https://meet.example.com");
    assert_eq!(p.site_permissions().get(&site, Camera), None);

    p.site_permissions().set(&site, Location, Some(Setting::Block)).unwrap();
    p.site_permissions().set(&site, Camera, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&site, Microphone, Some(Setting::Allow)).unwrap();
    assert_eq!(p.site_permissions().get(&site, Camera), Some(Setting::Allow));
    assert_eq!(
        p.site_permissions().for_site(&site),
        [(Camera, Setting::Allow), (Microphone, Setting::Allow), (Location, Setting::Block)],
        "Permission::ALL order"
    );

    let before = upto(&mut p);
    p.site_permissions().set(&site, Camera, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&site, Midi, None).unwrap();
    assert_eq!(upto(&mut p), before, "unchanged settings mint nothing");
    assert_eq!(exported(&mut p).len(), 3, "asking about a never-set permission stores nothing");

    p.site_permissions().set(&site, Microphone, None).unwrap();
    assert_eq!(p.site_permissions().get(&site, Microphone), None);

    let other = origin("https://other.example");
    p.site_permissions().set(&other, Notifications, Some(Setting::Block)).unwrap();
    let before = upto(&mut p);
    p.site_permissions().reset_site(&site).unwrap();
    assert!(p.site_permissions().for_site(&site).is_empty());
    assert_eq!(p.site_permissions().for_site(&other), [(Notifications, Setting::Block)], "other sites are kept");

    let reset = p.sync().changes_since(Kind::SitePermissions, before, usize::MAX).unwrap().records;
    assert_eq!(reset.len(), 2, "one transaction re-marks the two settings still set");
    let bodies: Vec<SitePermissionRecord> = reset.iter().map(|w| serde_json::from_slice(&w.body).unwrap()).collect();
    assert!(bodies.iter().all(|r| r.setting.v.is_none()), "a reset is a stored `None`, so it syncs");
    assert_eq!(bodies[0].setting.at, bodies[1].setting.at, "one stamp");
}

#[test]
fn all_is_sorted_by_origin_then_permission() {
    let (mut p, _dir) = open(1);
    let b = origin("https://b.example");
    let a = origin("https://a.example");
    p.site_permissions().set(&b, Midi, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&a, Notifications, Some(Setting::Block)).unwrap();
    p.site_permissions().set(&b, Camera, Some(Setting::Block)).unwrap();
    p.site_permissions().set(&a, Location, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&a, Camera, Some(Setting::Allow)).unwrap();
    p.site_permissions().set(&a, Camera, None).unwrap();
    let row = |origin: &Origin, permission, setting| SiteSetting { origin: origin.clone(), permission, setting };
    assert_eq!(
        p.site_permissions().all(),
        [
            row(&a, Location, Setting::Allow),
            row(&a, Notifications, Setting::Block),
            row(&b, Camera, Setting::Block),
            row(&b, Midi, Setting::Allow),
        ]
    );
}

#[test]
fn screen_sharing_remembers_only_a_block() {
    let (mut p, _dir) = open(1);
    let site = origin("https://meet.example.com");
    let err = p.site_permissions().set(&site, ScreenShare, Some(Setting::Allow)).unwrap_err();
    assert!(matches!(err, Error::AlwaysAsks(ScreenShare)));
    assert_eq!(err.to_string(), "Screen sharing is asked for every time");
    assert!(exported(&mut p).is_empty());

    p.site_permissions().set(&site, ScreenShare, Some(Setting::Block)).unwrap();
    assert_eq!(p.site_permissions().get(&site, ScreenShare), Some(Setting::Block));

    // an Allow from another build reads as ask, and a reset still clears it
    let (mut q, _dq) = open(2);
    let rec = SitePermissionRecord {
        origin: site.clone(),
        permission: ScreenShare,
        setting: Lww::new(Some(Setting::Allow), Stamp { hlc: Hlc(5 << 16), device: DeviceId(9) }),
    };
    let wire = WireRecord { kind: Kind::SitePermissions, id: format!("screen_share|{}", site.as_str()), body: serde_json::to_vec(&rec).unwrap() };
    assert!(q.sync().apply(vec![wire]).unwrap().rejected.is_empty());
    assert_eq!(q.site_permissions().get(&site, ScreenShare), None);
    assert!(q.site_permissions().for_site(&site).is_empty());
    assert!(q.site_permissions().all().is_empty());
    assert_eq!(q.site_permissions().decide(Some(&site), &[ScreenShare], &TabGrants::default()), Decision::Ask(vec![ScreenShare]));
    q.site_permissions().reset_site(&site).unwrap();
    let stored: SitePermissionRecord = serde_json::from_slice(&exported(&mut q)[0].body).unwrap();
    assert_eq!(stored.setting.v, None);
}

#[test]
fn decide_matrix() {
    let (mut p, _dir) = open(1);
    let site = origin("https://meet.example.com");
    let none = TabGrants::default();
    let av = [Camera, Microphone];

    assert_eq!(p.site_permissions().decide(Some(&site), &av, &none), Decision::Ask(vec![Camera, Microphone]));

    p.site_permissions().set(&site, Camera, Some(Setting::Allow)).unwrap();
    assert_eq!(p.site_permissions().decide(Some(&site), &av, &none), Decision::Ask(vec![Microphone]), "asks only the rest");
    assert_eq!(p.site_permissions().decide(Some(&site), &[Camera], &none), Decision::Allow);

    p.site_permissions().set(&site, Microphone, Some(Setting::Block)).unwrap();
    assert_eq!(p.site_permissions().decide(Some(&site), &av, &none), Decision::Block, "a block wins over an allow");

    p.site_permissions().set(&site, Microphone, None).unwrap();
    let mut grants = TabGrants::default();
    assert!(p.site_permissions().answer(Some(&site), &[Microphone], Answer::AllowThisTime, &mut grants).unwrap());
    assert_eq!(p.site_permissions().decide(Some(&site), &av, &grants), Decision::Allow, "a stored allow plus a grant");

    let other = origin("https://other.example");
    assert_eq!(p.site_permissions().decide(Some(&other), &[Microphone], &grants), Decision::Ask(vec![Microphone]), "grants are per site");

    p.site_permissions().set(&site, Microphone, Some(Setting::Block)).unwrap();
    assert_eq!(p.site_permissions().decide(Some(&site), &[Microphone], &grants), Decision::Block, "a stored block beats a grant");

    // opaque origin: nothing is stored, only grants count
    let mut file_grants = TabGrants::default();
    assert_eq!(p.site_permissions().decide(None, &[Location], &file_grants), Decision::Ask(vec![Location]));
    p.site_permissions().answer(None, &[Location], Answer::AllowThisTime, &mut file_grants).unwrap();
    assert_eq!(p.site_permissions().decide(None, &[Location], &file_grants), Decision::Allow);
    assert_eq!(p.site_permissions().decide(Some(&site), &[Location], &file_grants), Decision::Ask(vec![Location]));
}

#[test]
fn answers_store_grant_or_do_nothing() {
    let (mut p, _dir) = open(1);
    let site = origin("https://meet.example.com");
    let av = [Camera, Microphone];
    let mut grants = TabGrants::default();

    assert!(p.site_permissions().answer(Some(&site), &av, Answer::AllowWhileVisiting, &mut grants).unwrap());
    assert_eq!(p.site_permissions().for_site(&site), [(Camera, Setting::Allow), (Microphone, Setting::Allow)]);
    assert_eq!(grants.granted().count(), 0);
    let records = exported(&mut p);
    assert_eq!(records.len(), 2);
    let ids: Vec<&str> = records.iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"camera|https://meet.example.com") && ids.contains(&"microphone|https://meet.example.com"), "{ids:?}");

    assert!(!p.site_permissions().answer(Some(&site), &[Location], Answer::NeverAllow, &mut grants).unwrap());
    assert_eq!(p.site_permissions().get(&site, Location), Some(Setting::Block));

    let before = upto(&mut p);
    assert!(!p.site_permissions().answer(Some(&site), &[Notifications], Answer::Dismiss, &mut grants).unwrap());
    assert!(p.site_permissions().answer(Some(&site), &[ScreenShare], Answer::AllowThisTime, &mut grants).unwrap());
    assert_eq!(upto(&mut p), before, "dismissing and one-time grants store nothing");
    assert_eq!(p.site_permissions().get(&site, Notifications), None);
    assert!(!grants.allows(Some(&site), ScreenShare), "each screen share is asked for");
    assert_eq!(grants.granted().count(), 0);
    assert!(p.site_permissions().answer(Some(&site), &[Camera, ScreenShare], Answer::AllowThisTime, &mut grants).unwrap());
    assert_eq!(grants.granted().collect::<Vec<_>>(), [Camera]);

    assert!(matches!(
        p.site_permissions().answer(Some(&site), &[ScreenShare], Answer::AllowWhileVisiting, &mut grants),
        Err(Error::AlwaysAsks(ScreenShare))
    ));

    // with no origin nothing can be remembered: an allow holds this time, a block denies this time
    let mut file = TabGrants::default();
    assert!(p.site_permissions().answer(None, &[Midi], Answer::AllowWhileVisiting, &mut file).unwrap());
    assert!(file.allows(None, Midi));
    assert!(!p.site_permissions().answer(None, &[ClipboardRead], Answer::NeverAllow, &mut file).unwrap());
    assert!(!file.allows(None, ClipboardRead));
    assert_eq!(upto(&mut p), before);
}

#[test]
fn tab_grants_hold_while_the_tab_stays_on_the_site() {
    let site = origin("https://meet.example.com");
    let (mut p, _dir) = open(1);
    let mut grants = TabGrants::default();
    p.site_permissions().answer(Some(&site), &[Camera, Microphone], Answer::AllowThisTime, &mut grants).unwrap();

    grants.committed(&url("https://meet.example.com/another-call"));
    assert!(grants.allows(Some(&site), Camera), "same origin keeps grants");
    grants.revoke(Camera);
    assert!(!grants.allows(Some(&site), Camera));
    assert_eq!(grants.granted().collect::<Vec<_>>(), [Microphone]);

    grants.committed(&url("http://meet.example.com/"));
    assert!(!grants.allows(Some(&site), Microphone), "another scheme is another site");
    assert_eq!(grants.granted().count(), 0);

    p.site_permissions().answer(Some(&site), &[Camera], Answer::AllowThisTime, &mut grants).unwrap();
    grants.committed(&url("https://other.example/"));
    assert_eq!(grants.granted().count(), 0, "cross-origin commit ends grants");

    p.site_permissions().answer(None, &[Location], Answer::AllowThisTime, &mut grants).unwrap();
    assert!(grants.allows(None, Location));
    grants.committed(&url("file:///C:/other.html"));
    assert!(!grants.allows(None, Location), "an opaque origin is never shared by two documents");
}

#[test]
fn prompt_words_and_buttons() {
    let site = origin("https://meet.example.com");

    let av = prompt(Some(&site), &[Microphone, Camera]);
    assert_eq!(av.heading, "Use your camera and microphone?");
    assert_eq!(av.body, "“meet.example.com” wants to use your camera and microphone.");
    assert_eq!(av.answers, [Answer::AllowWhileVisiting, Answer::AllowThisTime, Answer::NeverAllow, Answer::Dismiss]);

    let screen = prompt(Some(&site), &[ScreenShare]);
    assert_eq!(screen.heading, "Share your screen?");
    assert_eq!(screen.body, "“meet.example.com” wants to share your screen.");
    assert_eq!(screen.answers, [Answer::AllowThisTime, Answer::NeverAllow, Answer::Dismiss]);

    let file = prompt(None, &[Location]);
    assert_eq!(file.heading, "Know your location?");
    assert_eq!(file.body, "This page wants to know your location.");
    assert_eq!(file.answers, [Answer::AllowThisTime, Answer::Dismiss]);

    let headings: Vec<String> =
        [Camera, Microphone, Notifications, ClipboardRead, Midi].iter().map(|&p| prompt(Some(&site), &[p]).heading).collect();
    assert_eq!(
        headings,
        ["Use your camera?", "Use your microphone?", "Show notifications?", "See text and images copied to the clipboard?", "Use your MIDI devices?"]
    );
    assert_eq!(prompt(Some(&origin("http://localhost:8080")), &[Midi]).body, "“localhost:8080” wants to use your MIDI devices.");
    assert_eq!(
        prompt(Some(&site), &[Midi, ScreenShare, Camera]).body,
        "“meet.example.com” wants to use your camera and MIDI devices and share your screen."
    );

    let labels: Vec<&str> = [Answer::AllowWhileVisiting, Answer::AllowThisTime, Answer::NeverAllow, Answer::Dismiss].iter().map(|a| a.label()).collect();
    assert_eq!(labels, ["Allow while visiting the site", "Allow this time", "Never allow", "Not now"]);
}

#[test]
fn capturing_wording() {
    let c = |camera, microphone, screen| Capturing { camera, microphone, screen };
    let cases = [
        (c(false, false, false), None),
        (c(true, false, false), Some("Using your camera")),
        (c(false, true, false), Some("Using your microphone")),
        (c(true, true, false), Some("Using your camera and microphone")),
        (c(false, false, true), Some("Sharing your screen")),
        (c(true, false, true), Some("Sharing your screen and using your camera")),
        (c(false, true, true), Some("Sharing your screen and using your microphone")),
        (c(true, true, true), Some("Sharing your screen and using your camera and microphone")),
    ];
    for (capturing, text) in cases {
        assert_eq!(capturing.description().as_deref(), text, "{capturing:?}");
        assert_eq!(capturing.any(), text.is_some());
    }
    assert!(!Capturing::default().any());
}

#[test]
fn a_v5_profile_gains_the_table() {
    let dir = tmp();
    let site = origin("https://kept.example");
    drop(open_at(&dir, 1));
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    conn.execute_batch("DROP TABLE site_permissions; DROP TABLE site_zoom; DROP TABLE vault_key; DROP TABLE sync_secrets; PRAGMA user_version = 5;").unwrap();
    drop(conn);

    let mut p = open_at(&dir, 1);
    p.site_permissions().set(&site, Location, Some(Setting::Allow)).unwrap();
    drop(p);
    let conn = rusqlite::Connection::open(dir.0.join("vsesvit.db")).unwrap();
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 8);
    drop(conn);
    assert_eq!(open_at(&dir, 1).site_permissions().get(&site, Location), Some(Setting::Allow));
}

#[test]
fn settings_sync_between_profiles() {
    let (mut a, _da) = open(1);
    let (mut b, _db) = open(2);
    let site = origin("https://meet.example.com");

    a.site_permissions().set(&site, Camera, Some(Setting::Allow)).unwrap();
    a.site_permissions().set(&site, Notifications, Some(Setting::Block)).unwrap();
    let report = b.sync().apply(exported(&mut a)).unwrap();
    assert_eq!((report.merged, report.rejected.len()), (2, 0));
    assert!(report.changed.site_permissions);
    assert_eq!(b.site_permissions().for_site(&site), [(Camera, Setting::Allow), (Notifications, Setting::Block)]);

    // b's reset is newer than what a holds, so it wins on a
    b.site_permissions().reset_site(&site).unwrap();
    let report = a.sync().apply(exported(&mut b)).unwrap();
    assert_eq!(report.merged, 2);
    assert!(a.site_permissions().for_site(&site).is_empty());
    assert_eq!(exported(&mut a).iter().map(|w| &w.body).collect::<Vec<_>>(), exported(&mut b).iter().map(|w| &w.body).collect::<Vec<_>>());

    // re-applying changes nothing
    let report = a.sync().apply(exported(&mut b)).unwrap();
    assert_eq!((report.merged, report.unchanged), (0, 2));
    assert!(!report.changed.site_permissions);

    // a permission this build does not know (from a newer build) is rejected, not stored
    let mut newer = serde_json::to_value(serde_json::from_slice::<SitePermissionRecord>(&exported(&mut a)[0].body).unwrap()).unwrap();
    newer["permission"] = serde_json::json!("usb");
    let wire = WireRecord { kind: Kind::SitePermissions, id: format!("usb|{}", site.as_str()), body: serde_json::to_vec(&newer).unwrap() };
    let report = a.sync().apply(vec![wire]).unwrap();
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(exported(&mut a).len(), 2);
}
