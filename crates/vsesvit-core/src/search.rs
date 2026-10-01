//! Search engines and the omnibox.
//!
//! Engines are identity-keyed records with LWW fields and a terminal tombstone, like
//! bookmarks: [`EngineRecord`] is `Record<EngineFields>`, either live or a tombstone,
//! never both. Built-in engines ship in code with fixed ids (`builtin:ddg`, ...) at
//! `Stamp::ZERO` and are never seeded into the database. Editing a built-in stores a
//! record for that id whose edited fields carry real stamps; its unedited fields stay at
//! `Stamp::ZERO` and read through to the code values. That way two devices never race to
//! seed the same rows, and a built-in's URL can be updated in a new release.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::crdt::{Extra, Lattice, Lww, Record, Seq, Stamp, extra_max_stamp, join_extra};
use crate::db::{extra_col, extra_text, opt_stamp_col, seq_col};
use crate::history::url_key;
use crate::prefs::keys;
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile, Url};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SearchEngineId(pub String);

impl SearchEngineId {
    pub fn builtin_default() -> Self {
        SearchEngineId("builtin:ddg".to_owned())
    }

    pub fn is_builtin(&self) -> bool {
        self.0.starts_with("builtin:")
    }
}

/// `{searchTerms}` is replaced by the percent-encoded query (OpenSearch convention): form
/// encoded in the query (a space as `+`), and with a space as `%20` in a path or fragment,
/// where `+` is a literal plus.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UrlTemplate(pub String);

impl UrlTemplate {
    pub fn expand(&self, terms: &str) -> Option<Url> {
        let mut encoded: String = url::form_urlencoded::byte_serialize(terms.as_bytes()).collect();
        let in_query = self.0.find("{searchTerms}").is_some_and(|at| {
            let before = &self.0[..at];
            before.contains('?') && !before.contains('#')
        });
        if !in_query {
            // A typed '+' is already %2B, so every '+' left stands for a space.
            encoded = encoded.replace('+', "%20");
        }
        Url::parse(&self.0.replace("{searchTerms}", &encoded)).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchEngine {
    pub id: SearchEngineId,
    pub name: String,
    /// Omnibox keyword: typing `w rust` searches engine `w`.
    pub keyword: Option<String>,
    pub search_url: UrlTemplate,
    pub suggest_url: Option<UrlTemplate>,
    pub builtin: bool,
}

pub(crate) struct Builtin {
    pub id: &'static str,
    pub name: &'static str,
    pub keyword: &'static str,
    pub search_url: &'static str,
    pub suggest_url: Option<&'static str>,
}

pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin { id: "builtin:ddg", name: "DuckDuckGo", keyword: "d", search_url: "https://duckduckgo.com/?q={searchTerms}", suggest_url: Some("https://duckduckgo.com/ac/?q={searchTerms}&type=list") },
    Builtin { id: "builtin:google", name: "Google", keyword: "g", search_url: "https://www.google.com/search?q={searchTerms}", suggest_url: Some("https://suggestqueries.google.com/complete/search?client=firefox&q={searchTerms}") },
    Builtin { id: "builtin:bing", name: "Bing", keyword: "b", search_url: "https://www.bing.com/search?q={searchTerms}", suggest_url: Some("https://www.bing.com/osjson.aspx?query={searchTerms}") },
    Builtin { id: "builtin:wikipedia", name: "Wikipedia", keyword: "w", search_url: "https://en.wikipedia.org/wiki/Special:Search?search={searchTerms}", suggest_url: None },
];

impl Builtin {
    fn find(id: &SearchEngineId) -> Option<&'static Builtin> {
        BUILTINS.iter().find(|b| b.id == id.0)
    }

    fn engine(&self) -> SearchEngine {
        SearchEngine {
            id: SearchEngineId(self.id.to_owned()),
            name: self.name.to_owned(),
            keyword: Some(self.keyword.to_owned()),
            search_url: UrlTemplate(self.search_url.to_owned()),
            suggest_url: self.suggest_url.map(|s| UrlTemplate(s.to_owned())),
            builtin: true,
        }
    }

    /// The record an edit of an untouched built-in starts from: every field at the zero stamp.
    fn fields(&self) -> EngineFields {
        let e = self.engine();
        EngineFields {
            name: Lww::new(e.name, Stamp::ZERO),
            keyword: Lww::new(e.keyword, Stamp::ZERO),
            search_url: Lww::new(e.search_url, Stamp::ZERO),
            suggest_url: Lww::new(e.suggest_url, Stamp::ZERO),
            extra: Extra::default(),
        }
    }
}

/// The live half of an engine record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineFields {
    pub name: Lww<String>,
    pub keyword: Lww<Option<String>>,
    pub search_url: Lww<UrlTemplate>,
    pub suggest_url: Lww<Option<UrlTemplate>>,
    pub extra: Extra,
}

impl Lattice for EngineFields {
    fn join(&mut self, other: Self) {
        self.name.join(other.name);
        self.keyword.join(other.keyword);
        self.search_url.join(other.search_url);
        self.suggest_url.join(other.suggest_url);
        join_extra(&mut self.extra, other.extra);
    }
}

impl EngineFields {
    fn max_stamp(&self) -> Stamp {
        [self.name.at, self.keyword.at, self.search_url.at, self.suggest_url.at]
            .into_iter()
            .chain(extra_max_stamp(&self.extra))
            .max()
            .expect("four fields")
    }

    /// Fields still at the zero stamp read through to the shipped values of a built-in.
    fn engine(&self, id: &SearchEngineId) -> SearchEngine {
        let own = SearchEngine {
            id: id.clone(),
            name: self.name.v.clone(),
            keyword: self.keyword.v.clone(),
            search_url: self.search_url.v.clone(),
            suggest_url: self.suggest_url.v.clone(),
            builtin: id.is_builtin(),
        };
        let Some(code) = Builtin::find(id).map(Builtin::engine) else { return own };
        let zero = |at: Stamp| at == Stamp::ZERO;
        SearchEngine {
            name: if zero(self.name.at) { code.name } else { own.name },
            keyword: if zero(self.keyword.at) { code.keyword } else { own.keyword },
            search_url: if zero(self.search_url.at) { code.search_url } else { own.search_url },
            suggest_url: if zero(self.suggest_url.at) { code.suggest_url } else { own.suggest_url },
            ..own
        }
    }
}

/// One engine on the wire and in the DB: live fields, or a tombstone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EngineWire", into = "EngineWire")]
pub struct EngineRecord {
    pub id: SearchEngineId,
    pub state: Record<EngineFields>,
}

impl Lattice for EngineRecord {
    fn join(&mut self, other: Self) {
        debug_assert_eq!(self.id, other.id, "join is per record");
        self.state.join(other.state);
    }
}

/// Flat wire shape: every mutable field `{"v","at"}`, unknown fields in `extra`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineWire {
    pub id: SearchEngineId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Lww<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<Lww<Option<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_url: Option<Lww<UrlTemplate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggest_url: Option<Lww<Option<UrlTemplate>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Stamp>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid search engine record: {0}")]
pub struct InvalidEngine(pub &'static str);

impl TryFrom<EngineWire> for EngineRecord {
    type Error = InvalidEngine;
    fn try_from(w: EngineWire) -> Result<Self, InvalidEngine> {
        if w.id.0.is_empty() {
            return Err(InvalidEngine("empty id"));
        }
        let state = match (w.deleted, w.name, w.keyword, w.search_url, w.suggest_url) {
            (Some(at), _, _, _, _) => Record::Tombstone(at),
            (None, Some(name), Some(keyword), Some(search_url), Some(suggest_url)) => {
                Record::Live(EngineFields { name, keyword, search_url, suggest_url, extra: w.extra })
            }
            _ => return Err(InvalidEngine("a live engine needs name, keyword, search_url and suggest_url")),
        };
        Ok(EngineRecord { id: w.id, state })
    }
}

impl From<EngineRecord> for EngineWire {
    fn from(r: EngineRecord) -> Self {
        match r.state {
            Record::Live(f) => EngineWire {
                id: r.id,
                name: Some(f.name),
                keyword: Some(f.keyword),
                search_url: Some(f.search_url),
                suggest_url: Some(f.suggest_url),
                deleted: None,
                extra: f.extra,
            },
            Record::Tombstone(at) => {
                EngineWire { id: r.id, name: None, keyword: None, search_url: None, suggest_url: None, deleted: Some(at), extra: Extra::default() }
            }
        }
    }
}

/// `wanted` among `engines`, else the first live built-in, else any engine.
fn pick_default(engines: &[SearchEngine], wanted: &SearchEngineId) -> Option<SearchEngine> {
    engines
        .iter()
        .find(|e| &e.id == wanted)
        .or_else(|| BUILTINS.iter().find_map(|b| engines.iter().find(|e| e.id.0 == b.id)))
        .or_else(|| engines.first())
        .cloned()
}

pub struct SearchEngines<'p> {
    pub(crate) p: &'p mut Profile,
}

impl SearchEngines<'_> {
    /// Built-ins overlaid with stored records, minus tombstones, ordered by name.
    pub fn list(&mut self) -> Result<Vec<SearchEngine>, Error> {
        let stored = load_all(&self.p.conn)?;
        let mut out: Vec<SearchEngine> = BUILTINS
            .iter()
            .filter_map(|b| match stored.get(&SearchEngineId(b.id.to_owned())) {
                None => Some(b.engine()),
                Some(rec) => rec.state.live().map(|f| f.engine(&rec.id)),
            })
            .collect();
        for (id, rec) in &stored {
            if Builtin::find(id).is_none()
                && let Some(f) = rec.state.live()
            {
                out.push(f.engine(id));
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Resolves `prefs::keys::DEFAULT_SEARCH_ENGINE`. If that engine was deleted (perhaps
    /// on another device), falls back to the first live built-in, then to any live engine.
    /// Derived at read time. Nothing is rewritten.
    pub fn default_engine(&mut self) -> Result<SearchEngine, Error> {
        let wanted = self.p.prefs().get(&keys::DEFAULT_SEARCH_ENGINE);
        pick_default(&self.list()?, &wanted).ok_or(Error::NotFound)
    }

    pub fn set_default(&mut self, id: &SearchEngineId) -> Result<(), Error> {
        if !self.list()?.iter().any(|e| &e.id == id) {
            return Err(Error::NotFound);
        }
        self.p.prefs().set(&keys::DEFAULT_SEARCH_ENGINE, id)
    }

    pub fn add(&mut self, name: &str, keyword: Option<&str>, search_url: UrlTemplate) -> Result<SearchEngineId, Error> {
        let id = SearchEngineId(uuid::Uuid::new_v4().to_string());
        let rec_id = id.clone();
        let (name, keyword) = (name.to_owned(), keyword.map(str::to_owned));
        self.p.write(move |tx| {
            let at = tx.stamp();
            let rec = EngineRecord {
                id: rec_id,
                state: Record::Live(EngineFields {
                    name: Lww::new(name, at),
                    keyword: Lww::new(keyword, at),
                    search_url: Lww::new(search_url, at),
                    suggest_url: Lww::new(None, at),
                    extra: Extra::default(),
                }),
            };
            let seq = tx.seq();
            store_record(&tx.sql, &rec, seq)
        })?;
        Ok(id)
    }

    pub fn update(&mut self, id: &SearchEngineId, edit: EngineEdit) -> Result<(), Error> {
        let id = id.clone();
        self.p.write(move |tx| {
            let mut rec = match (load_record(&tx.sql, &id)?, Builtin::find(&id)) {
                (Some(r), _) => r,
                (None, Some(b)) => EngineRecord { id: id.clone(), state: Record::Live(b.fields()) },
                (None, None) => return Err(Error::NotFound),
            };
            let Record::Live(f) = &mut rec.state else { return Err(Error::NotFound) };
            // Zero-stamped fields show the shipped values, whatever release stored them, so an
            // edit compares against those.
            if let Some(b) = Builtin::find(&id) {
                let code = b.fields();
                if f.name.at == Stamp::ZERO {
                    f.name = code.name;
                }
                if f.keyword.at == Stamp::ZERO {
                    f.keyword = code.keyword;
                }
                if f.search_url.at == Stamp::ZERO {
                    f.search_url = code.search_url;
                }
                if f.suggest_url.at == Stamp::ZERO {
                    f.suggest_url = code.suggest_url;
                }
            }
            let at = tx.stamp();
            let mut changed = false;
            if let Some(v) = edit.name {
                changed |= f.name.set(v, at);
            }
            if let Some(v) = edit.keyword {
                changed |= f.keyword.set(v, at);
            }
            if let Some(v) = edit.search_url {
                changed |= f.search_url.set(v, at);
            }
            if let Some(v) = edit.suggest_url {
                changed |= f.suggest_url.set(v, at);
            }
            if changed {
                let seq = tx.seq();
                store_record(&tx.sql, &rec, seq)?;
            }
            Ok(())
        })
    }

    pub fn remove(&mut self, id: &SearchEngineId) -> Result<(), Error> {
        let id = id.clone();
        self.p.write(move |tx| {
            match (load_record(&tx.sql, &id)?, Builtin::find(&id)) {
                (Some(EngineRecord { state: Record::Tombstone(_), .. }), _) => return Ok(()),
                (Some(_), _) | (None, Some(_)) => {}
                (None, None) => return Err(Error::NotFound),
            }
            let at = tx.stamp();
            let seq = tx.seq();
            store_record(&tx.sql, &EngineRecord { id, state: Record::Tombstone(at) }, seq)
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct EngineEdit {
    pub name: Option<String>,
    pub keyword: Option<Option<String>>,
    pub search_url: Option<UrlTemplate>,
    pub suggest_url: Option<Option<UrlTemplate>>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

const COLUMNS: &str =
    "id, name, name_at, keyword, keyword_at, search_url, search_url_at, suggest_url, suggest_url_at, deleted_at, extra, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, EngineRecord), rusqlite::Error> {
    let id = SearchEngineId(row.get(0)?);
    let seq = seq_col(row, 11)?;
    if let Some(at) = opt_stamp_col(row, 9)? {
        return Ok((seq, EngineRecord { id, state: Record::Tombstone(at) }));
    }
    let need = |o: Option<Stamp>, idx| o.ok_or_else(|| crate::db::bad_column(idx, "stamp"));
    let fields = EngineFields {
        name: Lww::new(row.get(1)?, need(opt_stamp_col(row, 2)?, 2)?),
        keyword: Lww::new(row.get(3)?, need(opt_stamp_col(row, 4)?, 4)?),
        search_url: Lww::new(UrlTemplate(row.get(5)?), need(opt_stamp_col(row, 6)?, 6)?),
        suggest_url: Lww::new(row.get::<_, Option<String>>(7)?.map(UrlTemplate), need(opt_stamp_col(row, 8)?, 8)?),
        extra: extra_col(row, 10)?,
    };
    Ok((seq, EngineRecord { id, state: Record::Live(fields) }))
}

fn load_all(conn: &rusqlite::Connection) -> Result<BTreeMap<SearchEngineId, EngineRecord>, Error> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM search_engines"))?;
    let rows = stmt.query_map([], row_record)?;
    let mut out = BTreeMap::new();
    for r in rows {
        let (_, rec) = r?;
        out.insert(rec.id.clone(), rec);
    }
    Ok(out)
}

fn load_record(conn: &rusqlite::Connection, id: &SearchEngineId) -> Result<Option<EngineRecord>, Error> {
    let rec = conn.query_row(&format!("SELECT {COLUMNS} FROM search_engines WHERE id = ?1"), [&id.0], row_record).optional()?;
    Ok(rec.map(|(_, r)| r))
}

fn store_record(conn: &rusqlite::Connection, rec: &EngineRecord, seq: Seq) -> Result<(), Error> {
    let sql = format!("INSERT OR REPLACE INTO search_engines ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)");
    let live = rec.state.live();
    let deleted_at = match &rec.state {
        Record::Tombstone(at) => Some(at.to_vec()),
        Record::Live(_) => None,
    };
    conn.execute(
        &sql,
        params![
            rec.id.0,
            live.map(|f| f.name.v.as_str()),
            live.map(|f| f.name.at.to_vec()),
            live.and_then(|f| f.keyword.v.as_deref()),
            live.map(|f| f.keyword.at.to_vec()),
            live.map(|f| f.search_url.v.0.as_str()),
            live.map(|f| f.search_url.at.to_vec()),
            live.and_then(|f| f.suggest_url.v.as_ref().map(|u| u.0.as_str())),
            live.map(|f| f.suggest_url.at.to_vec()),
            deleted_at,
            live.map_or_else(|| "{}".to_owned(), |f| extra_text(&f.extra)),
            seq.0 as i64,
        ],
    )?;
    Ok(())
}

pub(crate) struct EnginesTable;

impl SyncTable for EnginesTable {
    const KIND: Kind = Kind::SearchEngines;
    type Record = EngineRecord;

    fn wire_id(rec: &EngineRecord) -> String {
        rec.id.0.clone()
    }

    fn max_stamp(rec: &EngineRecord) -> Option<Stamp> {
        Some(match &rec.state {
            Record::Live(f) => f.max_stamp(),
            Record::Tombstone(at) => *at,
        })
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<EngineRecord>, Error> {
        load_record(tx, &SearchEngineId(wire_id.to_owned()))
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &EngineRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, EngineRecord)>, bool), Error> {
        changed_rows(conn, "search_engines", COLUMNS, "1", since, limit, row_record)
    }
}

// ---------------------------------------------------------------------------
// Omnibox
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavTarget {
    Url(Url),
    Search { engine: SearchEngineId, url: Url },
}

impl NavTarget {
    /// What the shell passes to `load_uri` / `Navigate`.
    pub fn url(&self) -> &Url {
        match self {
            NavTarget::Url(u) | NavTarget::Search { url: u, .. } => u,
        }
    }
}

pub(crate) const NAVIGABLE_SCHEMES: &[&str] = &["http", "https", "file", "about", "data", "view-source", "vsesvit"];

/// Steps 1-3 of [`classify`]: everything that makes the text a URL on its own.
pub fn classify_url(text: &str) -> Option<NavTarget> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(text)
        && NAVIGABLE_SCHEMES.contains(&url.scheme())
        && (!matches!(url.scheme(), "http" | "https") || url.host_str().is_some())
    {
        return Some(NavTarget::Url(url));
    }
    if let Some(url) = file_path_url(text) {
        return Some(NavTarget::Url(url));
    }
    if !text.chars().any(char::is_whitespace)
        && let Some(url) = host_url(text)
    {
        return Some(NavTarget::Url(url));
    }
    None
}

/// Pure omnibox classification, table-tested:
///
/// 1. trim; empty -> `None`
/// 2. parses as a URL with a navigable scheme (`http`, `https`, `file`, `about`, `data`,
///    `view-source`, `vsesvit`) -> `Url`; an absolute file path (`/usr/…`, `C:\…`,
///    `\\server\share`) -> `file:` URL
/// 3. no whitespace and looks like a host: `localhost`, an IPv4/IPv6 literal, or dotted
///    labels ending in an alphabetic TLD, internationalized names (`пример.укр`) included,
///    with optional `:port` and `/path` -> `https://` + text (`http://` for localhost and IP
///    literals)
/// 4. first word equals an engine keyword and there is more text -> search that engine
/// 5. otherwise -> search the default engine
pub fn classify(text: &str, engines: &[SearchEngine], default: &SearchEngine) -> Option<NavTarget> {
    if let Some(t) = classify_url(text) {
        return Some(t);
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((first, rest)) = text.split_once(char::is_whitespace) {
        let rest = rest.trim();
        if !rest.is_empty()
            && let Some(engine) = engines.iter().find(|e| e.keyword.as_deref() == Some(first))
        {
            return search_target(engine, rest);
        }
    }
    search_target(default, text)
}

fn search_target(engine: &SearchEngine, terms: &str) -> Option<NavTarget> {
    engine.search_url.expand(terms).map(|url| NavTarget::Search { engine: engine.id.clone(), url })
}

/// `C:\dir\f`, `\\server\share\f`, `/usr/share/f` -> `file:` URL. Spaces are allowed,
/// since real paths have them and the leading shape already rules out prose. A `%`, `#` or
/// `?` is part of a file's name, so it is escaped rather than read as URL syntax.
fn file_path_url(text: &str) -> Option<Url> {
    let path = |s: &str| s.replace('\\', "/").replace('%', "%25").replace('#', "%23").replace('?', "%3F");
    let bytes = text.as_bytes();
    let candidate = if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') {
        format!("file:///{}", path(text))
    } else if let Some(unc) = text.strip_prefix("\\\\").filter(|r| !r.starts_with('\\') && !r.is_empty()) {
        format!("file://{}", path(unc))
    } else if text.starts_with('/') && !text.starts_with("//") {
        format!("file://{}", path(text))
    } else {
        return None;
    };
    Url::parse(&candidate).ok()
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

/// Host-shaped text (`localhost`, IP literal, `name.tld`), with optional port and path.
fn host_url(text: &str) -> Option<Url> {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let (authority, rest) = text.split_at(end);
    if authority.is_empty() {
        return None;
    }
    let (host, port): (String, Option<&str>) = if let Some(inner) = authority.strip_prefix('[') {
        let (ip, after) = inner.split_once(']')?;
        ip.parse::<Ipv6Addr>().ok()?;
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':')?),
        };
        (format!("[{ip}]"), port)
    } else if authority.parse::<Ipv6Addr>().is_ok() {
        (format!("[{authority}]"), None)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (authority.to_owned(), None),
        }
    };
    if let Some(p) = port
        && (p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) || p.parse::<u32>().ok()? > 65_535)
    {
        return None;
    }
    let plain = host.trim_start_matches('[').trim_end_matches(']');
    let scheme = if host.eq_ignore_ascii_case("localhost") || plain.parse::<Ipv4Addr>().is_ok() || host.starts_with('[') {
        "http"
    } else {
        // Labels are checked in their punycode form, so `пример.укр` counts as a host too.
        let ascii = if host.is_ascii() { host.clone() } else { idna::domain_to_ascii(&host).ok()? };
        let labels: Vec<&str> = ascii.split('.').collect();
        // The TLD as typed: two or more letters of any script, or punycode already.
        let tld = host.rsplit('.').next()?;
        let dotted = labels.len() >= 2
            && labels.iter().all(|l| valid_label(l))
            && ((tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)) || tld.starts_with("xn--"));
        if !dotted {
            return None;
        }
        "https"
    };
    let port = port.map(|p| format!(":{p}")).unwrap_or_default();
    Url::parse(&format!("{scheme}://{host}{port}{rest}")).ok()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestionSource {
    /// "Search <engine> for <text>". Always first when the input is not a URL.
    Search,
    Typed,
    Bookmark,
    History,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub source: SuggestionSource,
    pub title: String,
    pub target: NavTarget,
    /// What the address box shows while this row is highlighted: the typed text for a search,
    /// the URL for the rest, without `https://`/`http://` and `www.` unless the typed text has them.
    pub fill: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suggestions {
    /// `items[0]` is the default match: what Enter opens while no other row is highlighted.
    pub items: Vec<Suggestion>,
    /// Text to show after what the user typed, selected, so the box reads `items[0].fill`.
    /// `Some` only when it is non-empty and `allow_inline` was true.
    pub inline: Option<String>,
}

/// The item Chrome adds to a page's context menu for selected text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionAction {
    /// `Search DuckDuckGo for “text”`, or `Go to example.com` when the selection is an address.
    pub label: String,
    pub url: Url,
}

/// The context menu item for selected `text`: `Go to` when it is one `http(s)` address,
/// otherwise a search of `default` for the whitespace-collapsed text. None for blank text.
pub fn selection_action(text: &str, default: &SearchEngine) -> Option<SelectionAction> {
    selection(text, Some(default))
}

fn selection(text: &str, default: Option<&SearchEngine>) -> Option<SelectionAction> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 50;
    let shown = match text.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => format!("{}\u{2026}", &text[..cut]),
        None => text.clone(),
    };
    if !text.contains(' ')
        && let Some(NavTarget::Url(url)) = classify_url(&text)
        && matches!(url.scheme(), "http" | "https")
    {
        return Some(SelectionAction { label: format!("Go to {shown}"), url });
    }
    let default = default?;
    let url = default.search_url.expand(&text)?;
    Some(SelectionAction { label: format!("Search {} for \u{201c}{shown}\u{201d}", default.name), url })
}

/// Ctrl+Enter in the address box: `www.<text>.com`, as in Chrome, for a single bare word.
pub fn ctrl_enter_url(text: &str) -> Option<Url> {
    let word = text.trim();
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    Url::parse(&format!("https://www.{word}.com/")).ok()
}

/// The address box text for `url`: no `http(s)://` or leading `www.` unless `typed` has them,
/// and no trailing `/` on a bare origin.
fn fill(url: &Url, typed: &str) -> String {
    let full = url.as_str();
    let Some(scheme) = ["https://", "http://"].into_iter().find(|s| full.starts_with(s)) else { return full.to_owned() };
    let typed = typed.trim().to_lowercase();
    let rest = &full[scheme.len()..];
    let typed_rest = typed.split_once("://").map_or(typed.as_str(), |(_, r)| r);
    let rest = if typed_rest.starts_with("www.") { rest } else { rest.strip_prefix("www.").unwrap_or(rest) };
    let rest = if url.path() == "/" && url.query().is_none() && url.fragment().is_none() { rest.strip_suffix('/').unwrap_or(rest) } else { rest };
    let scheme = if typed.starts_with(scheme) { scheme } else { "" };
    format!("{scheme}{rest}")
}

/// A bookmark or history row in rank order; `prefix` is whether its url starts with the typed text.
struct Candidate {
    prefix: bool,
    row: Suggestion,
}

/// Picks the default match over the ranked rows: the inline completion when there is one,
/// then the what-you-typed row, then the rest. A ranked row for the typed url takes the
/// what-you-typed row's place, so the url shows once and with its title.
fn arrange(text: &str, typed: Option<Suggestion>, ranked: Vec<Candidate>, allow_inline: bool, limit: usize) -> Suggestions {
    let (lead, inline) = match allow_inline.then(|| completion(text, &ranked)).flatten() {
        Some((row, inline)) => (Some(row), Some(inline)),
        None => (None, None),
    };
    let lead_target = lead.as_ref().map(|l| l.target.clone());
    let mut rest: Vec<Suggestion> = ranked.into_iter().map(|c| c.row).filter(|r| lead_target.as_ref() != Some(&r.target)).collect();
    let typed = typed.map(|t| match rest.iter().position(|r| r.target == t.target) {
        Some(i) => rest.remove(i),
        None => t,
    });
    let items = lead.into_iter().chain(typed).chain(rest).take(limit).collect();
    Suggestions { items, inline }
}

/// The best prefix match, widened to its origin root while the typed text is still a host
/// prefix, and the suffix that completes `text` to its fill. A root that is no bookmark or
/// history row of its own reads as a typed address.
fn completion(text: &str, ranked: &[Candidate]) -> Option<(Suggestion, String)> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    let best = &ranked.iter().find(|c| c.prefix)?.row;
    let after_scheme = text.split_once("://").map_or(text, |(_, r)| r);
    let origin = best.target.url().origin();
    let root = if origin.is_tuple() { Url::parse(&format!("{}/", origin.ascii_serialization())).ok() } else { None };
    let row = match root {
        Some(root) if !after_scheme.contains('/') => match ranked.iter().find(|c| c.row.target.url() == &root) {
            Some(c) => c.row.clone(),
            None => Suggestion {
                source: SuggestionSource::Typed,
                title: root.host_str().unwrap_or_default().to_owned(),
                fill: fill(&root, text),
                target: NavTarget::Url(root),
            },
        },
        _ => best.clone(),
    };
    let inline = inline_suffix(text, &row.fill)?;
    Some((row, inline))
}

/// The rest of `fill` after `typed`, compared case-insensitively char by char. None when
/// `typed` is not a prefix of `fill` or nothing is left.
fn inline_suffix(typed: &str, fill: &str) -> Option<String> {
    let mut rest = fill.char_indices();
    for t in typed.chars() {
        let (_, f) = rest.next()?;
        if !t.to_lowercase().eq(f.to_lowercase()) {
            return None;
        }
    }
    let suffix = rest.as_str();
    (!suffix.is_empty()).then(|| suffix.to_owned())
}

pub struct Omnibox<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Omnibox<'_> {
    /// Access pattern 2: Enter pressed.
    pub fn resolve(&mut self, text: &str) -> Result<Option<NavTarget>, Error> {
        let engines = self.p.search_engines().list()?;
        Ok(self.classify_with(text, &engines))
    }

    /// [`classify`] against `engines`, already listed, and the default among them.
    fn classify_with(&mut self, text: &str, engines: &[SearchEngine]) -> Option<NavTarget> {
        let wanted = self.p.prefs().get(&keys::DEFAULT_SEARCH_ENGINE);
        match pick_default(engines, &wanted) {
            Some(default) => classify(text, engines, &default),
            None => classify_url(text),
        }
    }

    /// Access pattern 2: every keystroke. Bookmarks come from memory, history from one
    /// indexed query (`History::search`). Deduplicated by url and ranked: url prefix
    /// match, then bookmark, then frecency. Bookmarks and history are left out when the
    /// user turned them off ([`keys::SUGGEST_BOOKMARKS`], [`keys::SUGGEST_HISTORY`]). Remote
    /// search-engine suggestions are a network call and belong to the shell (it has the
    /// `suggest_url`).
    ///
    /// With `allow_inline`, a host prefix of a visited or bookmarked url completes inline to
    /// that site's root (`git` -> `github.com`), and text with a `/` to the url itself; the
    /// completion becomes `items[0]`, ahead of the what-you-typed row.
    pub fn suggest(&mut self, text: &str, limit: usize, allow_inline: bool) -> Result<Suggestions, Error> {
        let typed = text.trim();
        if typed.is_empty() || limit == 0 {
            return Ok(Suggestions::default());
        }
        let with_bookmarks = self.p.prefs().get(&keys::SUGGEST_BOOKMARKS);
        let with_history = self.p.prefs().get(&keys::SUGGEST_HISTORY);
        let engines = self.p.search_engines().list()?;
        let typed_row = match self.classify_with(typed, &engines) {
            Some(target @ NavTarget::Url(_)) => {
                Some(Suggestion { source: SuggestionSource::Typed, title: target.url().to_string(), fill: fill(target.url(), typed), target })
            }
            Some(target @ NavTarget::Search { .. }) => {
                let NavTarget::Search { engine, .. } = &target else { unreachable!() };
                let name = engines.iter().find(|e| &e.id == engine).map(|e| e.name.as_str()).unwrap_or("the web");
                Some(Suggestion { source: SuggestionSource::Search, title: format!("Search {name} for \"{typed}\""), fill: typed.to_owned(), target })
            }
            None => None,
        };
        let typed_key = url_key(&typed.to_lowercase());
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let bookmarks = if with_bookmarks { self.p.bookmarks().search(typed, limit) } else { Vec::new() };
        for node in bookmarks {
            let Some(url) = node.url else { continue };
            if seen.insert(url.clone()) {
                let prefix = url_key(url.as_str()).starts_with(&typed_key);
                let row = Suggestion { source: SuggestionSource::Bookmark, title: node.title, fill: fill(&url, typed), target: NavTarget::Url(url) };
                candidates.push(Candidate { prefix, row });
            }
        }
        let history = if with_history { self.p.history().search(typed, limit)? } else { Vec::new() };
        for entry in history {
            if seen.insert(entry.url.clone()) {
                let prefix = url_key(entry.url.as_str()).starts_with(&typed_key);
                let title = if entry.title.is_empty() { entry.url.to_string() } else { entry.title };
                let row = Suggestion { source: SuggestionSource::History, title, fill: fill(&entry.url, typed), target: NavTarget::Url(entry.url) };
                candidates.push(Candidate { prefix, row });
            }
        }
        // stable, so bookmarks stay ahead of history within each group, each in its own order
        candidates.sort_by_key(|c| !c.prefix);
        Ok(arrange(text, typed_row, candidates, allow_inline, limit))
    }

    /// The page context menu's item for selected text, or None for blank text.
    pub fn for_selection(&mut self, text: &str) -> Result<Option<SelectionAction>, Error> {
        match self.p.search_engines().default_engine() {
            Ok(default) => Ok(selection(text, Some(&default))),
            Err(Error::NotFound) => Ok(selection(text, None)),
            Err(e) => Err(e),
        }
    }
}
