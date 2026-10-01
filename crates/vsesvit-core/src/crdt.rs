//! Merge primitives. Every synced field of every kind is one of three things:
//!
//! - [`Lww<T>`]: last-writer-wins register. Join = max by `(stamp, value)`.
//! - a grow-only set (history visits, history deletion directives). Join = union.
//! - a terminal tombstone ([`Record<T>`]: bookmarks, search engines). Join = "deleted
//!   absorbs alive".
//!
//! All three are join-semilattices, so merging is commutative, associative and
//! idempotent. That is the whole convergence argument: a sync engine may apply remote
//! records in any order, in any batch split, any number of times, and every device that
//! has seen the same set of records holds the same state. Anything that must hold
//! *globally* (the bookmark tree has no cycles or orphans) is not enforced by merge. It
//! is computed from the merged state by a pure function (`bookmarks::materialize`), which
//! converges because its input does.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Random, nonzero, minted once per profile and stored in `meta`. 0 is reserved for
/// [`Stamp::ZERO`]: values that ship in code (built-in search engines, bookmark roots).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub u64);

impl DeviceId {
    pub fn random() -> Self {
        loop {
            let (hi, _) = uuid::Uuid::new_v4().as_u64_pair();
            if hi != 0 {
                return DeviceId(hi);
            }
        }
    }

    /// Wire form of a device id (the `Sessions` record key): 16 lowercase hex digits.
    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Hybrid logical clock value: `(unix_ms << 16) | logical`. It follows wall time while
/// clocks are sane and never goes backwards. After a device observes a remote stamp,
/// every later local stamp is greater, so "edit made after seeing X" always beats X,
/// even with skewed clocks.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc(pub u64);

impl Hlc {
    pub const ZERO: Hlc = Hlc(0);

    pub fn wall_ms(self) -> u64 {
        self.0 >> 16
    }
}

/// Where the clock reads wall time. `Manual` exists for the convergence property test,
/// which runs devices with independently skewed clocks.
#[derive(Clone, Debug, Default)]
pub enum TimeSource {
    #[default]
    System,
    Manual(Rc<Cell<u64>>),
}

pub(crate) struct Clock {
    source: TimeSource,
    last: Hlc,
}

impl Clock {
    pub(crate) fn new(source: TimeSource, persisted_last: Hlc) -> Self {
        Clock { source, last: persisted_last }
    }

    pub(crate) fn now_ms(&self) -> u64 {
        match &self.source {
            TimeSource::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            TimeSource::Manual(cell) => cell.get(),
        }
    }

    /// Next local stamp value: `max(last + 1, now_ms << 16)`. The caller (`db::Tx`)
    /// persists `last` in the same transaction that uses the value.
    pub(crate) fn tick(&mut self) -> Hlc {
        let wall = Hlc(self.now_ms() << 16);
        let next = Hlc(self.last.0.saturating_add(1));
        self.last = wall.max(next);
        self.last
    }

    /// Called with every stamp found in incoming records, so later local edits order after them.
    pub(crate) fn observe(&mut self, seen: Hlc) {
        if seen > self.last {
            self.last = seen;
        }
    }

    pub(crate) fn last(&self) -> Hlc {
        self.last
    }
}

/// Totally ordered edit stamp. `Ord` compares `hlc`, then `device`.
///
/// DB form: 16 bytes big-endian (`hlc ++ device`) in `*_at BLOB` columns.
/// Wire form: the same 16 bytes as 32 lowercase hex chars.
/// Both forms sort bytewise in `Ord` order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Stamp {
    pub hlc: Hlc,
    pub device: DeviceId,
}

impl Stamp {
    /// The stamp of values that ship in code. Any real edit beats it.
    pub const ZERO: Stamp = Stamp { hlc: Hlc::ZERO, device: DeviceId(0) };

    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.hlc.0.to_be_bytes());
        out[8..].copy_from_slice(&self.device.0.to_be_bytes());
        out
    }

    pub fn from_bytes(b: [u8; 16]) -> Self {
        let hlc = u64::from_be_bytes(b[..8].try_into().expect("8 bytes"));
        let device = u64::from_be_bytes(b[8..].try_into().expect("8 bytes"));
        Stamp { hlc: Hlc(hlc), device: DeviceId(device) }
    }

    /// DB column form. `None` stays `NULL`.
    pub(crate) fn to_vec(self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }

    pub(crate) fn from_slice(b: &[u8]) -> Result<Self, BadStamp> {
        let arr: [u8; 16] = b.try_into().map_err(|_| BadStamp)?;
        Ok(Stamp::from_bytes(arr))
    }
}

impl From<Stamp> for String {
    fn from(s: Stamp) -> String {
        format!("{:032x}", u128::from_be_bytes(s.to_bytes()))
    }
}

impl TryFrom<String> for Stamp {
    type Error = BadStamp;
    fn try_from(s: String) -> Result<Stamp, BadStamp> {
        // The guard also rejects the leading '+' that from_str_radix would accept.
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BadStamp);
        }
        u128::from_str_radix(&s, 16).map(|n| Stamp::from_bytes(n.to_be_bytes())).map_err(|_| BadStamp)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("malformed stamp")]
pub struct BadStamp;

/// Last-writer-wins register.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lww<T> {
    pub v: T,
    pub at: Stamp,
}

impl<T: Ord> Lww<T> {
    pub fn new(v: T, at: Stamp) -> Self {
        Lww { v, at }
    }

    /// Join = max by `(at, v)`. The value tiebreak makes this a total order even if two
    /// devices mint the same stamp (a copied profile directory), so convergence never
    /// depends on stamps being unique.
    pub fn join(&mut self, other: Self) {
        if (other.at, &other.v) > (self.at, &self.v) {
            *self = other;
        }
    }

    /// Local write. Writing the current value is a no-op and does not mint a new
    /// stamp, so idempotent UI actions never create sync traffic. Returns whether the
    /// value changed.
    pub fn set(&mut self, v: T, at: Stamp) -> bool {
        if self.v == v {
            return false;
        }
        debug_assert!(at > self.at, "local stamps are always newer than anything observed");
        *self = Lww { v, at };
        true
    }
}

impl<T: Ord + Clone> Lattice for Lww<T> {
    fn join(&mut self, other: Self) {
        Lww::join(self, other);
    }
}

/// A record type that merges as a join-semilattice. Every `SyncTable::Record` implements
/// it, and `tests/convergence.rs` checks the three laws for each one.
pub trait Lattice: Clone + Eq {
    fn join(&mut self, other: Self);
}

/// A record with a terminal tombstone: once deleted, always deleted. Two tombstones keep
/// the later stamp; two live values join field-wise. Used where re-adding is a new thing
/// (search engines; bookmarks use the same rule inside `NodeState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record<T> {
    Live(T),
    Tombstone(Stamp),
}

impl<T> Record<T> {
    pub fn is_deleted(&self) -> bool {
        matches!(self, Record::Tombstone(_))
    }

    pub fn live(&self) -> Option<&T> {
        match self {
            Record::Live(t) => Some(t),
            Record::Tombstone(_) => None,
        }
    }
}

impl<T: Lattice> Lattice for Record<T> {
    fn join(&mut self, other: Self) {
        match (&mut *self, other) {
            (Record::Tombstone(mine), Record::Tombstone(theirs)) => {
                if theirs > *mine {
                    *mine = theirs;
                }
            }
            (Record::Tombstone(_), Record::Live(_)) => {}
            (Record::Live(_), Record::Tombstone(at)) => *self = Record::Tombstone(at),
            (Record::Live(mine), Record::Live(theirs)) => mine.join(theirs),
        }
    }
}

/// Canonical JSON (the `serde_json::to_string` form of a `Value`, object keys sorted).
/// `Ord` by bytes so it can sit inside an `Lww`. Prefs, `storage.sync` values and
/// unknown fields use it. On the wire it is the JSON value itself, which is what
/// [`Extra`] needs; an `Lww<Option<JsonText>>` register uses [`json_register`] instead.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "serde_json::Value", from = "serde_json::Value")]
pub struct JsonText(String);

impl JsonText {
    /// `serde_json::Value` objects are `BTreeMap`s, so the text has sorted keys.
    pub fn from_value(v: &serde_json::Value) -> Self {
        JsonText(serde_json::to_string(v).expect("a Value always serializes"))
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::from_str(&self.0).expect("JsonText holds valid JSON")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Boundary constructor for DB text. Re-canonicalizes, so a hand-edited row cannot
    /// break the `Ord`-by-bytes contract.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        serde_json::from_str::<serde_json::Value>(text).ok().map(|v| JsonText::from_value(&v))
    }
}

impl From<JsonText> for serde_json::Value {
    fn from(j: JsonText) -> Self {
        j.to_value()
    }
}

impl From<serde_json::Value> for JsonText {
    fn from(v: serde_json::Value) -> Self {
        JsonText::from_value(&v)
    }
}

/// Serde form of an `Lww<Option<JsonText>>` register (prefs, `storage.sync` items):
/// `{"v": "<canonical json>" | null, "at": "<stamp>"}`, the two states of the DB column.
///
/// The value travels as text rather than as the JSON value itself because the value may
/// be JSON `null`, which `Option` would read back as "no value" (a reset or removal). A
/// peer would then drop the key, and a device re-downloading its own upload would merge it
/// as different and re-upload it every round. `Extra` keeps the value form: a newer
/// build's typed field must come back exactly as it was sent.
pub mod json_register {
    use std::borrow::Cow;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{JsonText, Lww, Stamp};

    #[derive(Serialize, Deserialize)]
    struct Wire<'a> {
        v: Option<Cow<'a, str>>,
        at: Stamp,
    }

    pub fn serialize<S: Serializer>(reg: &Lww<Option<JsonText>>, s: S) -> Result<S::Ok, S::Error> {
        Wire { v: reg.v.as_ref().map(|j| Cow::Borrowed(j.as_str())), at: reg.at }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Lww<Option<JsonText>>, D::Error> {
        let Wire { v, at } = Wire::deserialize(d)?;
        let v = match v {
            None => None,
            Some(text) => Some(JsonText::parse(&text).ok_or_else(|| serde::de::Error::custom("value is not JSON"))?),
        };
        Ok(Lww { v, at })
    }
}

/// Fields this build does not know, received from a newer build.
///
/// Wire-format rule: every mutable synced field is shaped `{"v": .., "at": "<stamp>"}`.
/// Because of that rule, an older device can merge a newer device's unknown fields as
/// LWW JSON and upload them again intact. Without it, the first old device to re-upload
/// a merged record would erase the new field everywhere. A newer build that needs a
/// field with any other merge rule must add a new `sync::Kind`.
pub type Extra = BTreeMap<String, Lww<JsonText>>;

pub fn join_extra(a: &mut Extra, b: Extra) {
    for (k, v) in b {
        match a.get_mut(&k) {
            Some(mine) => mine.join(v),
            None => {
                a.insert(k, v);
            }
        }
    }
}

/// The greatest stamp inside an `Extra`, for clock observation.
pub(crate) fn extra_max_stamp(extra: &Extra) -> Option<Stamp> {
    extra.values().map(|l| l.at).max()
}

/// Local change sequence, the cursor a sync engine keeps. Every local edit and every
/// merge whose result is newer than the incoming record (see `sync::apply_one`) sets the
/// row's `seq` to a fresh value. `Seq::ZERO` marks a row that holds nothing the server
/// lacks (received from sync and never edited since). Unlike `Stamp`, it is local, dense
/// and never sent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Seq(pub u64);

impl Seq {
    pub const ZERO: Seq = Seq(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_bytes_and_hex_round_trip_and_sort_like_ord() {
        let a = Stamp { hlc: Hlc(0x0102_0304_0506_0708), device: DeviceId(0x0a0b_0c0d_0e0f_1011) };
        let b = Stamp { hlc: Hlc(0x0102_0304_0506_0709), device: DeviceId(1) };
        assert_eq!(Stamp::from_bytes(a.to_bytes()), a);
        let hex: String = a.into();
        assert_eq!(hex, "01020304050607080a0b0c0d0e0f1011");
        assert_eq!(Stamp::try_from(hex).unwrap(), a);
        assert!(a < b);
        assert!(a.to_bytes() < b.to_bytes());
        assert!(String::from(a) < String::from(b));
        assert!(Stamp::try_from("zz".to_owned()).is_err());
    }

    #[test]
    fn stamp_hex_is_32_padded_digits_of_either_case() {
        assert_eq!(String::from(Stamp::ZERO), "0".repeat(32));
        let a = Stamp { hlc: Hlc(0x0102_0304_0506_0708), device: DeviceId(0x0a0b_0c0d_0e0f_1011) };
        assert_eq!(Stamp::try_from("01020304050607080A0B0C0D0E0F1011".to_owned()).unwrap(), a);
        assert!(Stamp::try_from(format!("+{}", "0".repeat(31))).is_err());
        assert!(Stamp::try_from("0".repeat(31)).is_err());
        assert!(Stamp::try_from("0".repeat(33)).is_err());
    }

    #[test]
    fn clock_never_goes_backwards_and_orders_after_observed() {
        let t = Rc::new(Cell::new(1_000u64));
        let mut c = Clock::new(TimeSource::Manual(t.clone()), Hlc::ZERO);
        let first = c.tick();
        assert_eq!(first.wall_ms(), 1_000);
        let second = c.tick();
        assert!(second > first);
        assert_eq!(second.wall_ms(), 1_000);
        c.observe(Hlc(5_000 << 16));
        let third = c.tick();
        assert!(third > Hlc(5_000 << 16));
        t.set(9_000);
        assert_eq!(c.tick().wall_ms(), 9_000);
    }

    /// A clock already at the top of the range (persisted before sync bounded remote stamps)
    /// stays there instead of overflowing back to wall time.
    #[test]
    fn a_clock_at_the_top_of_its_range_saturates() {
        let mut c = Clock::new(TimeSource::Manual(Rc::new(Cell::new(1_000))), Hlc(u64::MAX));
        assert_eq!(c.tick(), Hlc(u64::MAX));
    }

    #[test]
    fn lww_join_is_max_by_stamp_then_value() {
        let s1 = Stamp { hlc: Hlc(1), device: DeviceId(1) };
        let mut a = Lww::new("a".to_owned(), s1);
        a.join(Lww::new("b".to_owned(), s1));
        assert_eq!(a.v, "b");
        a.join(Lww::new("a".to_owned(), s1));
        assert_eq!(a.v, "b");
        assert!(!a.set("b".to_owned(), Stamp { hlc: Hlc(2), device: DeviceId(1) }));
    }

    #[test]
    fn record_tombstone_is_terminal() {
        let s1 = Stamp { hlc: Hlc(1), device: DeviceId(1) };
        let s2 = Stamp { hlc: Hlc(2), device: DeviceId(1) };
        let mut r = Record::Live(Lww::new(1u8, s2));
        r.join(Record::Tombstone(s1));
        assert_eq!(r, Record::Tombstone(s1));
        r.join(Record::Live(Lww::new(2u8, s2)));
        assert_eq!(r, Record::Tombstone(s1));
        r.join(Record::Tombstone(s2));
        assert_eq!(r, Record::Tombstone(s2));
    }

    #[test]
    fn json_text_is_canonical() {
        let v: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":[1, 2]}"#).unwrap();
        assert_eq!(JsonText::from_value(&v).as_str(), r#"{"a":[1,2],"b":1}"#);
        assert_eq!(JsonText::parse(" {\"z\" : null } ").unwrap().as_str(), r#"{"z":null}"#);
    }
}
