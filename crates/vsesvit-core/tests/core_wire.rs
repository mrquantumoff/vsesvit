//! The wire rule, checked with serde: every mutable synced field is `{"v": .., "at": ".."}`,
//! unknown fields of that shape survive a round trip through an older build, and a field
//! of any other shape is rejected at the boundary instead of being silently dropped.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use vsesvit_core::bookmarks::{BookmarkId, BookmarkRecord, NodeKind, NodeState, Placement, Position};
use vsesvit_core::crdt::{DeviceId, Extra, Hlc, JsonText, Lww, Record, Stamp};
use vsesvit_core::ext_storage::SyncItemRecord;
use vsesvit_core::extensions::{ExtensionId, ExtensionRecord, StoreRef};
use vsesvit_core::history::{DeletionDirective, PageRecord, Transition, Visit};
use vsesvit_core::permissions::{Origin, Permission, Setting, SitePermissionRecord};
use vsesvit_core::prefs::PrefRecord;
use vsesvit_core::search::{EngineFields, EngineRecord, SearchEngineId, UrlTemplate};
use vsesvit_core::session::{DeviceSessionRecord, SessionSnapshot};
use vsesvit_core::Url;

fn stamp(h: u64) -> Stamp {
    Stamp { hlc: Hlc(h), device: DeviceId(0x1234) }
}

fn is_stamp(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Every top-level field except the named immutable ones is an `{"v","at"}` object.
fn assert_wire_rule(json: &Value, immutable: &[&str]) {
    let obj = json.as_object().expect("record is an object");
    for (k, v) in obj {
        if immutable.contains(&k.as_str()) {
            continue;
        }
        let field = v.as_object().unwrap_or_else(|| panic!("field {k} is not an object: {v}"));
        assert_eq!(field.len(), 2, "field {k} has keys other than v/at: {v}");
        assert!(field.contains_key("v"), "field {k} lacks v");
        assert!(is_stamp(&field["at"]), "field {k} lacks a stamp: {v}");
    }
}

fn bookmark() -> BookmarkRecord {
    BookmarkRecord {
        id: BookmarkId(uuid::Uuid::from_u128(0xabc)),
        placement: Lww::new(Placement { parent: BookmarkId::TOOLBAR, pos: Position::parse("V").unwrap() }, stamp(5)),
        added_ms: 42,
        state: NodeState::Url {
            title: Lww::new("t".into(), stamp(6)),
            url: Lww::new(Url::parse("https://a.example/").unwrap(), stamp(7)),
            extra: Extra::default(),
        },
    }
}

#[test]
fn stamps_are_32_hex_chars_and_round_trip() {
    let s = stamp(0x0102);
    let text = serde_json::to_string(&s).unwrap();
    assert_eq!(text, "\"00000000000001020000000000001234\"");
    assert_eq!(serde_json::from_str::<Stamp>(&text).unwrap(), s);
    assert!(serde_json::from_str::<Stamp>("\"0102\"").is_err());
}

#[test]
fn bookmark_wire_shape() {
    let v = serde_json::to_value(bookmark()).unwrap();
    assert_wire_rule(&v, &["id", "kind", "added_ms"]);
    assert_eq!(v["kind"], "url");
    assert_eq!(v["placement"]["v"]["parent"], BookmarkId::TOOLBAR.0.to_string());
    assert_eq!(v["placement"]["v"]["pos"], "V");
    assert!(v.get("deleted").is_none());
    let back: BookmarkRecord = serde_json::from_value(v).unwrap();
    assert_eq!(back, bookmark());
}

#[test]
fn bookmark_tombstone_wire_shape() {
    let mut r = bookmark();
    r.state = NodeState::Deleted { kind: NodeKind::Url, at: stamp(9) };
    let v = serde_json::to_value(r.clone()).unwrap();
    assert_wire_rule(&v, &["id", "kind", "added_ms", "deleted"]);
    assert!(is_stamp(&v["deleted"]));
    assert!(v.get("title").is_none() && v.get("url").is_none());
    assert_eq!(serde_json::from_value::<BookmarkRecord>(v).unwrap(), r);
}

#[test]
fn unknown_lww_fields_survive_a_round_trip_intact() {
    let mut v = serde_json::to_value(bookmark()).unwrap();
    let newer = json!({"v": {"color": "red", "n": [1, 2]}, "at": String::from(stamp(11))});
    v["label"] = newer.clone();
    let back: BookmarkRecord = serde_json::from_value(v.clone()).unwrap();
    let NodeState::Url { extra, .. } = &back.state else { panic!() };
    assert_eq!(extra.len(), 1);
    assert_eq!(extra["label"].at, stamp(11));
    assert_eq!(extra["label"].v, JsonText::from_value(&json!({"color": "red", "n": [1, 2]})));
    let again = serde_json::to_value(back).unwrap();
    assert_eq!(again["label"], newer);
    assert_eq!(again, v);
}

#[test]
fn unknown_fields_of_another_shape_are_rejected() {
    let mut v = serde_json::to_value(bookmark()).unwrap();
    v["counter"] = json!(3);
    assert!(serde_json::from_value::<BookmarkRecord>(v.clone()).is_err(), "a bare value is not an LWW field");
    v["counter"] = json!({"v": 3});
    assert!(serde_json::from_value::<BookmarkRecord>(v.clone()).is_err(), "missing stamp");
    v["counter"] = json!({"v": 3, "at": "zz"});
    assert!(serde_json::from_value::<BookmarkRecord>(v).is_err(), "malformed stamp");
}

#[test]
fn bookmark_boundary_validation() {
    let good = serde_json::to_value(bookmark()).unwrap();
    let mut root = good.clone();
    root["id"] = json!(BookmarkId::OTHER.0.to_string());
    assert!(serde_json::from_value::<BookmarkRecord>(root).is_err(), "roots are never records");
    let mut nil = good.clone();
    nil["id"] = json!(uuid::Uuid::nil().to_string());
    assert!(serde_json::from_value::<BookmarkRecord>(nil).is_ok(), "the nil uuid is an ordinary id, not a root");
    let mut mismatch = good.clone();
    mismatch["kind"] = json!("folder");
    assert!(serde_json::from_value::<BookmarkRecord>(mismatch).is_err(), "a folder has no url");
    let mut sep = good.clone();
    sep["kind"] = json!("separator");
    assert!(serde_json::from_value::<BookmarkRecord>(sep).is_err(), "a separator has no title");
    let mut pos = good.clone();
    pos["placement"]["v"]["pos"] = json!("a0");
    assert!(serde_json::from_value::<BookmarkRecord>(pos).is_err(), "trailing zero position");
    let mut url = good.clone();
    url["url"]["v"] = json!("not a url");
    assert!(serde_json::from_value::<BookmarkRecord>(url).is_err());
    let mut no_title = good;
    no_title.as_object_mut().unwrap().remove("title");
    assert!(serde_json::from_value::<BookmarkRecord>(no_title).is_err());
}

#[test]
fn page_record_wire_shape() {
    let mut visits = BTreeSet::new();
    visits.insert(Visit { at_ms: 5, device: DeviceId(1), transition: Transition::Typed });
    let mut extra = Extra::default();
    extra.insert("favicon".into(), Lww::new(JsonText::from_value(&json!("x")), stamp(2)));
    let rec = PageRecord { url: Url::parse("https://a.example/p").unwrap(), title: Lww::new("T".into(), stamp(1)), visits, extra };
    let v = serde_json::to_value(&rec).unwrap();
    assert_wire_rule(&v, &["url", "visits"]);
    assert_eq!(v["visits"][0]["transition"], "typed");
    assert_eq!(serde_json::from_value::<PageRecord>(v).unwrap(), rec);
    let d = DeletionDirective { id: uuid::Uuid::from_u128(3), url: None, from_ms: 1, to_ms: 2 };
    assert_eq!(serde_json::from_value::<DeletionDirective>(serde_json::to_value(&d).unwrap()).unwrap(), d);
}

#[test]
fn other_records_follow_the_rule() {
    let pref = PrefRecord { key: "theme".into(), value: Lww::new(Some(JsonText::from_value(&json!("dark"))), stamp(1)) };
    let v = serde_json::to_value(&pref).unwrap();
    assert_wire_rule(&v, &["key"]);
    assert_eq!(v["value"]["v"], "\"dark\"", "a JSON register carries its value as canonical text");
    assert_eq!(serde_json::from_value::<PrefRecord>(v).unwrap(), pref);

    let ext = ExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let item = SyncItemRecord { ext: ext.clone(), key: "k".into(), value: Lww::new(None, stamp(1)) };
    let v = serde_json::to_value(&item).unwrap();
    assert_wire_rule(&v, &["ext", "key"]);
    assert!(v["value"]["v"].is_null());
    assert_eq!(serde_json::from_value::<SyncItemRecord>(v).unwrap(), item);

    let mut extra = Extra::default();
    extra.insert("pinned".into(), Lww::new(JsonText::from_value(&json!(true)), stamp(3)));
    let x = ExtensionRecord { id: ext, store: Lww::new(StoreRef::Amo, stamp(1)), installed: Lww::new(true, stamp(2)), enabled: Lww::new(false, stamp(2)), extra };
    let v = serde_json::to_value(&x).unwrap();
    assert_wire_rule(&v, &["id"]);
    assert_eq!(v["store"]["v"], "amo");
    assert_eq!(v["pinned"]["v"], true);
    assert_eq!(serde_json::from_value::<ExtensionRecord>(v).unwrap(), x);

    let s = DeviceSessionRecord {
        device: DeviceId(9),
        session: Lww::new(Some(SessionSnapshot { device_name: "d".into(), windows: vec![], active_window: 0 }), stamp(4)),
    };
    let v = serde_json::to_value(&s).unwrap();
    assert_wire_rule(&v, &["device"]);
    assert_eq!(serde_json::from_value::<DeviceSessionRecord>(v).unwrap(), s);

    let origin = Origin::parse("https://meet.example.com").unwrap();
    let perm = SitePermissionRecord { origin, permission: Permission::ScreenShare, setting: Lww::new(Some(Setting::Block), stamp(5)) };
    let v = serde_json::to_value(&perm).unwrap();
    assert_wire_rule(&v, &["origin", "permission"]);
    assert_eq!(v["origin"], "https://meet.example.com");
    assert_eq!(v["permission"], "screen_share");
    assert_eq!(v["setting"]["v"], "block");
    assert_eq!(serde_json::from_value::<SitePermissionRecord>(v).unwrap(), perm);
}

/// `Lww<Option<JsonText>>` (prefs, `storage.sync` items) has two states that a bare JSON
/// value cannot tell apart: no value, and the value `null`. Both must round-trip.
#[test]
fn json_registers_carry_a_null_value_distinct_from_no_value() {
    let null = Some(JsonText::from_value(&Value::Null));
    let pref = PrefRecord { key: "x".into(), value: Lww::new(null.clone(), stamp(1)) };
    let with_null = serde_json::to_value(&pref).unwrap();
    assert_wire_rule(&with_null, &["key"]);
    assert_eq!(serde_json::from_value::<PrefRecord>(with_null.clone()).unwrap(), pref);

    let reset = PrefRecord { key: "x".into(), value: Lww::new(None, stamp(1)) };
    let without = serde_json::to_value(&reset).unwrap();
    assert_wire_rule(&without, &["key"]);
    assert!(without["value"]["v"].is_null(), "no value stays a JSON null, like every other Option register");
    assert_ne!(with_null, without);
    assert_eq!(serde_json::from_value::<PrefRecord>(without).unwrap(), reset);

    let ext = ExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let item = SyncItemRecord { ext, key: "k".into(), value: Lww::new(null, stamp(2)) };
    let v = serde_json::to_value(&item).unwrap();
    assert_wire_rule(&v, &["ext", "key"]);
    assert_eq!(serde_json::from_value::<SyncItemRecord>(v).unwrap(), item);

    // the value slot holds JSON text; anything else is rejected at the boundary
    let mut bad = serde_json::to_value(&pref).unwrap();
    bad["value"]["v"] = json!("{not json");
    assert!(serde_json::from_value::<PrefRecord>(bad).is_err());
}

#[test]
fn engine_record_is_live_or_tombstone() {
    let live = EngineRecord {
        id: SearchEngineId("builtin:ddg".into()),
        state: Record::Live(EngineFields {
            name: Lww::new("DDG".into(), stamp(1)),
            keyword: Lww::new(None, Stamp::ZERO),
            search_url: Lww::new(UrlTemplate("https://x/?q={searchTerms}".into()), Stamp::ZERO),
            suggest_url: Lww::new(None, Stamp::ZERO),
            extra: Extra::default(),
        }),
    };
    let v = serde_json::to_value(&live).unwrap();
    assert_wire_rule(&v, &["id"]);
    assert!(v.get("deleted").is_none());
    assert_eq!(serde_json::from_value::<EngineRecord>(v.clone()).unwrap(), live);

    let dead = EngineRecord { id: SearchEngineId("builtin:ddg".into()), state: Record::Tombstone(stamp(8)) };
    let t = serde_json::to_value(&dead).unwrap();
    assert_eq!(t.as_object().unwrap().len(), 2, "a tombstone carries only id and deleted: {t}");
    assert!(is_stamp(&t["deleted"]));
    assert_eq!(serde_json::from_value::<EngineRecord>(t).unwrap(), dead);

    let mut partial = v;
    partial.as_object_mut().unwrap().remove("search_url");
    assert!(serde_json::from_value::<EngineRecord>(partial).is_err(), "a live engine needs every field");
}
