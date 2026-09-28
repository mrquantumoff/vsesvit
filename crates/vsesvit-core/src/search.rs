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

/// `{searchTerms}` is replaced by the percent-encoded query (OpenSearch convention).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UrlTemplate(pub String);

impl UrlTemplate {
    pub fn expand(&self, terms: &str) -> Option<Url> {
        let encoded: String = url::form_urlencoded::byte_serialize(terms.as_bytes()).collect();
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
        let code = Builtin::find(id).map(Builtin::engine);
        let through = |at: Stamp| at == Stamp::ZERO && code.is_some();
        let code_ref = code.as_ref();
        SearchEngine {
            id: id.clone(),
            name: if through(self.name.at) { code_ref.map(|c| c.name.clone()).unwrap_or_default() } else { self.name.v.clone() },
            keyword: if through(self.keyword.at) { code_ref.and_then(|c| c.keyword.clone()) } else { self.keyword.v.clone() },
            search_url: if through(self.search_url.at) {
                code_ref.map(|c| c.search_url.clone()).unwrap_or_else(|| self.search_url.v.clone())
            } else {
                self.search_url.v.clone()
            },
            suggest_url: if through(self.suggest_url.at) { code_ref.and_then(|c| c.suggest_url.clone()) } else { self.suggest_url.v.clone() },
            builtin: id.is_builtin(),
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
        let engines = self.list()?;
        if let Some(e) = engines.iter().find(|e| e.id == wanted) {
            return Ok(e.clone());
        }
        for b in BUILTINS {
            if let Some(e) = engines.iter().find(|e| e.id.0 == b.id) {
                return Ok(e.clone());
            }
        }
        engines.into_iter().next().ok_or(Error::NotFound)
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
    match &rec.state {
        Record::Live(f) => conn.execute(
            &sql,
            params![
                rec.id.0,
                f.name.v,
                f.name.at.to_vec(),
                f.keyword.v,
                f.keyword.at.to_vec(),
                f.search_url.v.0,
                f.search_url.at.to_vec(),
                f.suggest_url.v.as_ref().map(|u| u.0.as_str()),
                f.suggest_url.at.to_vec(),
                Option::<Vec<u8>>::None,
                extra_text(&f.extra),
                seq.0 as i64,
            ],
        )?,
        Record::Tombstone(at) => conn.execute(
            &sql,
            params![
                rec.id.0,
                Option::<String>::None,
                Option::<Vec<u8>>::None,
                Option::<String>::None,
                Option::<Vec<u8>>::None,
                Option::<String>::None,
                Option::<Vec<u8>>::None,
                Option::<String>::None,
                Option::<Vec<u8>>::None,
                at.to_vec(),
                "{}",
                seq.0 as i64,
            ],
        )?,
    };
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
///    labels ending in an alphabetic TLD, with optional `:port` and `/path` ->
///    `https://` + text (`http://` for localhost and IP literals)
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
/// since real paths have them and the leading shape already rules out prose.
fn file_path_url(text: &str) -> Option<Url> {
    let bytes = text.as_bytes();
    let candidate = if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') {
        format!("file:///{}", text.replace('\\', "/"))
    } else if let Some(unc) = text.strip_prefix("\\\\").filter(|r| !r.starts_with('\\') && !r.is_empty()) {
        format!("file://{}", unc.replace('\\', "/"))
    } else if text.starts_with('/') && !text.starts_with("//") {
        format!("file://{text}")
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
        let labels: Vec<&str> = host.split('.').collect();
        let tld = labels.last()?;
        let dotted = labels.len() >= 2
            && labels.iter().all(|l| valid_label(l))
            && tld.len() >= 2
            && tld.bytes().all(|b| b.is_ascii_alphabetic());
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
}

pub struct Omnibox<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Omnibox<'_> {
    /// Access pattern 2: Enter pressed.
    pub fn resolve(&mut self, text: &str) -> Result<Option<NavTarget>, Error> {
        let engines = self.p.search_engines().list()?;
        match self.p.search_engines().default_engine() {
            Ok(default) => Ok(classify(text, &engines, &default)),
            Err(Error::NotFound) => Ok(classify_url(text)),
            Err(e) => Err(e),
        }
    }

    /// Access pattern 2: every keystroke. Bookmarks come from memory, history from one
    /// indexed query (`History::search`). Deduplicated by url and ranked: url prefix
    /// match, then bookmark, then frecency. Remote search-engine suggestions are a
    /// network call and belong to the shell (it has the `suggest_url`).
    pub fn suggest(&mut self, text: &str, limit: usize) -> Result<Vec<Suggestion>, Error> {
        let text = text.trim();
        if text.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let engines = self.p.search_engines().list()?;
        let mut out = Vec::new();
        match self.resolve(text)? {
            Some(target @ NavTarget::Url(_)) => {
                out.push(Suggestion { source: SuggestionSource::Typed, title: target.url().to_string(), target });
            }
            Some(target @ NavTarget::Search { .. }) => {
                let NavTarget::Search { engine, .. } = &target else { unreachable!() };
                let name = engines.iter().find(|e| &e.id == engine).map(|e| e.name.as_str()).unwrap_or("the web");
                out.push(Suggestion { source: SuggestionSource::Search, title: format!("Search {name} for \"{text}\""), target });
            }
            None => {}
        }
        let typed_key = url_key(&text.to_lowercase());
        let mut candidates: Vec<(u8, u8, usize, Suggestion)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for node in self.p.bookmarks().search(text, limit) {
            let Some(url) = node.url else { continue };
            if seen.insert(url.clone()) {
                let prefix = u8::from(!url_key(url.as_str()).starts_with(&typed_key));
                let n = candidates.len();
                candidates.push((prefix, 0, n, Suggestion { source: SuggestionSource::Bookmark, title: node.title, target: NavTarget::Url(url) }));
            }
        }
        for entry in self.p.history().search(text, limit)? {
            if seen.insert(entry.url.clone()) {
                let prefix = u8::from(!url_key(entry.url.as_str()).starts_with(&typed_key));
                let n = candidates.len();
                let title = if entry.title.is_empty() { entry.url.to_string() } else { entry.title };
                candidates.push((prefix, 1, n, Suggestion { source: SuggestionSource::History, title, target: NavTarget::Url(entry.url) }));
            }
        }
        candidates.sort_by_key(|(prefix, source, n, _)| (*prefix, *source, *n));
        out.extend(candidates.into_iter().map(|(_, _, _, s)| s).take(limit.saturating_sub(out.len())));
        Ok(out)
    }
}
