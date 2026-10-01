//! Convergence property tests. They drive the real public API: real `Profile`s on real
//! SQLite files, the real `SyncStore`, and the dumbest possible sync server. No model
//! of core is involved.
//!
//! `devices_converge` is the one that matters. Three devices with independently skewed
//! clocks run random local edits across every synced kind, with bookmark moves, reorders,
//! and deletes of folders while children are added elsewhere. They upload and download
//! through a server that keeps only the last upload per record and never merges. Downloads
//! are truncated, shuffled, duplicated and split into batches. Then the devices settle.
//! The test asserts:
//!
//! 1. **Quiescence**: settling ends within `MAX_SETTLE_ROUNDS`. No record echoes forever.
//! 2. **Convergence**: every device exports byte-identical records for every kind.
//! 3. **Same tree**: every device materializes the same bookmark tree.
//! 4. **Valid tree**: no cycles, every live record reachable exactly once from the roots,
//!    every parent a folder, indexes dense.
//! 5. **Deletion spec, exactly**: live bookmarks = created - deleted, where "deleted"
//!    is every node some device removed, directly or inside a subtree it could see. So
//!    deletions propagate, and nothing added concurrently is lost.
//! 6. **Idempotence and order independence**: re-applying a device's full export, shuffled
//!    and duplicated, to another device changes nothing and marks nothing dirty.
//!
//! `lattice_laws` and `materialize_always_valid` pin the two layers underneath:
//! per-record joins are semilattices, and `materialize` returns a valid tree for *any*
//! record set, including ones no API sequence can produce (arbitrary cycles, dangling
//! parents).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::PathBuf;
use std::rc::Rc;

use proptest::prelude::*;
use proptest::sample::Index;

use vsesvit_core::bookmarks::{
    materialize, BookmarkId, BookmarkNode, BookmarkRecord, InsertAt, NodeKind, NodeState, Placement, Position,
};
use vsesvit_core::crdt::{DeviceId, Extra, Hlc, JsonText, Lattice, Lww, Record, Seq, Stamp, TimeSource};
use vsesvit_core::ext_storage::{Area, SyncItemRecord};
use vsesvit_core::extensions::{ExtensionId, ExtensionRecord, StoreRef, DEFAULT_CHROME_VERSION};
use vsesvit_core::history::{DeletionDirective, PageRecord, Transition, Visit};
use vsesvit_core::permissions::{Origin, Permission, Setting, SitePermissionRecord};
use vsesvit_core::prefs::{keys, PrefRecord, Theme};
use vsesvit_core::search::{EngineFields, EngineRecord, SearchEngineId, UrlTemplate};
use vsesvit_core::session::{DeviceSessionRecord, SessionSnapshot};
use vsesvit_core::sync::{Kind, WireRecord};
use vsesvit_core::{OpenOptions, Profile, Url};

const DEVICES: usize = 3;
const MAX_SETTLE_ROUNDS: usize = 4;
const VISIBLE_ROOTS: [BookmarkId; 3] = [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE];

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Device {
    profile: Profile, // dropped before `_dir`
    time: Rc<Cell<u64>>,
    upload_cursor: BTreeMap<Kind, Seq>,
    download_cursor: usize,
    _dir: TempDir,
}

impl Device {
    fn new(n: usize, start_ms: u64) -> Device {
        let dir = TempDir(std::env::temp_dir().join(format!("vsesvit-conv-{}", uuid::Uuid::new_v4())));
        let time = Rc::new(Cell::new(start_ms));
        let profile = Profile::open(
            &dir.0,
            OpenOptions {
                time: TimeSource::Manual(time.clone()),
                new_device_id: Some(DeviceId(n as u64 + 1)),
                chrome_version: DEFAULT_CHROME_VERSION.to_owned(),
                ..OpenOptions::default()
            },
        )
        .expect("open profile");
        Device { profile, time, upload_cursor: BTreeMap::new(), download_cursor: 0, _dir: dir }
    }

    /// Upload local changes since the cursor, at most `limit` per kind.
    fn upload(&mut self, server: &mut Server, limit: usize) -> usize {
        let mut sent = 0;
        for &kind in Kind::ALL {
            let since = self.upload_cursor.get(&kind).copied().unwrap_or(Seq::ZERO);
            let batch = self.profile.sync().changes_since(kind, since, limit).unwrap();
            sent += batch.records.len();
            server.upload(batch.records);
            self.upload_cursor.insert(kind, batch.upto);
        }
        sent
    }

    fn pending_upload(&mut self) -> bool {
        Kind::ALL.iter().any(|&kind| {
            let since = self.upload_cursor.get(&kind).copied().unwrap_or(Seq::ZERO);
            !self.profile.sync().changes_since(kind, since, 1).unwrap().records.is_empty()
        })
    }

    /// Download from the server, shuffled, optionally duplicated, applied in `split` batches.
    fn download(&mut self, server: &Server, limit: usize, split: usize, seed: u64, duplicate: bool) {
        let (mut records, cursor) = server.since(self.download_cursor, limit);
        self.download_cursor = cursor;
        if duplicate {
            let dup = records[..records.len() / 2].to_vec();
            records.extend(dup);
        }
        shuffle(&mut records, seed);
        let chunk = records.len().div_ceil(split.max(1)).max(1);
        for batch in records.chunks(chunk) {
            let report = self.profile.sync().apply(batch.to_vec()).unwrap();
            assert!(report.rejected.is_empty(), "valid records rejected: {:?}", report.rejected);
        }
    }

    /// Full state as sync sees it: every record of every kind, as bytes.
    fn export(&mut self) -> BTreeMap<(Kind, String), Vec<u8>> {
        let mut out = BTreeMap::new();
        for &kind in Kind::ALL {
            let batch = self.profile.sync().changes_since(kind, Seq::ZERO, usize::MAX).unwrap();
            assert!(!batch.more);
            for r in batch.records {
                out.insert((kind, r.id), r.body);
            }
        }
        out
    }
}

/// Keeps the last upload per (kind, id). Never merges. A download returns each record's
/// latest upload whose log position is past the cursor.
#[derive(Default)]
struct Server {
    log: Vec<WireRecord>,
    latest: BTreeMap<(Kind, String), usize>,
}

impl Server {
    fn upload(&mut self, records: Vec<WireRecord>) {
        for r in records {
            self.latest.insert((r.kind, r.id.clone()), self.log.len());
            self.log.push(r);
        }
    }

    /// At most `limit` records, oldest upload first, plus the next cursor.
    fn since(&self, cursor: usize, limit: usize) -> (Vec<WireRecord>, usize) {
        let mut idx: Vec<usize> = self.latest.values().copied().filter(|&i| i >= cursor).collect();
        idx.sort_unstable();
        let next = if idx.len() > limit {
            idx.truncate(limit);
            idx[limit - 1] + 1
        } else {
            self.log.len()
        };
        (idx.into_iter().map(|i| self.log[i].clone()).collect(), next)
    }
}

fn shuffle<T>(v: &mut [T], mut seed: u64) {
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        v.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    // bookmarks: picks are resolved against the device's *current* tree when the op runs
    AddUrl { parent: Index, at: Index, title: u8, url: u8 },
    AddFolder { parent: Index, at: Index, title: u8 },
    Rename { node: Index, title: u8 },
    Move { node: Index, parent: Index, at: Index },
    Remove { node: Index },
    // every other kind goes through the same machinery
    Visit { url: u8 },
    ForgetUrl { url: u8 },
    SetTheme { theme: u8 },
    StorageSet { key: u8, value: u8 },
    StorageRemove { key: u8 },
    SetPermission { site: u8, permission: u8, setting: u8 },
    ResetSite { site: u8 },
}

#[derive(Clone, Debug)]
enum Step {
    Local { dev: usize, op: Op },
    /// Clocks advance independently (or not at all): skew is part of the test.
    Tick { dev: usize, ms: u32 },
    Upload { dev: usize, limit: usize },
    Download { dev: usize, limit: usize, split: usize, seed: u64, duplicate: bool },
}

/// Test-side bookkeeping that defines the expected outcome (assertion 5).
#[derive(Default)]
struct Expect {
    created: BTreeSet<BookmarkId>,
    deleted: BTreeSet<BookmarkId>,
}

fn url(n: u8) -> Url {
    Url::parse(&format!("https://site{}.example/", n % 6)).unwrap()
}

fn origin(n: u8) -> Origin {
    Origin::of(&url(n)).unwrap()
}

fn ext() -> ExtensionId {
    ExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
}

/// Live nodes in DFS order from the visible roots (roots excluded).
fn live_nodes(p: &mut Profile) -> Vec<BookmarkNode> {
    let mut out = Vec::new();
    let mut stack: Vec<BookmarkId> = VISIBLE_ROOTS.to_vec();
    while let Some(folder) = stack.pop() {
        for child in p.bookmarks().children(folder) {
            if child.kind == NodeKind::Folder {
                stack.push(child.id);
            }
            out.push(child);
        }
    }
    out
}

fn subtree(p: &mut Profile, id: BookmarkId) -> Vec<BookmarkId> {
    let mut out = vec![id];
    let mut i = 0;
    while i < out.len() {
        let kids = p.bookmarks().children(out[i]);
        out.extend(kids.into_iter().map(|n| n.id));
        i += 1;
    }
    out
}

fn run_op(p: &mut Profile, op: &Op, expect: &mut Expect) {
    let nodes = live_nodes(p);
    let folders: Vec<BookmarkId> = VISIBLE_ROOTS
        .iter()
        .copied()
        .chain(nodes.iter().filter(|n| n.kind == NodeKind::Folder).map(|n| n.id))
        .collect();
    let slot = |p: &mut Profile, folder: BookmarkId, at: &Index| {
        InsertAt::Index(at.index(p.bookmarks().children(folder).len() + 1))
    };
    match op {
        Op::AddUrl { parent, at, title, url: u } => {
            let parent = *parent.get(&folders);
            let at = slot(p, parent, at);
            let id = p.bookmarks().add_url(parent, at, &format!("t{title}"), &url(*u)).unwrap();
            expect.created.insert(id);
        }
        Op::AddFolder { parent, at, title } => {
            let parent = *parent.get(&folders);
            let at = slot(p, parent, at);
            let id = p.bookmarks().add_folder(parent, at, &format!("f{title}")).unwrap();
            expect.created.insert(id);
        }
        Op::Rename { node, title } if !nodes.is_empty() => {
            let id = node.get(&nodes).id;
            p.bookmarks().rename(id, &format!("r{title}")).unwrap();
        }
        Op::Move { node, parent, at } if !nodes.is_empty() => {
            let id = node.get(&nodes).id;
            let parent = *parent.get(&folders);
            let at = slot(p, parent, at);
            // WouldCycle is an expected refusal. Anything else is a bug.
            match p.bookmarks().move_to(id, parent, at) {
                Ok(()) | Err(vsesvit_core::Error::Bookmark(vsesvit_core::bookmarks::BookmarkError::WouldCycle)) => {}
                Err(e) => panic!("move failed: {e}"),
            }
        }
        Op::Remove { node } if !nodes.is_empty() => {
            let id = node.get(&nodes).id;
            expect.deleted.extend(subtree(p, id)); // exactly what this device can see
            p.bookmarks().remove(id).unwrap();
        }
        Op::Rename { .. } | Op::Move { .. } | Op::Remove { .. } => {} // empty tree
        Op::Visit { url: u } => p.history().record_visit(&url(*u), Transition::Link).unwrap(),
        Op::ForgetUrl { url: u } => p.history().delete_url(&url(*u)).unwrap(),
        Op::SetTheme { theme } => {
            let t = [Theme::System, Theme::Light, Theme::Dark][*theme as usize % 3];
            p.prefs().set(&keys::THEME, &t).unwrap();
        }
        Op::StorageSet { key, value } => {
            // JSON null is a stored value (Chrome semantics), distinct from a removed key.
            let value = if value % 4 == 0 { serde_json::Value::Null } else { serde_json::json!(value) };
            let items = BTreeMap::from([(format!("k{}", key % 4), value)]);
            p.ext_storage().set(&ext(), Area::Sync, items).unwrap();
        }
        Op::StorageRemove { key } => {
            p.ext_storage().remove(&ext(), Area::Sync, &[format!("k{}", key % 4)]).unwrap();
        }
        Op::SetPermission { site, permission, setting } => {
            let permission = Permission::ALL[*permission as usize % Permission::ALL.len()];
            let setting = [None, Some(Setting::Allow), Some(Setting::Block)][*setting as usize % 3];
            // AlwaysAsks (an Allow of screen sharing) is an expected refusal. Anything else is a bug.
            match p.site_permissions().set(&origin(site % 3), permission, setting) {
                Ok(()) | Err(vsesvit_core::Error::AlwaysAsks(_)) => {}
                Err(e) => panic!("set failed: {e}"),
            }
        }
        Op::ResetSite { site } => p.site_permissions().reset_site(&origin(site % 3)).unwrap(),
    }
}

/// Everyone uploads everything, then everyone downloads everything, until nobody has
/// anything left to send. Returns the number of rounds used.
fn settle(devices: &mut [Device], server: &mut Server) -> usize {
    for round in 1..=MAX_SETTLE_ROUNDS {
        for d in devices.iter_mut() {
            d.upload(server, usize::MAX);
        }
        for d in devices.iter_mut() {
            d.download(server, usize::MAX, 1, round as u64, false);
        }
        if !devices.iter_mut().any(|d| d.pending_upload()) {
            return round;
        }
    }
    panic!("no quiescence after {MAX_SETTLE_ROUNDS} rounds: records are echoing");
}

/// Flattened tree: (id, parent, index, title, url) in DFS order.
fn tree_shape(p: &mut Profile) -> Vec<(BookmarkId, BookmarkId, usize, String, Option<Url>)> {
    live_nodes(p).into_iter().map(|n| (n.id, n.parent, n.index, n.title, n.url)).collect()
}

fn assert_valid_tree(p: &mut Profile, live_records: &BTreeSet<BookmarkId>) {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<BookmarkId> = VISIBLE_ROOTS.to_vec();
    while let Some(folder) = stack.pop() {
        for (i, child) in p.bookmarks().children(folder).into_iter().enumerate() {
            assert!(seen.insert(child.id), "{:?} reachable twice: cycle or duplicate", child.id);
            assert_eq!(child.parent, folder);
            assert_eq!(child.index, i);
            if child.kind == NodeKind::Folder {
                stack.push(child.id);
            }
        }
    }
    assert_eq!(&seen, live_records, "live records not reachable from the roots (orphans)");
}

fn live_bookmark_ids(export: &BTreeMap<(Kind, String), Vec<u8>>) -> BTreeSet<BookmarkId> {
    export
        .iter()
        .filter(|((k, _), _)| *k == Kind::Bookmarks)
        .map(|(_, body)| serde_json::from_slice::<BookmarkRecord>(body).unwrap())
        .filter(|r| !r.state.is_deleted())
        .map(|r| r.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<Index>(), any::<Index>(), any::<u8>(), any::<u8>()).prop_map(|(parent, at, title, url)| Op::AddUrl { parent, at, title, url }),
        3 => (any::<Index>(), any::<Index>(), any::<u8>()).prop_map(|(parent, at, title)| Op::AddFolder { parent, at, title }),
        2 => (any::<Index>(), any::<u8>()).prop_map(|(node, title)| Op::Rename { node, title }),
        4 => (any::<Index>(), any::<Index>(), any::<Index>()).prop_map(|(node, parent, at)| Op::Move { node, parent, at }),
        2 => any::<Index>().prop_map(|node| Op::Remove { node }),
        1 => any::<u8>().prop_map(|url| Op::Visit { url }),
        1 => any::<u8>().prop_map(|url| Op::ForgetUrl { url }),
        1 => any::<u8>().prop_map(|theme| Op::SetTheme { theme }),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(key, value)| Op::StorageSet { key, value }),
        1 => any::<u8>().prop_map(|key| Op::StorageRemove { key }),
        2 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(site, permission, setting)| Op::SetPermission { site, permission, setting }),
        1 => any::<u8>().prop_map(|site| Op::ResetSite { site }),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => (0..DEVICES, op()).prop_map(|(dev, op)| Step::Local { dev, op }),
        2 => (0..DEVICES, 0u32..120_000).prop_map(|(dev, ms)| Step::Tick { dev, ms }),
        2 => (0..DEVICES, 1usize..16).prop_map(|(dev, limit)| Step::Upload { dev, limit }),
        2 => (0..DEVICES, 1usize..16, 1usize..=3, any::<u64>(), any::<bool>())
            .prop_map(|(dev, limit, split, seed, duplicate)| Step::Download { dev, limit, split, seed, duplicate }),
    ]
}

// ---------------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn devices_converge(
        // Wall clocks up to +-1 h apart at the start
        skew in prop::array::uniform3(0u64..7_200_000),
        steps in prop::collection::vec(step(), 1..120),
        reapply_seed in any::<u64>(),
    ) {
        let base = 1_780_000_000_000u64;
        let mut devices: Vec<Device> = (0..DEVICES).map(|n| Device::new(n, base + skew[n])).collect();
        let mut server = Server::default();
        let mut expect = Expect::default();

        for s in &steps {
            match s {
                Step::Local { dev, op } => run_op(&mut devices[*dev].profile, op, &mut expect),
                Step::Tick { dev, ms } => { let t = &devices[*dev].time; t.set(t.get() + *ms as u64) }
                Step::Upload { dev, limit } => { devices[*dev].upload(&mut server, *limit); }
                Step::Download { dev, limit, split, seed, duplicate } =>
                    devices[*dev].download(&server, *limit, *split, *seed, *duplicate),
            }
        }

        // 1. quiescence
        settle(&mut devices, &mut server);

        // 2. convergence: identical records for every kind
        let exports: Vec<_> = devices.iter_mut().map(Device::export).collect();
        for e in &exports[1..] {
            prop_assert_eq!(e, &exports[0]);
        }

        // 3. identical materialized tree
        let shape0 = tree_shape(&mut devices[0].profile);
        for d in &mut devices[1..] {
            prop_assert_eq!(tree_shape(&mut d.profile), shape0.clone());
        }

        // 4. valid tree, and every live record is in it
        let live = live_bookmark_ids(&exports[0]);
        for d in &mut devices {
            assert_valid_tree(&mut d.profile, &live);
        }

        // 5. deletion spec, exactly
        let expected_live: BTreeSet<_> = expect.created.difference(&expect.deleted).copied().collect();
        prop_assert_eq!(&live, &expected_live);

        // 6. re-applying a full export (shuffled, duplicated) is a no-op everywhere
        let mut replay: Vec<WireRecord> = exports[0]
            .iter()
            .map(|((kind, id), body)| WireRecord { kind: *kind, id: id.clone(), body: body.clone() })
            .collect();
        let dup = replay.clone();
        replay.extend(dup);
        shuffle(&mut replay, reapply_seed);
        for d in &mut devices {
            let report = d.profile.sync().apply(replay.clone()).unwrap();
            prop_assert_eq!(report.merged, 0);
            prop_assert!(!d.pending_upload(), "re-apply marked records dirty");
        }
    }
}

// ---------------------------------------------------------------------------
// Layer 1: per-record joins are semilattices (one instantiation per record type)
// ---------------------------------------------------------------------------

fn check_laws<R: Lattice + Debug>(a: R, b: R, c: R) -> Result<(), TestCaseError> {
    let join = |mut x: R, y: R| {
        x.join(y);
        x
    };
    prop_assert_eq!(join(a.clone(), a.clone()), a.clone(), "idempotent");
    prop_assert_eq!(join(a.clone(), b.clone()), join(b.clone(), a.clone()), "commutative");
    prop_assert_eq!(
        join(join(a.clone(), b.clone()), c.clone()),
        join(a, join(b, c)),
        "associative"
    );
    Ok(())
}

fn stamp() -> impl Strategy<Value = Stamp> {
    // Tiny ranges on purpose: equal stamps and equal hlc across devices must be common.
    (0u64..4, 1u64..3).prop_map(|(h, d)| Stamp { hlc: Hlc(h), device: DeviceId(d) })
}

fn small_id(n: u8) -> BookmarkId {
    BookmarkId(uuid::Uuid::from_u128(1000 + n as u128))
}

fn placement() -> impl Strategy<Value = Lww<Placement>> {
    let pos = prop::sample::select(vec!["V", "k", "kV", "z"]).prop_map(|s| Position::parse(s).unwrap());
    let parent = prop_oneof![Just(BookmarkId::TOOLBAR), Just(BookmarkId::OTHER), (0u8..8).prop_map(small_id)];
    (parent, pos, stamp()).prop_map(|(parent, pos, at)| Lww::new(Placement { parent, pos }, at))
}

/// Unknown fields from a newer build: tiny key and value spaces so equal stamps collide.
fn extra() -> impl Strategy<Value = Extra> {
    prop::collection::btree_map(
        prop::sample::select(vec!["x", "y"]).prop_map(str::to_owned),
        (0u8..3, stamp()).prop_map(|(v, at)| Lww::new(JsonText::from_value(&serde_json::json!(v)), at)),
        0..3,
    )
}

/// Records of one fixed id and one fixed kind (joins across kinds are rejected at the boundary).
fn folder_record(id: BookmarkId) -> impl Strategy<Value = BookmarkRecord> {
    (placement(), 0i64..3, prop::option::weighted(0.3, stamp()), stamp(), 0u8..3, extra()).prop_map(
        move |(placement, added_ms, deleted, title_at, t, extra)| {
            let state = match deleted {
                Some(at) => NodeState::Deleted { kind: NodeKind::Folder, at },
                None => NodeState::Folder { title: Lww::new(format!("t{t}"), title_at), extra },
            };
            BookmarkRecord { id, placement, added_ms, state }
        },
    )
}

fn url_record(id: BookmarkId) -> impl Strategy<Value = BookmarkRecord> {
    (placement(), prop::option::weighted(0.3, stamp()), stamp(), 0u8..3, stamp(), 0u8..2).prop_map(
        move |(placement, deleted, title_at, t, url_at, u)| {
            let state = match deleted {
                Some(at) => NodeState::Deleted { kind: NodeKind::Url, at },
                None => NodeState::Url { title: Lww::new(format!("t{t}"), title_at), url: Lww::new(url(u), url_at), extra: Default::default() },
            };
            BookmarkRecord { id, placement, added_ms: 0, state }
        },
    )
}

fn visit() -> impl Strategy<Value = Visit> {
    (0i64..200, 1u64..3, prop::sample::select(vec![Transition::Link, Transition::Typed]))
        .prop_map(|(at_ms, d, transition)| Visit { at_ms, device: DeviceId(d), transition })
}

fn page_record() -> impl Strategy<Value = PageRecord> {
    (stamp(), 0u8..3, prop::collection::btree_set(visit(), 0..80), extra())
        .prop_map(|(at, t, visits, extra)| PageRecord { url: url(0), title: Lww::new(format!("t{t}"), at), visits, extra })
}

/// Bodies differ under one id: honest devices never mint that, but a server can send it.
fn directive_record() -> impl Strategy<Value = DeletionDirective> {
    (prop::option::of(0u8..2), 0i64..20, 0i64..20).prop_map(|(u, a, b)| DeletionDirective {
        id: uuid::Uuid::from_u128(9),
        url: u.map(url),
        from_ms: a.min(b),
        to_ms: a.max(b),
    })
}

fn session_record() -> impl Strategy<Value = DeviceSessionRecord> {
    (stamp(), prop::option::of(0u8..2)).prop_map(|(at, s)| DeviceSessionRecord {
        device: DeviceId(1),
        session: Lww::new(s.map(|n| SessionSnapshot { device_name: format!("d{n}"), windows: vec![], active_window: 0 }), at),
    })
}

fn extension_record() -> impl Strategy<Value = ExtensionRecord> {
    (stamp(), any::<bool>(), stamp(), any::<bool>(), stamp(), any::<bool>(), extra()).prop_map(
        |(s_at, s, i_at, i, e_at, e, extra)| ExtensionRecord {
            id: ext(),
            store: Lww::new(if s { StoreRef::ChromeWebStore } else { StoreRef::Amo }, s_at),
            installed: Lww::new(i, i_at),
            enabled: Lww::new(e, e_at),
            extra,
        },
    )
}

fn json_value() -> impl Strategy<Value = Option<JsonText>> {
    let value = prop_oneof![Just(serde_json::Value::Null), (0u8..3).prop_map(serde_json::Value::from)];
    prop::option::of(value.prop_map(|v| JsonText::from_value(&v)))
}

fn storage_record() -> impl Strategy<Value = SyncItemRecord> {
    (stamp(), json_value()).prop_map(|(at, v)| SyncItemRecord { ext: ext(), key: "k".to_owned(), value: Lww::new(v, at) })
}

fn pref_record() -> impl Strategy<Value = PrefRecord> {
    (stamp(), json_value()).prop_map(|(at, v)| PrefRecord { key: "theme".to_owned(), value: Lww::new(v, at) })
}

fn site_permission_record() -> impl Strategy<Value = SitePermissionRecord> {
    let setting = prop::option::of(prop::sample::select(vec![Setting::Allow, Setting::Block]));
    (stamp(), setting).prop_map(|(at, s)| SitePermissionRecord { origin: origin(0), permission: Permission::Camera, setting: Lww::new(s, at) })
}

fn engine_record() -> impl Strategy<Value = EngineRecord> {
    (prop::option::weighted(0.3, stamp()), stamp(), 0u8..3, stamp(), prop::option::of(0u8..2), stamp(), 0u8..2, stamp(), extra()).prop_map(
        |(deleted, n_at, n, k_at, k, s_at, s, g_at, extra)| {
            let state = match deleted {
                Some(at) => Record::Tombstone(at),
                None => Record::Live(EngineFields {
                    name: Lww::new(format!("n{n}"), n_at),
                    keyword: Lww::new(k.map(|k| format!("k{k}")), k_at),
                    search_url: Lww::new(UrlTemplate(format!("https://s{s}.example/?q={{searchTerms}}")), s_at),
                    suggest_url: Lww::new(None, g_at),
                    extra,
                }),
            };
            EngineRecord { id: SearchEngineId("builtin:ddg".to_owned()), state }
        },
    )
}

proptest! {
    #[test]
    fn lattice_laws(
        (a, b, c) in (folder_record(small_id(0)), folder_record(small_id(0)), folder_record(small_id(0))),
        (ua, ub, uc) in (url_record(small_id(1)), url_record(small_id(1)), url_record(small_id(1))),
        (pa, pb, pc) in (page_record(), page_record(), page_record()),
        (da, db, dc) in (directive_record(), directive_record(), directive_record()),
        (sa, sb, sc) in (session_record(), session_record(), session_record()),
        (xa, xb, xc) in (extension_record(), extension_record(), extension_record()),
        (ia, ib, ic) in (storage_record(), storage_record(), storage_record()),
        (ra, rb, rc) in (pref_record(), pref_record(), pref_record()),
        (ea, eb, ec) in (engine_record(), engine_record(), engine_record()),
        (qa, qb, qc) in (site_permission_record(), site_permission_record(), site_permission_record()),
    ) {
        check_laws(a, b, c)?;
        check_laws(ua, ub, uc)?;
        check_laws(pa, pb, pc)?;
        check_laws(da, db, dc)?;
        check_laws(sa, sb, sc)?;
        check_laws(xa, xb, xc)?;
        check_laws(ia, ib, ic)?;
        check_laws(ra, rb, rc)?;
        check_laws(ea, eb, ec)?;
        check_laws(qa, qb, qc)?;
    }

    /// The stored form of a page is a deterministic function of the union, whatever the
    /// arrival order and whatever duplicate `(at_ms, device)` keys (the table's primary
    /// key) the records carry. `devices_converge` cannot reach that case: its devices have
    /// distinct ids, so only a copied profile or a foreign record produces it. Records are
    /// what a device can export: a page with no visits has no row and is never sent.
    #[test]
    fn page_records_converge_through_the_table(
        records in prop::collection::vec(page_record().prop_filter("exported pages have a visit", |p| !p.visits.is_empty()), 1..4),
        seed in any::<u64>(),
    ) {
        let base = 1_780_000_000_000u64;
        let mut a = Device::new(0, base);
        let mut b = Device::new(1, base);
        let wire = |r: &PageRecord| WireRecord { kind: Kind::HistoryPages, id: r.url.to_string(), body: serde_json::to_vec(r).unwrap() };
        let forward: Vec<WireRecord> = records.iter().map(wire).collect();
        let mut shuffled = forward.clone();
        shuffle(&mut shuffled, seed);
        for w in &forward {
            let report = a.profile.sync().apply(vec![w.clone()]).unwrap();
            prop_assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        }
        let report = b.profile.sync().apply(shuffled).unwrap();
        prop_assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        let export = a.export();
        prop_assert_eq!(&export, &b.export());

        // the stored form is a fixpoint: re-applying it merges nothing and marks nothing dirty
        a.upload(&mut Server::default(), usize::MAX);
        let replay: Vec<WireRecord> = export.iter().map(|((kind, id), body)| WireRecord { kind: *kind, id: id.clone(), body: body.clone() }).collect();
        let report = a.profile.sync().apply(replay).unwrap();
        prop_assert_eq!(report.merged, 0);
        prop_assert!(!a.pending_upload(), "the stored form echoes");
    }

    // -----------------------------------------------------------------------
    // Layer 2: materialize returns a valid tree for ANY record set
    // -----------------------------------------------------------------------

    #[test]
    fn materialize_always_valid(records in prop::collection::vec((0u8..12, placement(), any::<bool>(), any::<bool>()), 0..24)) {
        let mut map = BTreeMap::new();
        for (n, placement, is_folder, deleted) in records {
            let id = small_id(n);
            let kind = if is_folder { NodeKind::Folder } else { NodeKind::Separator };
            let state = match (deleted, is_folder) {
                (true, _) => NodeState::Deleted { kind, at: Stamp::ZERO },
                (false, true) => NodeState::Folder { title: Lww::new(String::new(), Stamp::ZERO), extra: Default::default() },
                (false, false) => NodeState::Separator { extra: Default::default() },
            };
            map.insert(id, BookmarkRecord { id, placement, added_ms: 0, state });
        }
        let tree = materialize(&map);

        let live: BTreeSet<_> = map.values().filter(|r| !r.state.is_deleted()).map(|r| r.id).collect();
        let mut seen = BTreeSet::new();
        let mut stack: Vec<BookmarkId> = VISIBLE_ROOTS.to_vec();
        while let Some(f) = stack.pop() {
            for &c in tree.children(f) {
                prop_assert!(seen.insert(c), "reachable twice");
                prop_assert_eq!(tree.parent(c), Some(f));
                let parent_is_folder = f.is_root() || matches!(map[&f].state, NodeState::Folder { .. });
                prop_assert!(parent_is_folder);
                stack.push(c);
            }
        }
        prop_assert_eq!(seen, live);
    }
}

/// Two devices hold different bodies under one directive id (a buggy or hostile server, or
/// a reused uuid). They settle on one copy instead of overwriting each other's upload
/// every round.
#[test]
fn differing_directive_bodies_converge() {
    let base = 1_780_000_000_000u64;
    let mut devices = [Device::new(0, base), Device::new(1, base)];
    let mut server = Server::default();
    let wire = |to_ms| {
        let d = DeletionDirective { id: uuid::Uuid::from_u128(9), url: None, from_ms: 0, to_ms };
        WireRecord { kind: Kind::HistoryDeletions, id: d.id.to_string(), body: serde_json::to_vec(&d).unwrap() }
    };
    for (d, to_ms) in devices.iter_mut().zip([10, 99]) {
        let report = d.profile.sync().apply(vec![wire(to_ms)]).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    }
    server.upload(vec![wire(10)]);
    settle(&mut devices, &mut server);
    let [a, b] = &mut devices;
    assert_eq!(a.export(), b.export());
}
