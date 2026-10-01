//! Bookmarks.
//!
//! Storage and sync unit: one [`BookmarkRecord`] per node. It has a random 128-bit id,
//! an immutable kind, one LWW `placement` register (parent + fractional position, set
//! together by every move), LWW title/url, and a terminal tombstone.
//!
//! Merge (`Lattice::join`) is per record and never has to produce a valid tree.
//! [`materialize`] is a pure, deterministic function from the set of records to a valid
//! tree (no cycles, no orphans, every parent a live folder). Devices that hold the same
//! records therefore show the same tree. Repairs are never written back as edits: if
//! every device wrote its own repair with its own stamp, they would fight forever.
//!
//! Rules `materialize` applies, in order:
//! 1. **Unknown or invalid parent** (not received yet, not a folder): treat as child of
//!    `OTHER`. This happens when a sync batch delivers a child before its parent. The
//!    next materialization fixes it once the parent arrives.
//! 2. **Cycles** from concurrent moves (A moves X into Y while B moves Y into X): the raw
//!    parent graph is functional (one parent per node), so each cycle is simple. The
//!    member whose placement stamp is greatest (the move that closed the cycle) loses
//!    its edge and hangs under `OTHER`. This is the state-based analogue of "the later
//!    move is skipped".
//! 3. **Orphans** (a live node whose parent is a tombstone, e.g. B added a bookmark to a
//!    folder A deleted concurrently): tombstones keep their placement, so walk up raw
//!    parents to the nearest live ancestor. New data is never lost.
//! 4. **Order**: siblings sort by `(position, id)`. Equal positions from concurrent
//!    inserts at the same spot tie-break on id, deterministically.
//!
//! Deletion semantics (tested exactly in `tests/convergence.rs`): removing a folder
//! tombstones the folder and every node that was in its subtree *on the deleting device
//! at that moment*. A node is dead iff some device deleted it, directly or as part of a
//! subtree it could see. Nodes added or moved in concurrently survive through rule 3.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crdt::{Extra, Lattice, Lww, Seq, Stamp, extra_max_stamp, join_extra};
use crate::db::{extra_col, extra_text, opt_stamp_col, seq_col, stamp_col, uuid_col};
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile, Url};

/// Random v4 UUID. The four roots are fixed, identical on every device, and never
/// stored: they exist implicitly at `Stamp::ZERO`, so first sync never produces
/// duplicate roots and there is no seeding write to conflict on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BookmarkId(pub Uuid);

impl BookmarkId {
    /// Invisible parent of the three visible roots.
    pub const ROOT: BookmarkId = BookmarkId(Uuid::from_u128(1));
    pub const TOOLBAR: BookmarkId = BookmarkId(Uuid::from_u128(2));
    /// "Other bookmarks". Also where rule 1 and rule 2 put nodes.
    pub const OTHER: BookmarkId = BookmarkId(Uuid::from_u128(3));
    pub const MOBILE: BookmarkId = BookmarkId(Uuid::from_u128(4));

    const VISIBLE_ROOTS: [BookmarkId; 3] = [BookmarkId::TOOLBAR, BookmarkId::OTHER, BookmarkId::MOBILE];

    /// The four roots. The nil UUID is not one of them.
    pub fn is_root(self) -> bool {
        matches!(self.0.as_u128(), 1..=4)
    }

    /// A root that can hold children (`ROOT` cannot: its children are the three visible roots).
    pub fn is_visible_root(self) -> bool {
        matches!(self.0.as_u128(), 2..=4)
    }

    pub(crate) fn random() -> Self {
        BookmarkId(Uuid::new_v4())
    }

    fn root_title(self) -> &'static str {
        match self.0.as_u128() {
            2 => "Bookmarks bar",
            3 => "Other bookmarks",
            4 => "Mobile bookmarks",
            _ => "",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Folder,
    Url,
    Separator,
}

impl NodeKind {
    pub(crate) fn code(self) -> i64 {
        match self {
            NodeKind::Folder => 0,
            NodeKind::Url => 1,
            NodeKind::Separator => 2,
        }
    }

    pub(crate) fn from_code(c: i64) -> Option<NodeKind> {
        match c {
            0 => Some(NodeKind::Folder),
            1 => Some(NodeKind::Url),
            2 => Some(NodeKind::Separator),
            _ => None,
        }
    }
}

/// Fractional index: a non-empty base-62 string (`0-9A-Za-z`, ASCII order) that never
/// ends in `'0'`, so a key strictly between any two distinct keys always exists.
/// Compared bytewise.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Position(String);

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn digit_value(c: u8) -> Option<usize> {
    DIGITS.iter().position(|&d| d == c)
}

/// Midpoint of two digit strings, `a < b`, neither ending in `'0'`; `a = ""` is the low
/// end and `b = None` the high end. A loop, not recursion: synced keys can be any length.
fn midpoint(mut a: &[u8], mut b: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        if let Some(bb) = b {
            let mut n = 0;
            while n < bb.len() && a.get(n).copied().unwrap_or(b'0') == bb[n] {
                n += 1;
            }
            if n > 0 {
                out.extend_from_slice(&bb[..n]);
                a = &a[n.min(a.len())..];
                b = Some(&bb[n..]);
                continue;
            }
        }
        let da = a.first().and_then(|&c| digit_value(c)).unwrap_or(0);
        let db = b.and_then(|b| b.first()).and_then(|&c| digit_value(c)).unwrap_or(DIGITS.len());
        if db - da > 1 {
            out.push(DIGITS[(da + db).div_ceil(2)]);
            return out;
        }
        match b {
            Some(bb) if bb.len() > 1 => {
                out.push(bb[0]);
                return out;
            }
            _ => {
                out.push(DIGITS[da]);
                a = a.get(1..).unwrap_or(&[]);
                b = None;
            }
        }
    }
}

/// The key after `lo` when appending at the end, kept short. A pure midpoint towards
/// the open end converges on `z` within a few steps and then grows one char per five
/// appends. Instead, the run of leading `z`s acts as a length prefix: level k keys are
/// `z^k` + k+1 digits (first digit below `z`), counted up in base 62, so a level holds
/// about 61·62^k keys before the next `z` is needed. Any key still compares as a plain
/// string, and [`midpoint`] still works between any two of them.
fn successor(lo: &[u8]) -> Vec<u8> {
    let k = lo.iter().take_while(|&&c| c == b'z').count();
    let rest = &lo[k..];
    if rest.is_empty() {
        let mut out = lo.to_vec();
        out.push(b'1');
        return out;
    }
    let mut digits: Vec<usize> = rest.iter().map(|&c| digit_value(c).unwrap_or(0)).collect();
    let mut i = digits.len();
    let overflow = loop {
        if i == 0 {
            break true;
        }
        i -= 1;
        let max = if i == 0 { DIGITS.len() - 2 } else { DIGITS.len() - 1 };
        if digits[i] < max {
            digits[i] += 1;
            break false;
        }
        digits[i] = 0;
    };
    let mut out = vec![b'z'; k];
    if overflow {
        out.push(b'z');
        out.push(b'1');
        out.extend(std::iter::repeat_n(b'0', rest.len().saturating_sub(1)));
        out.push(b'1');
        return out;
    }
    if *digits.last().expect("non-empty") == 0 {
        *digits.last_mut().expect("non-empty") = 1;
    }
    out.extend(digits.into_iter().map(|d| DIGITS[d]));
    out
}

impl Position {
    /// A key strictly between `lo` and `hi` (`None` = open end), as short as possible.
    /// Precondition: `lo < hi`. When the neighbours have *equal* keys (left by concurrent
    /// inserts), the caller passes `hi = None` for the run and the new node lands after
    /// the run. That is a harmless one-slot misplacement in a rare case.
    pub fn between(lo: Option<&Position>, hi: Option<&Position>) -> Position {
        let a = lo.map(|p| p.0.as_bytes()).unwrap_or(&[]);
        let b = hi.map(|p| p.0.as_bytes()).filter(|b| *b > a);
        let out = match b {
            Some(b) => midpoint(a, Some(b)),
            None => successor(a),
        };
        Position(String::from_utf8(out).expect("base-62 digits are ASCII"))
    }

    /// Boundary constructor for wire/DB values: non-empty, base-62, no trailing `'0'`.
    pub fn parse(s: &str) -> Option<Position> {
        let ok = !s.is_empty() && s.bytes().all(|c| digit_value(c).is_some()) && !s.ends_with('0');
        ok.then(|| Position(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Position {
    type Error = InvalidRecord;
    fn try_from(s: String) -> Result<Self, InvalidRecord> {
        Position::parse(&s).ok_or(InvalidRecord("malformed position"))
    }
}

impl From<Position> for String {
    fn from(p: Position) -> String {
        p.0
    }
}

/// Parent and position live in one register because a move sets both. Two registers
/// could merge into a position from one move and a parent from another.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Placement {
    pub parent: BookmarkId,
    pub pos: Position,
}

/// The complete mergeable state of one node: the unit of storage (one `bookmarks` row)
/// and of sync (one `WireRecord`). Serialized through [`BookmarkWire`], which is where
/// incoming records are validated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BookmarkWire", into = "BookmarkWire")]
pub struct BookmarkRecord {
    pub id: BookmarkId,
    /// Kept on tombstones: rule 3 walks tombstones' parents.
    pub placement: Lww<Placement>,
    /// Earliest creation time seen (min-merge). Display only.
    pub added_ms: i64,
    pub state: NodeState,
}

impl BookmarkRecord {
    pub(crate) fn max_stamp(&self) -> Stamp {
        let mut m = self.placement.at;
        let mut bump = |s: Stamp| {
            if s > m {
                m = s;
            }
        };
        match &self.state {
            NodeState::Folder { title, extra } => {
                bump(title.at);
                extra_max_stamp(extra).map(&mut bump);
            }
            NodeState::Url { title, url, extra } => {
                bump(title.at);
                bump(url.at);
                extra_max_stamp(extra).map(&mut bump);
            }
            NodeState::Separator { extra } => {
                extra_max_stamp(extra).map(&mut bump);
            }
            NodeState::Deleted { at, .. } => bump(*at),
        }
        m
    }

    fn title(&self) -> Option<&Lww<String>> {
        match &self.state {
            NodeState::Folder { title, .. } | NodeState::Url { title, .. } => Some(title),
            _ => None,
        }
    }

    fn url(&self) -> Option<&Lww<Url>> {
        match &self.state {
            NodeState::Url { url, .. } => Some(url),
            _ => None,
        }
    }
}

/// The kind is the variant, so it can never change for an id. A remote record whose
/// kind disagrees with the local one is rejected at the boundary (`BookmarkWire`) and
/// never merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeState {
    Folder { title: Lww<String>, extra: Extra },
    Url { title: Lww<String>, url: Lww<Url>, extra: Extra },
    Separator { extra: Extra },
    /// Terminal. Absorbs every live state. Title and url are dropped: deleted bookmarks
    /// should not linger, and a tombstone's fields can never be observed again.
    Deleted { kind: NodeKind, at: Stamp },
}

impl NodeState {
    pub fn kind(&self) -> NodeKind {
        match self {
            NodeState::Folder { .. } => NodeKind::Folder,
            NodeState::Url { .. } => NodeKind::Url,
            NodeState::Separator { .. } => NodeKind::Separator,
            NodeState::Deleted { kind, .. } => *kind,
        }
    }

    pub fn is_deleted(&self) -> bool {
        matches!(self, NodeState::Deleted { .. })
    }
}

impl Lattice for BookmarkRecord {
    /// placement: LWW. added_ms: min. state: Deleted absorbs live (max `at` between two
    /// tombstones), live+live joins field-wise. Different kinds cannot reach here
    /// (validated at the boundary; debug_assert).
    fn join(&mut self, other: Self) {
        debug_assert_eq!(self.id, other.id, "join is per record");
        debug_assert_eq!(self.state.kind(), other.state.kind(), "kind mismatch is rejected at the boundary");
        self.placement.join(other.placement);
        self.added_ms = self.added_ms.min(other.added_ms);
        match (&mut self.state, other.state) {
            (NodeState::Deleted { at, .. }, NodeState::Deleted { at: theirs, .. }) => {
                if theirs > *at {
                    *at = theirs;
                }
            }
            (NodeState::Deleted { .. }, _) => {}
            (mine, NodeState::Deleted { at, .. }) => *mine = NodeState::Deleted { kind: mine.kind(), at },
            (NodeState::Folder { title, extra }, NodeState::Folder { title: t, extra: e }) => {
                title.join(t);
                join_extra(extra, e);
            }
            (NodeState::Url { title, url, extra }, NodeState::Url { title: t, url: u, extra: e }) => {
                title.join(t);
                url.join(u);
                join_extra(extra, e);
            }
            (NodeState::Separator { extra }, NodeState::Separator { extra: e }) => join_extra(extra, e),
            (_, _) => {}
        }
    }
}

/// Wire/DB shape. Flat, every mutable field `{"v","at"}`, unknown fields captured in
/// `extra` (see `crdt::Extra`). `TryFrom` is the validation boundary: kind/field
/// agreement, the id is not a root, the url parses, the position is well-formed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookmarkWire {
    pub id: BookmarkId,
    pub kind: NodeKind,
    pub placement: Lww<Placement>,
    pub added_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Lww<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<Lww<Url>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Stamp>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl TryFrom<BookmarkWire> for BookmarkRecord {
    type Error = InvalidRecord;
    fn try_from(w: BookmarkWire) -> Result<Self, InvalidRecord> {
        if w.id.is_root() {
            return Err(InvalidRecord("a root is not a record"));
        }
        let state = match (w.deleted, w.kind, w.title, w.url) {
            (Some(at), kind, _, _) => NodeState::Deleted { kind, at },
            (None, NodeKind::Folder, Some(title), None) => NodeState::Folder { title, extra: w.extra },
            (None, NodeKind::Url, Some(title), Some(url)) => NodeState::Url { title, url, extra: w.extra },
            (None, NodeKind::Separator, None, None) => NodeState::Separator { extra: w.extra },
            (None, _, _, _) => return Err(InvalidRecord("fields do not match the kind")),
        };
        Ok(BookmarkRecord { id: w.id, placement: w.placement, added_ms: w.added_ms, state })
    }
}

impl From<BookmarkRecord> for BookmarkWire {
    fn from(r: BookmarkRecord) -> Self {
        let kind = r.state.kind();
        let (title, url, deleted, extra) = match r.state {
            NodeState::Folder { title, extra } => (Some(title), None, None, extra),
            NodeState::Url { title, url, extra } => (Some(title), Some(url), None, extra),
            NodeState::Separator { extra } => (None, None, None, extra),
            NodeState::Deleted { at, .. } => (None, None, Some(at), Extra::default()),
        };
        BookmarkWire { id: r.id, kind, placement: r.placement, added_ms: r.added_ms, title, url, deleted, extra }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid bookmark record: {0}")]
pub struct InvalidRecord(pub &'static str);

// ---------------------------------------------------------------------------
// Materialized view (what the shells see)
// ---------------------------------------------------------------------------

/// A live node as the UI sees it. `parent` and `index` are *effective* values from
/// [`materialize`], not the raw placement register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkNode {
    pub id: BookmarkId,
    pub kind: NodeKind,
    pub parent: BookmarkId,
    pub index: usize,
    /// Empty for separators.
    pub title: String,
    pub url: Option<Url>,
    pub added_ms: i64,
}

/// In-memory state held by `Profile`. `records` is the source of truth (mirrors the
/// `bookmarks` table). `tree` is derived from it by `materialize` after every change.
pub(crate) struct Model {
    pub(crate) records: BTreeMap<BookmarkId, BookmarkRecord>,
    pub(crate) tree: Tree,
}

impl Model {
    pub(crate) fn new(records: BTreeMap<BookmarkId, BookmarkRecord>) -> Model {
        let tree = materialize(&records);
        Model { records, tree }
    }

    pub(crate) fn rematerialize(&mut self) {
        self.tree = materialize(&self.records);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// Every live node except ROOT: effective parent.
    parent: HashMap<BookmarkId, BookmarkId>,
    /// Every live folder, roots included: ordered children.
    children: HashMap<BookmarkId, Vec<BookmarkId>>,
    /// Exact serialized-URL match; the star button's lookup.
    by_url: HashMap<Url, Vec<BookmarkId>>,
}

impl Tree {
    pub fn parent(&self, id: BookmarkId) -> Option<BookmarkId> {
        self.parent.get(&id).copied()
    }
    pub fn children(&self, folder: BookmarkId) -> &[BookmarkId] {
        self.children.get(&folder).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn ids_for_url(&self, url: &Url) -> &[BookmarkId] {
        self.by_url.get(url).map(Vec::as_slice).unwrap_or(&[])
    }
    /// Whether a live bookmark has this serialized origin (`https://example.com`).
    pub(crate) fn has_origin(&self, origin: &str) -> bool {
        self.by_url.keys().any(|url| url.origin().ascii_serialization() == origin)
    }
    /// A live folder (the visible roots included), i.e. a valid target for adds and moves.
    pub fn is_live_folder(&self, id: BookmarkId) -> bool {
        id.is_visible_root() || (!id.is_root() && self.children.contains_key(&id))
    }
    /// Effective ancestry test used to reject local moves that would cycle.
    pub fn is_ancestor(&self, ancestor: BookmarkId, of: BookmarkId) -> bool {
        let mut cur = of;
        while let Some(&p) = self.parent.get(&cur) {
            if p == ancestor {
                return true;
            }
            cur = p;
        }
        false
    }
}

/// Rule 1: the raw parent of a record is its placement parent when that is a visible
/// root or a folder record (live or tombstoned), else `OTHER`.
fn raw_parent(records: &BTreeMap<BookmarkId, BookmarkRecord>, rec: &BookmarkRecord) -> BookmarkId {
    let p = rec.placement.v.parent;
    let valid = p.is_visible_root() || (p != rec.id && records.get(&p).is_some_and(|r| r.state.kind() == NodeKind::Folder));
    if valid { p } else { BookmarkId::OTHER }
}

/// Pure, deterministic, O(n log n). Public so the property test can feed it arbitrary
/// record sets (including ones no sequence of API calls could produce) and check that
/// the output is always a valid tree.
///
/// ```text
/// raw(n)   = n.placement.parent if that id is a visible root or a record of kind Folder, else OTHER    -- rule 1
/// for each cycle in the raw graph (walk with white/grey/black colouring):
///     loser = member with max (placement.at, id); raw(loser) = OTHER                            -- rule 2
/// for each live n: eff(n) = first live node on raw(n), raw(raw(n)), ...                         -- rule 3
///                  (terminates: after rule 2 every raw chain ends at a root)
/// children(f) = live n with eff(n) = f, sorted by (placement.pos, id)                            -- rule 4
/// ```
pub fn materialize(records: &BTreeMap<BookmarkId, BookmarkRecord>) -> Tree {
    let mut raw: HashMap<BookmarkId, BookmarkId> = records.iter().map(|(&id, r)| (id, raw_parent(records, r))).collect();

    // Rule 2. 0 = white, 1 = grey (on the current path), 2 = black.
    let mut colour: HashMap<BookmarkId, u8> = HashMap::with_capacity(records.len());
    let mut path: Vec<BookmarkId> = Vec::new();
    for &start in records.keys() {
        if colour.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        path.clear();
        let mut cur = start;
        loop {
            if cur.is_visible_root() {
                break;
            }
            match colour.get(&cur).copied().unwrap_or(0) {
                2 => break,
                1 => {
                    let first = path.iter().position(|&n| n == cur).expect("grey nodes are on the path");
                    let loser = path[first..]
                        .iter()
                        .copied()
                        .max_by_key(|&n| (records[&n].placement.at, n))
                        .expect("a cycle has members");
                    raw.insert(loser, BookmarkId::OTHER);
                    break;
                }
                _ => {
                    colour.insert(cur, 1);
                    path.push(cur);
                    cur = raw[&cur];
                }
            }
        }
        for &n in &path {
            colour.insert(n, 2);
        }
    }

    // Rule 3: anchor(n) = first live folder or root strictly above n along raw parents.
    let mut anchor: HashMap<BookmarkId, BookmarkId> = HashMap::with_capacity(records.len());
    let is_live_folder = |id: BookmarkId| records.get(&id).is_some_and(|r| matches!(r.state, NodeState::Folder { .. }));
    for &id in records.keys() {
        if anchor.contains_key(&id) {
            continue;
        }
        path.clear();
        path.push(id);
        let mut cur = raw[&id];
        loop {
            if cur.is_visible_root() || is_live_folder(cur) {
                break;
            }
            if let Some(&a) = anchor.get(&cur) {
                cur = a;
                break;
            }
            path.push(cur);
            cur = raw[&cur];
        }
        for &n in &path {
            anchor.insert(n, cur);
        }
    }

    // Rule 4.
    let mut tree = Tree::default();
    for root in BookmarkId::VISIBLE_ROOTS {
        tree.children.insert(root, Vec::new());
        tree.parent.insert(root, BookmarkId::ROOT);
    }
    tree.children.insert(BookmarkId::ROOT, BookmarkId::VISIBLE_ROOTS.to_vec());
    for (&id, rec) in records {
        match &rec.state {
            NodeState::Deleted { .. } => continue,
            NodeState::Folder { .. } => {
                tree.children.entry(id).or_default();
            }
            NodeState::Url { url, .. } => tree.by_url.entry(url.v.clone()).or_default().push(id),
            NodeState::Separator { .. } => {}
        }
        let parent = anchor[&id];
        tree.parent.insert(id, parent);
        tree.children.entry(parent).or_default().push(id);
    }
    for (folder, kids) in tree.children.iter_mut() {
        if *folder == BookmarkId::ROOT {
            continue;
        }
        kids.sort_by(|a, b| (&records[a].placement.v.pos, a).cmp(&(&records[b].placement.v.pos, b)));
    }
    tree
}

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InsertAt {
    Start,
    End,
    /// Index among the folder's current children, clamped to the end. This is what a
    /// drag-and-drop gives the shell.
    Index(usize),
}

#[derive(Debug, thiserror::Error)]
pub enum BookmarkError {
    #[error("no live bookmark {0:?}")]
    NotFound(BookmarkId),
    #[error("{0:?} is not a folder")]
    NotAFolder(BookmarkId),
    #[error("roots cannot be moved, renamed or removed")]
    IsRoot,
    #[error("a folder cannot be moved into itself or its descendants")]
    WouldCycle,
    #[error("only url bookmarks have a url")]
    NotAUrl,
}

pub struct Bookmarks<'p> {
    pub(crate) p: &'p mut Profile,
}

/// One node of an import, before ids and stamps exist.
struct Planned {
    id: BookmarkId,
    placement: Placement,
    added_ms: Option<i64>,
    kind: PlannedKind,
}

enum PlannedKind {
    Folder { title: String },
    Url { title: String, url: Url },
    Separator,
}

impl Planned {
    fn record(self, at: Stamp, now_ms: i64) -> BookmarkRecord {
        let state = match self.kind {
            PlannedKind::Folder { title } => NodeState::Folder { title: Lww::new(title, at), extra: Extra::default() },
            PlannedKind::Url { title, url } => {
                NodeState::Url { title: Lww::new(title, at), url: Lww::new(url, at), extra: Extra::default() }
            }
            PlannedKind::Separator => NodeState::Separator { extra: Extra::default() },
        };
        BookmarkRecord {
            id: self.id,
            placement: Lww::new(self.placement, at),
            added_ms: self.added_ms.unwrap_or(now_ms),
            state,
        }
    }
}

impl Bookmarks<'_> {
    // --- reads: in-memory, no SQL ---------------------------------------------------

    fn model(&self) -> &Model {
        &self.p.bookmarks
    }

    fn node(&self, id: BookmarkId, parent: BookmarkId, index: usize) -> Option<BookmarkNode> {
        if id.is_visible_root() {
            return Some(BookmarkNode {
                id,
                kind: NodeKind::Folder,
                parent: BookmarkId::ROOT,
                index,
                title: id.root_title().to_owned(),
                url: None,
                added_ms: 0,
            });
        }
        let rec = self.model().records.get(&id)?;
        if rec.state.is_deleted() {
            return None;
        }
        Some(BookmarkNode {
            id,
            kind: rec.state.kind(),
            parent,
            index,
            title: rec.title().map(|t| t.v.clone()).unwrap_or_default(),
            url: rec.url().map(|u| u.v.clone()),
            added_ms: rec.added_ms,
        })
    }

    pub fn get(&self, id: BookmarkId) -> Option<BookmarkNode> {
        let tree = &self.model().tree;
        let parent = tree.parent(id)?;
        let index = tree.children(parent).iter().position(|&c| c == id)?;
        self.node(id, parent, index)
    }

    /// Children in display order. Access pattern 3 (bookmarks bar / menu).
    pub fn children(&self, folder: BookmarkId) -> Vec<BookmarkNode> {
        self.model()
            .tree
            .children(folder)
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| self.node(id, folder, i))
            .collect()
    }

    /// Access pattern 1: runs on every committed navigation. One hash lookup.
    pub fn is_bookmarked(&self, url: &Url) -> bool {
        !self.p.bookmarks.tree.ids_for_url(url).is_empty()
    }

    pub fn find_by_url(&self, url: &Url) -> Vec<BookmarkNode> {
        self.model().tree.ids_for_url(url).iter().filter_map(|&id| self.get(id)).collect()
    }

    /// Case-insensitive substring match on title and url, used by the omnibox. A linear
    /// scan of about 10^4 nodes costs under a millisecond. Title prefix matches rank
    /// first, then title substrings, then url substrings.
    pub fn search(&self, text: &str, limit: usize) -> Vec<BookmarkNode> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut hits: Vec<(u8, String, BookmarkId)> = self
            .model()
            .records
            .values()
            .filter(|r| !r.state.is_deleted() && r.state.kind() == NodeKind::Url)
            .filter_map(|r| {
                let title = r.title().map(|t| t.v.to_lowercase()).unwrap_or_default();
                let url = r.url().map(|u| u.v.as_str().to_lowercase()).unwrap_or_default();
                let rank = if title.starts_with(&needle) {
                    0
                } else if title.contains(&needle) {
                    1
                } else if url.contains(&needle) {
                    2
                } else {
                    return None;
                };
                Some((rank, title, r.id))
            })
            .collect();
        hits.sort();
        hits.into_iter().take(limit).filter_map(|(_, _, id)| self.get(id)).collect()
    }

    // --- writes: plan against memory, persist in one Tx, then swap memory -----------
    //
    // Each write: (1) validate against the effective tree, (2) build the new/changed
    // records, (3) `Profile::write` stores them with the tx's stamp and seq, (4) only
    // after commit, replace them in `Model.records` and re-run `materialize`. A failed
    // transaction leaves memory untouched.

    fn live_record(&self, id: BookmarkId) -> Result<&BookmarkRecord, BookmarkError> {
        if id.is_root() {
            return Err(BookmarkError::IsRoot);
        }
        match self.model().records.get(&id) {
            Some(r) if !r.state.is_deleted() => Ok(r),
            _ => Err(BookmarkError::NotFound(id)),
        }
    }

    fn folder_target(&self, parent: BookmarkId) -> Result<(), BookmarkError> {
        if self.model().tree.is_live_folder(parent) { Ok(()) } else { Err(BookmarkError::NotAFolder(parent)) }
    }

    /// A position for a new child of `parent` at `at`, ignoring `exclude` (the node
    /// being moved within the same folder).
    fn position_for(&self, parent: BookmarkId, at: InsertAt, exclude: Option<BookmarkId>) -> Position {
        let records = &self.model().records;
        let siblings: Vec<&Position> = self
            .model()
            .tree
            .children(parent)
            .iter()
            .filter(|&&c| Some(c) != exclude)
            .map(|c| &records[c].placement.v.pos)
            .collect();
        let index = match at {
            InsertAt::Start => 0,
            InsertAt::End => siblings.len(),
            InsertAt::Index(i) => i.min(siblings.len()),
        };
        let lo = index.checked_sub(1).map(|i| siblings[i]);
        let mut j = index;
        if let Some(lo) = lo {
            while j < siblings.len() && siblings[j] == lo {
                j += 1;
            }
        }
        let hi = siblings.get(j).copied();
        Position::between(lo, hi)
    }

    /// Store `build`'s records in one transaction, then replace them in memory and
    /// re-materialize. `build` receives the transaction's stamp and wall time.
    fn persist(&mut self, build: impl FnOnce(Stamp, i64) -> Vec<BookmarkRecord>) -> Result<(), Error> {
        let mut built = Vec::new();
        self.p.write(|tx| {
            let at = tx.stamp();
            let now = tx.now_ms() as i64;
            let recs = build(at, now);
            let seq = tx.seq();
            for r in &recs {
                store(&tx.sql, r, seq)?;
            }
            built = recs;
            Ok(())
        })?;
        for r in built {
            self.p.bookmarks.records.insert(r.id, r);
        }
        self.p.bookmarks.rematerialize();
        Ok(())
    }

    fn insert(&mut self, parent: BookmarkId, at: InsertAt, kind: PlannedKind) -> Result<BookmarkId, Error> {
        self.folder_target(parent)?;
        let planned = Planned { id: BookmarkId::random(), placement: Placement { parent, pos: self.position_for(parent, at, None) }, added_ms: None, kind };
        let id = planned.id;
        self.persist(|stamp, now| vec![planned.record(stamp, now)])?;
        Ok(id)
    }

    pub fn add_url(&mut self, parent: BookmarkId, at: InsertAt, title: &str, url: &Url) -> Result<BookmarkId, Error> {
        self.insert(parent, at, PlannedKind::Url { title: title.to_owned(), url: url.clone() })
    }

    pub fn add_folder(&mut self, parent: BookmarkId, at: InsertAt, title: &str) -> Result<BookmarkId, Error> {
        self.insert(parent, at, PlannedKind::Folder { title: title.to_owned() })
    }

    pub fn add_separator(&mut self, parent: BookmarkId, at: InsertAt) -> Result<BookmarkId, Error> {
        self.insert(parent, at, PlannedKind::Separator)
    }

    /// No-op (no stamp, no seq) when the title is unchanged.
    pub fn rename(&mut self, id: BookmarkId, title: &str) -> Result<(), Error> {
        let rec = self.live_record(id)?;
        let current = rec.title().ok_or(BookmarkError::NotFound(id))?;
        if current.v == title {
            return Ok(());
        }
        let mut rec = rec.clone();
        self.persist(|stamp, _| {
            match &mut rec.state {
                NodeState::Folder { title: t, .. } | NodeState::Url { title: t, .. } => {
                    t.set(title.to_owned(), stamp);
                }
                _ => {}
            }
            vec![rec]
        })
    }

    pub fn set_url(&mut self, id: BookmarkId, url: &Url) -> Result<(), Error> {
        let rec = self.live_record(id)?;
        let current = rec.url().ok_or(BookmarkError::NotAUrl)?;
        if &current.v == url {
            return Ok(());
        }
        let mut rec = rec.clone();
        self.persist(|stamp, _| {
            if let NodeState::Url { url: u, .. } = &mut rec.state {
                u.set(url.clone(), stamp);
            }
            vec![rec]
        })
    }

    /// Move between folders or reorder within one. Rejects `WouldCycle` if `id` is an
    /// ancestor of `parent` in the effective tree *or* in the raw placement graph (a raw
    /// path through an edge that rule 2 broke). The second check guarantees a local
    /// move can never lose its own cycle tie-break and land somewhere the user did not
    /// put it.
    pub fn move_to(&mut self, id: BookmarkId, parent: BookmarkId, at: InsertAt) -> Result<(), Error> {
        let rec = self.live_record(id)?.clone();
        self.folder_target(parent)?;
        if id == parent || self.model().tree.is_ancestor(id, parent) || self.on_raw_chain(id, parent) {
            return Err(BookmarkError::WouldCycle.into());
        }
        // Dropped back on its own slot: a new position would mint a stamp that re-uploads
        // the node and beats a concurrent move made elsewhere.
        if rec.placement.v.parent == parent {
            let children = self.model().tree.children(parent);
            if let Some(cur) = children.iter().position(|&c| c == id) {
                let others = children.len() - 1;
                let want = match at {
                    InsertAt::Start => 0,
                    InsertAt::End => others,
                    InsertAt::Index(i) => i.min(others),
                };
                if cur == want {
                    return Ok(());
                }
            }
        }
        let pos = self.position_for(parent, at, Some(id));
        let placement = Placement { parent, pos };
        if placement == rec.placement.v {
            return Ok(());
        }
        let mut rec = rec;
        self.persist(|stamp, _| {
            rec.placement.set(placement, stamp);
            vec![rec]
        })
    }

    /// Whether `id` lies on the raw placement chain above `from`. The raw graph may
    /// contain cycles that do not involve `id`, hence the visited set.
    fn on_raw_chain(&self, id: BookmarkId, from: BookmarkId) -> bool {
        let records = &self.model().records;
        let mut visited = HashSet::new();
        let mut cur = from;
        while !cur.is_visible_root() && visited.insert(cur) {
            if cur == id {
                return true;
            }
            match records.get(&cur) {
                Some(r) => cur = raw_parent(records, r),
                None => return false,
            }
        }
        false
    }

    /// Tombstones `id` and its whole effective subtree with one stamp.
    pub fn remove(&mut self, id: BookmarkId) -> Result<(), Error> {
        self.live_record(id)?;
        let mut ids = vec![id];
        let mut i = 0;
        while i < ids.len() {
            ids.extend_from_slice(self.model().tree.children(ids[i]));
            i += 1;
        }
        let records = &self.model().records;
        let mut recs: Vec<BookmarkRecord> = ids.iter().map(|n| records[n].clone()).collect();
        self.persist(|stamp, _| {
            for r in &mut recs {
                r.state = NodeState::Deleted { kind: r.state.kind(), at: stamp };
            }
            recs
        })
    }

    /// Bulk insert (import from another browser) in one transaction with one
    /// materialization, instead of n full re-materializations.
    pub fn import(&mut self, parent: BookmarkId, items: Vec<ImportItem>) -> Result<usize, Error> {
        self.folder_target(parent)?;
        let mut planned = Vec::new();
        let records = &self.model().records;
        let last = self.model().tree.children(parent).last().map(|c| records[c].placement.v.pos.clone());
        plan_import(parent, last, items, &mut planned);
        let count = planned.len();
        if count == 0 {
            return Ok(0);
        }
        self.persist(|stamp, now| planned.into_iter().map(|p| p.record(stamp, now)).collect())?;
        Ok(count)
    }

    /// Imports `items` into a new folder called `title` at the end of the bookmarks bar, where
    /// the user sees them at once. Returns how many items it added, not counting that folder;
    /// nothing is written when `items` is empty.
    pub fn import_folder(&mut self, title: &str, items: Vec<ImportItem>) -> Result<usize, Error> {
        if items.is_empty() {
            return Ok(0);
        }
        let folder = ImportItem::Folder { title: title.to_owned(), children: items };
        Ok(self.import(BookmarkId::TOOLBAR, vec![folder])? - 1)
    }
}

fn plan_import(parent: BookmarkId, mut prev: Option<Position>, items: Vec<ImportItem>, out: &mut Vec<Planned>) {
    for item in items {
        let pos = Position::between(prev.as_ref(), None);
        prev = Some(pos.clone());
        let id = BookmarkId::random();
        let placement = Placement { parent, pos };
        match item {
            ImportItem::Url { title, url, added_ms } => {
                out.push(Planned { id, placement, added_ms, kind: PlannedKind::Url { title, url } });
            }
            ImportItem::Folder { title, children } => {
                out.push(Planned { id, placement, added_ms: None, kind: PlannedKind::Folder { title } });
                plan_import(id, None, children, out);
            }
            ImportItem::Separator => out.push(Planned { id, placement, added_ms: None, kind: PlannedKind::Separator }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportItem {
    /// `added_ms` is `None` when the source did not record it; the import time is used.
    Url { title: String, url: Url, added_ms: Option<i64> },
    Folder { title: String, children: Vec<ImportItem> },
    Separator,
}

// ---------------------------------------------------------------------------
// Storage (bookmarks table) and sync plumbing
// ---------------------------------------------------------------------------

const COLUMNS: &str = "id, kind, parent, position, placement_at, added_ms, title, title_at, url, url_at, deleted_at, extra, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, BookmarkRecord), rusqlite::Error> {
    let bad = |what| crate::db::bad_column(0, what);
    let id = BookmarkId(uuid_col(row, 0)?);
    let kind = NodeKind::from_code(row.get(1)?).ok_or_else(|| bad("kind"))?;
    let parent = BookmarkId(uuid_col(row, 2)?);
    let pos = Position::parse(&row.get::<_, String>(3)?).ok_or_else(|| bad("position"))?;
    let placement = Lww::new(Placement { parent, pos }, stamp_col(row, 4)?);
    let added_ms: i64 = row.get(5)?;
    let title: Option<String> = row.get(6)?;
    let title_at = opt_stamp_col(row, 7)?;
    let url: Option<String> = row.get(8)?;
    let url_at = opt_stamp_col(row, 9)?;
    let deleted_at = opt_stamp_col(row, 10)?;
    let extra = extra_col(row, 11)?;
    let seq = seq_col(row, 12)?;
    let title = title.zip(title_at).map(|(v, at)| Lww::new(v, at));
    let url = match url.zip(url_at) {
        Some((v, at)) => Some(Lww::new(Url::parse(&v).map_err(|_| bad("url"))?, at)),
        None => None,
    };
    let state = match (deleted_at, kind, title, url) {
        (Some(at), kind, _, _) => NodeState::Deleted { kind, at },
        (None, NodeKind::Folder, Some(title), _) => NodeState::Folder { title, extra },
        (None, NodeKind::Url, Some(title), Some(url)) => NodeState::Url { title, url, extra },
        (None, NodeKind::Separator, _, _) => NodeState::Separator { extra },
        _ => return Err(bad("kind/field agreement")),
    };
    Ok((seq, BookmarkRecord { id, placement, added_ms, state }))
}

pub(crate) fn load_all(conn: &rusqlite::Connection) -> Result<BTreeMap<BookmarkId, BookmarkRecord>, Error> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM bookmarks"))?;
    let rows = stmt.query_map([], row_record)?;
    let mut out = BTreeMap::new();
    for r in rows {
        let (_, rec) = r?;
        out.insert(rec.id, rec);
    }
    Ok(out)
}

pub(crate) fn store(tx: &rusqlite::Transaction<'_>, rec: &BookmarkRecord, seq: Seq) -> Result<(), Error> {
    let (title, url, deleted_at, extra) = match &rec.state {
        NodeState::Folder { title, extra } => (Some(title), None, None, extra_text(extra)),
        NodeState::Url { title, url, extra } => (Some(title), Some(url), None, extra_text(extra)),
        NodeState::Separator { extra } => (None, None, None, extra_text(extra)),
        NodeState::Deleted { at, .. } => (None, None, Some(at.to_vec()), "{}".to_owned()),
    };
    tx.execute(
        &format!(
            "INSERT OR REPLACE INTO bookmarks ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        ),
        params![
            rec.id.0.as_bytes().as_slice(),
            rec.state.kind().code(),
            rec.placement.v.parent.0.as_bytes().as_slice(),
            rec.placement.v.pos.as_str(),
            rec.placement.at.to_vec(),
            rec.added_ms,
            title.map(|t| t.v.as_str()),
            title.map(|t| t.at.to_vec()),
            url.map(|u| u.v.as_str()),
            url.map(|u| u.at.to_vec()),
            deleted_at,
            extra,
            seq.0 as i64,
        ],
    )?;
    Ok(())
}

pub(crate) struct BookmarksTable;

impl SyncTable for BookmarksTable {
    const KIND: Kind = Kind::Bookmarks;
    type Record = BookmarkRecord;

    fn wire_id(rec: &BookmarkRecord) -> String {
        rec.id.0.to_string()
    }

    fn max_stamp(rec: &BookmarkRecord) -> Option<Stamp> {
        Some(rec.max_stamp())
    }

    fn compatible(local: &BookmarkRecord, incoming: &BookmarkRecord) -> Result<(), &'static str> {
        if local.state.kind() == incoming.state.kind() { Ok(()) } else { Err("kind differs from the local record") }
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<BookmarkRecord>, Error> {
        let Ok(id) = Uuid::parse_str(wire_id) else { return Ok(None) };
        let rec = tx
            .query_row(&format!("SELECT {COLUMNS} FROM bookmarks WHERE id = ?1"), [id.as_bytes().as_slice()], row_record)
            .optional()?;
        Ok(rec.map(|(_, r)| r))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &BookmarkRecord, seq: Seq) -> Result<(), Error> {
        store(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, BookmarkRecord)>, bool), Error> {
        changed_rows(conn, "bookmarks", COLUMNS, "1", since, limit, row_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(s: &str) -> Position {
        Position::parse(s).unwrap()
    }

    #[test]
    fn between_is_strictly_inside_and_well_formed() {
        let cases: &[(Option<&str>, Option<&str>)] = &[
            (None, None),
            (None, Some("V")),
            (Some("V"), None),
            (Some("V"), Some("k")),
            (Some("k"), Some("kV")),
            (Some("1"), Some("2")),
            (None, Some("1")),
            (Some("1z"), Some("2")),
            (Some("zz"), None),
            (Some("kV"), Some("kW")),
            (Some("k"), Some("k1")),
        ];
        for (lo, hi) in cases {
            let lo = lo.map(pos);
            let hi = hi.map(pos);
            let mid = Position::between(lo.as_ref(), hi.as_ref());
            assert!(Position::parse(mid.as_str()).is_some(), "{mid:?} malformed");
            if let Some(lo) = &lo {
                assert!(*lo < mid, "{lo:?} < {mid:?}");
            }
            if let Some(hi) = &hi {
                assert!(mid < *hi, "{mid:?} < {hi:?}");
            }
        }
        assert_eq!(Position::between(None, None).as_str(), "1");
        assert_eq!(Position::between(Some(&pos("y")), None).as_str(), "z11");
        assert_eq!(Position::between(Some(&pos("z1z")), None).as_str(), "z21");
        assert_eq!(Position::between(Some(&pos("zyz")), None).as_str(), "zz101");
        assert_eq!(Position::between(Some(&pos("zz")), None).as_str(), "zz1");
        assert_eq!(Position::between(Some(&pos("zV")), None).as_str(), "zW");
    }

    #[test]
    fn between_stays_short_when_appending() {
        let mut prev: Option<Position> = None;
        let mut longest = 0;
        for _ in 0..20_000 {
            let next = Position::between(prev.as_ref(), None);
            assert!(Position::parse(next.as_str()).is_some(), "{next:?} malformed");
            assert!(prev.as_ref().is_none_or(|p| *p < next), "{prev:?} < {next:?}");
            longest = longest.max(next.as_str().len());
            prev = Some(next);
        }
        assert!(longest <= 6, "longest appended key has {longest} chars");
    }

    /// A synced key can be any length. Its length must not drive the stack depth, or a
    /// forged pair of neighbours crashes the browser when the user drops between them.
    #[test]
    fn between_long_z_run_does_not_overflow_stack() {
        let lo = pos(&format!("y{}1", "z".repeat(200_000)));
        let hi = pos("z");
        let mid = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn({
                let (lo, hi) = (lo.clone(), hi.clone());
                move || Position::between(Some(&lo), Some(&hi))
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(Position::parse(mid.as_str()).is_some(), "malformed");
        assert!(lo < mid && mid < hi);
    }

    /// Rule 1: only a visible root or a folder is a valid raw parent, so a record placed
    /// under the invisible ROOT lands in OTHER.
    #[test]
    fn a_record_placed_under_root_lands_in_other() {
        let id = BookmarkId(Uuid::from_u128(100));
        let placement = Lww::new(Placement { parent: BookmarkId::ROOT, pos: pos("1") }, Stamp::ZERO);
        let state = NodeState::Separator { extra: Extra::default() };
        let tree = materialize(&BTreeMap::from([(id, BookmarkRecord { id, placement, added_ms: 0, state })]));
        assert_eq!(tree.parent(id), Some(BookmarkId::OTHER));
        assert_eq!(tree.children(BookmarkId::ROOT), BookmarkId::VISIBLE_ROOTS);
    }

    #[test]
    fn parse_rejects_bad_positions() {
        assert!(Position::parse("").is_none());
        assert!(Position::parse("a0").is_none());
        assert!(Position::parse("a-").is_none());
        assert!(Position::parse("0").is_none());
        assert!(Position::parse("01").is_some());
    }

    #[test]
    fn roots_exclude_nil() {
        assert!(!BookmarkId(Uuid::nil()).is_root());
        assert!(BookmarkId::ROOT.is_root());
        assert!(BookmarkId::MOBILE.is_root());
        assert!(!BookmarkId(Uuid::from_u128(5)).is_root());
        assert!(!BookmarkId::ROOT.is_visible_root());
    }
}
