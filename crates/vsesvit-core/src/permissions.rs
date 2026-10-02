//! Site permissions: what a site may use (camera, location, notifications, ...), per origin.
//!
//! Three layers, each owned where it lives:
//!
//! - **Stored settings** ([`SitePermissions`]): Allow or Block per `(origin, permission)`,
//!   synced like Chrome's site settings. No row, or a reset row, means Ask, or Block for
//!   picture-in-picture, which no site asks for.
//! - **One-time grants** ([`TabGrants`]): "Allow this time", held in memory by one shell tab
//!   until it leaves the site.
//! - **The prompt** ([`prompt`]): the words and buttons both shells show, and
//!   [`SitePermissions::answer`], which applies the button pressed; and the rows of the
//!   site-info popup's Permissions section ([`site_rows`]).
//!
//! A request goes `decide` -> (on [`Decision::Ask`]) `prompt` -> `answer`.
//!
//! Opaque origins (`file:`, `data:`, `about:`) have no [`Origin`], so nothing is stored for
//! them; only one-time grants apply.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::crdt::{Lattice, Lww, Seq, Stamp};
use crate::db::{seq_col, stamp_col};
use crate::sync::{ChangedRows, Kind, SyncTable, changed_rows};
use crate::{Error, Profile, Url};

pub(crate) const SCHEMA: &str = "
CREATE TABLE site_permissions (          -- Kind::SitePermissions
  origin      TEXT NOT NULL,             -- Origin::as_str
  permission  TEXT NOT NULL,             -- Permission::key
  setting     TEXT CHECK (setting IN ('allow', 'block')),   -- NULL = ask; a reset keeps the row so it syncs
  setting_at  BLOB NOT NULL,
  seq         INTEGER NOT NULL,
  PRIMARY KEY (origin, permission)
) WITHOUT ROWID;
CREATE INDEX site_permissions_seq ON site_permissions(seq);
";

/// What a site can ask for. Serde names are the stable wire/SQL names.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Camera,
    Microphone,
    Location,
    Notifications,
    ScreenShare,
    ClipboardRead,
    Midi,
    /// Showing the site's playing video in the media player while another tab is selected.
    /// No site asks for it: it is off until the user turns it on (see [`Permission::asks`]).
    PictureInPicture,
}

impl Permission {
    pub const ALL: &'static [Permission] = &[
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
        Permission::Notifications,
        Permission::ScreenShare,
        Permission::ClipboardRead,
        Permission::Midi,
        Permission::PictureInPicture,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Permission::Camera => "camera",
            Permission::Microphone => "microphone",
            Permission::Location => "location",
            Permission::Notifications => "notifications",
            Permission::ScreenShare => "screen_share",
            Permission::ClipboardRead => "clipboard_read",
            Permission::Midi => "midi",
            Permission::PictureInPicture => "picture_in_picture",
        }
    }

    pub fn from_key(key: &str) -> Option<Permission> {
        Permission::ALL.iter().copied().find(|p| p.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Permission::Camera => "Camera",
            Permission::Microphone => "Microphone",
            Permission::Location => "Location",
            Permission::Notifications => "Notifications",
            Permission::ScreenShare => "Screen sharing",
            Permission::ClipboardRead => "Clipboard",
            Permission::Midi => "MIDI devices",
            Permission::PictureInPicture => "Picture-in-picture",
        }
    }

    /// Whether a site may ask for it. The user turns picture-in-picture on for a site, so with
    /// nothing stored it is blocked, not asked for, and its rows offer no Ask.
    pub fn asks(self) -> bool {
        self != Permission::PictureInPicture
    }

    /// Screen sharing is chosen share by share, so only a block is remembered (as Chrome does),
    /// and "Allow this time" covers that one request, not the rest of the tab's visit.
    pub fn remembers_allow(self) -> bool {
        self != Permission::ScreenShare
    }

    /// The settings a site can have stored for this permission: Allow only where it is
    /// remembered.
    pub fn settings(self) -> &'static [Setting] {
        if self.remembers_allow() { &[Setting::Allow, Setting::Block] } else { &[Setting::Block] }
    }

    /// What the site wants to do, as a verb and its object. Requests that share a verb read
    /// as one phrase: "use your camera and microphone".
    fn request(self) -> (&'static str, &'static str) {
        match self {
            Permission::Camera => ("use your", "camera"),
            Permission::Microphone => ("use your", "microphone"),
            Permission::Location => ("know your", "location"),
            Permission::Notifications => ("show", "notifications"),
            Permission::ScreenShare => ("share your", "screen"),
            Permission::ClipboardRead => ("see", "text and images copied to the clipboard"),
            Permission::Midi => ("use your", "MIDI devices"),
            Permission::PictureInPicture => ("show", "videos in picture-in-picture"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    Allow,
    Block,
}

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Setting::Allow => "Allow",
            Setting::Block => "Block",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Setting::Allow => "allow",
            Setting::Block => "block",
        }
    }

    fn from_key(key: &str) -> Option<Setting> {
        [Setting::Allow, Setting::Block].into_iter().find(|s| s.key() == key)
    }
}

/// `scheme://host[:port]` (`url::Origin` ascii serialization). Opaque origins (file:, data:,
/// about:) have none, so nothing is remembered for them. On the wire it is that string, and
/// only the canonical form deserializes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Origin(String);

impl Origin {
    pub fn of(url: &Url) -> Option<Origin> {
        let origin = url.origin();
        origin.is_tuple().then(|| Origin(origin.ascii_serialization()))
    }

    /// Accepts a URL or an origin string and normalizes it: `HTTPS://Example.com:443/a`
    /// is `https://example.com`.
    pub fn parse(text: &str) -> Option<Origin> {
        Origin::of(&Url::parse(text).ok()?)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// For prompts and lists: the host decoded for display, plus `:port` only when it is not
    /// the scheme's default. `meet.example.com`, `localhost:8080`.
    pub fn host_for_display(&self) -> String {
        let url = Url::parse(&self.0).expect("an origin is a URL");
        let host = crate::address::readable_host(url.host_str().unwrap_or_default());
        match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        }
    }
}

impl From<Origin> for String {
    fn from(o: Origin) -> String {
        o.0
    }
}

impl TryFrom<String> for Origin {
    type Error = BadOrigin;
    fn try_from(s: String) -> Result<Origin, BadOrigin> {
        Origin::parse(&s).filter(|o| o.0 == s).ok_or(BadOrigin)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("not a canonical origin")]
pub struct BadOrigin;

// ---------------------------------------------------------------------------
// The prompt
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    AllowWhileVisiting,
    AllowThisTime,
    NeverAllow,
    Dismiss,
}

impl Answer {
    pub fn label(self) -> &'static str {
        match self {
            Answer::AllowWhileVisiting => "Allow while visiting the site",
            Answer::AllowThisTime => "Allow this time",
            Answer::NeverAllow => "Never allow",
            Answer::Dismiss => "Not now",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub heading: String,
    pub body: String,
    /// In button order.
    pub answers: Vec<Answer>,
}

/// What both shells show for one engine request. `permissions` is the set that request asks
/// for together (camera + microphone from one `getUserMedia`).
///
/// Offers "Allow while visiting the site" only when the choice can be remembered (a real
/// origin, and no permission that is asked for every time), and "Never allow" only for a
/// real origin.
pub fn prompt(origin: Option<&Origin>, permissions: &[Permission]) -> Prompt {
    let wants = request_phrase(permissions);
    let site = origin.map_or_else(|| "This page".to_owned(), |o| format!("“{}”", o.host_for_display()));
    let mut heading = capitalized(&wants);
    heading.push('?');
    let mut answers = Vec::new();
    if origin.is_some() && permissions.iter().all(|p| p.remembers_allow()) {
        answers.push(Answer::AllowWhileVisiting);
    }
    answers.push(Answer::AllowThisTime);
    if origin.is_some() {
        answers.push(Answer::NeverAllow);
    }
    answers.push(Answer::Dismiss);
    Prompt { heading, body: format!("{site} wants to {wants}."), answers }
}

/// `use your camera and microphone`, `know your location and show notifications`.
fn request_phrase(permissions: &[Permission]) -> String {
    let asked: BTreeSet<Permission> = permissions.iter().copied().collect();
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for (verb, object) in asked.into_iter().map(Permission::request) {
        match groups.iter_mut().find(|(v, _)| *v == verb) {
            Some((_, objects)) => objects.push(object),
            None => groups.push((verb, vec![object])),
        }
    }
    let phrases: Vec<String> = groups.into_iter().map(|(verb, objects)| format!("{verb} {}", listed(&objects))).collect();
    listed(&phrases)
}

/// `a`, `a and b`, `a, b and c`.
fn listed<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [init @ .., last] => {
            let init: Vec<&str> = init.iter().map(AsRef::as_ref).collect();
            format!("{} and {}", init.join(", "), last.as_ref())
        }
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// What a tab is capturing right now, as the engine reports it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Capturing {
    pub camera: bool,
    pub microphone: bool,
    pub screen: bool,
}

impl Capturing {
    pub fn any(self) -> bool {
        self.camera || self.microphone || self.screen
    }

    /// Whether this includes `permission`'s capture.
    pub fn uses(self, permission: Permission) -> bool {
        match permission {
            Permission::Camera => self.camera,
            Permission::Microphone => self.microphone,
            Permission::ScreenShare => self.screen,
            _ => false,
        }
    }

    /// The in-use indicator's text: "Using your camera and microphone", "Sharing your screen
    /// and using your microphone". `None` when nothing is captured.
    pub fn description(self) -> Option<String> {
        let devices = match (self.camera, self.microphone) {
            (true, true) => Some("camera and microphone"),
            (true, false) => Some("camera"),
            (false, true) => Some("microphone"),
            (false, false) => None,
        };
        match (self.screen, devices) {
            (false, None) => None,
            (false, Some(devices)) => Some(format!("Using your {devices}")),
            (true, None) => Some("Sharing your screen".to_owned()),
            (true, Some(devices)) => Some(format!("Sharing your screen and using your {devices}")),
        }
    }
}

/// A permission's state in the site-info popup.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SiteChoice {
    /// Allowed for now with nothing stored: a one-time grant of the tab, or a capture allowed
    /// for its one request (a screen share is never granted).
    AllowedThisTime,
    Ask,
    Allow,
    Block,
}

impl SiteChoice {
    pub fn label(self) -> &'static str {
        match self {
            SiteChoice::AllowedThisTime => "Allowed this time",
            SiteChoice::Ask => "Ask",
            SiteChoice::Allow => "Allow",
            SiteChoice::Block => "Block",
        }
    }

    /// What choosing it stores; `None` = ask. [`SiteChoice::AllowedThisTime`] stores nothing.
    pub fn setting(self) -> Option<Setting> {
        match self {
            SiteChoice::Allow => Some(Setting::Allow),
            SiteChoice::Block => Some(Setting::Block),
            SiteChoice::AllowedThisTime | SiteChoice::Ask => None,
        }
    }
}

/// One row of the site-info popup's Permissions section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteRow {
    pub permission: Permission,
    pub current: SiteChoice,
    /// In the order the choice box lists them.
    pub choices: Vec<SiteChoice>,
    /// The page captures what this permission governs: the row has a Stop button.
    pub live: bool,
}

/// A row for every permission the site has a setting for, was granted this time, or uses, in
/// [`Permission::ALL`] order. `storable`: the page has an origin, so Allow and Block can be
/// remembered for it; Allow is never offered for what is asked every time, nor Ask for what
/// is never asked.
pub fn site_rows(storable: bool, stored: &[(Permission, Setting)], granted: &[Permission], capturing: Capturing) -> Vec<SiteRow> {
    Permission::ALL
        .iter()
        .filter_map(|&permission| {
            let live = capturing.uses(permission);
            let current = match stored.iter().find(|(p, _)| *p == permission).map(|(_, s)| *s) {
                Some(Setting::Allow) => SiteChoice::Allow,
                Some(Setting::Block) => SiteChoice::Block,
                None if permission.asks() && (granted.contains(&permission) || live) => SiteChoice::AllowedThisTime,
                None => return None,
            };
            let mut choices = Vec::new();
            if current == SiteChoice::AllowedThisTime {
                choices.push(SiteChoice::AllowedThisTime);
            }
            if permission.asks() {
                choices.push(SiteChoice::Ask);
            }
            if storable && permission.remembers_allow() {
                choices.push(SiteChoice::Allow);
            }
            if storable {
                choices.push(SiteChoice::Block);
            }
            Some(SiteRow { permission, current, choices, live })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// One-time grants
// ---------------------------------------------------------------------------

/// "Allow this time" grants of one tab. They hold while the tab stays on the site that got them.
#[derive(Clone, Debug, Default)]
pub struct TabGrants {
    site: Option<Origin>,
    granted: BTreeSet<Permission>,
}

impl TabGrants {
    pub fn allows(&self, origin: Option<&Origin>, p: Permission) -> bool {
        self.site.as_ref() == origin && self.granted.contains(&p)
    }

    /// Called on every committed navigation: grants end when the tab leaves their origin.
    /// An opaque origin is never shared by two documents, so its grants end on any commit.
    pub fn committed(&mut self, url: &Url) {
        if self.site.is_none() || Origin::of(url) != self.site {
            *self = TabGrants::default();
        }
    }

    /// For the site-info panel.
    pub fn granted(&self) -> impl Iterator<Item = Permission> {
        self.granted.iter().copied()
    }

    pub fn revoke(&mut self, p: Permission) {
        self.granted.remove(&p);
    }

    fn grant(&mut self, origin: Option<&Origin>, permissions: &[Permission]) {
        if self.site.as_ref() != origin {
            *self = TabGrants { site: origin.cloned(), granted: BTreeSet::new() };
        }
        self.granted.extend(permissions.iter().filter(|p| p.remembers_allow()));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block,
    /// The permissions still undecided, to [`prompt`] for.
    Ask(Vec<Permission>),
}

/// Any block wins, and a permission that is never asked for blocks unless allowed; then
/// everything allowed (stored or granted) allows; else ask for the rest.
fn decision(permissions: &[Permission], mut setting: impl FnMut(Permission) -> Option<Setting>, granted: impl Fn(Permission) -> bool) -> Decision {
    let blocked = |p: Permission, setting: Option<Setting>| match setting {
        Some(Setting::Block) => true,
        Some(Setting::Allow) => false,
        None => !p.asks(),
    };
    if permissions.iter().any(|&p| blocked(p, setting(p))) {
        return Decision::Block;
    }
    let mut undecided: Vec<Permission> = Vec::new();
    for &p in permissions {
        if setting(p) != Some(Setting::Allow) && !granted(p) && !undecided.contains(&p) {
            undecided.push(p);
        }
    }
    if undecided.is_empty() { Decision::Allow } else { Decision::Ask(undecided) }
}

// ---------------------------------------------------------------------------
// Stored settings
// ---------------------------------------------------------------------------

/// Sync record: one per `(origin, permission)`. `None` = ask.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePermissionRecord {
    pub origin: Origin,
    pub permission: Permission,
    pub setting: Lww<Option<Setting>>,
}

impl Lattice for SitePermissionRecord {
    fn join(&mut self, other: Self) {
        self.setting.join(other.setting);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteSetting {
    pub origin: Origin,
    pub permission: Permission,
    pub setting: Setting,
}

/// One site's stored settings, for the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteGroup {
    pub origin: Origin,
    /// The host, or the whole origin when another listed site has the same host
    /// (`http://example.com` and `https://example.com`).
    pub heading: String,
    /// In [`Permission::ALL`] order.
    pub settings: Vec<(Permission, Setting)>,
}

pub struct SitePermissions<'p> {
    pub(crate) p: &'p mut Profile,
}

impl SitePermissions<'_> {
    /// `None` = ask. A stored Allow of a permission that is asked for every time (written by
    /// another build) reads as `None`.
    pub fn get(&mut self, origin: &Origin, p: Permission) -> Option<Setting> {
        load_record(&self.p.conn, origin, p).ok().flatten().and_then(|r| effective(p, r.setting.v))
    }

    /// `None` goes back to asking. No-op (no stamp) when unchanged.
    pub fn set(&mut self, origin: &Origin, p: Permission, setting: Option<Setting>) -> Result<(), Error> {
        self.write(origin, &[p], setting)
    }

    /// The site's settings, in [`Permission::ALL`] order.
    pub fn for_site(&mut self, origin: &Origin) -> Vec<(Permission, Setting)> {
        Permission::ALL.iter().filter_map(|&p| self.get(origin, p).map(|s| (p, s))).collect()
    }

    /// Every stored setting, by origin, then permission.
    pub fn all(&mut self) -> Vec<SiteSetting> {
        let records = load_all(&self.p.conn).unwrap_or_else(|e| {
            log::warn!("site settings: {e}");
            Vec::new()
        });
        let mut out: Vec<SiteSetting> = records
            .into_iter()
            .filter_map(|r| {
                effective(r.permission, r.setting.v).map(|setting| SiteSetting { origin: r.origin, permission: r.permission, setting })
            })
            .collect();
        out.sort_by(|a, b| (&a.origin, a.permission).cmp(&(&b.origin, b.permission)));
        out
    }

    /// Every stored setting, grouped by site in [`all`](Self::all)'s order.
    pub fn by_site(&mut self) -> Vec<SiteGroup> {
        let mut sites: Vec<SiteGroup> = Vec::new();
        for s in self.all() {
            match sites.last_mut() {
                Some(site) if site.origin == s.origin => site.settings.push((s.permission, s.setting)),
                _ => sites.push(SiteGroup {
                    heading: s.origin.host_for_display(),
                    origin: s.origin,
                    settings: vec![(s.permission, s.setting)],
                }),
            }
        }
        let hosts: Vec<String> = sites.iter().map(|site| site.heading.clone()).collect();
        for site in &mut sites {
            if hosts.iter().filter(|h| **h == site.heading).count() > 1 {
                site.heading = site.origin.as_str().to_owned();
            }
        }
        sites
    }

    /// Every setting of the site back to ask, in one transaction.
    pub fn reset_site(&mut self, origin: &Origin) -> Result<(), Error> {
        self.write(origin, Permission::ALL, None)
    }

    /// Whether a request for `permissions` may go ahead. With no origin only `grants` count.
    pub fn decide(&mut self, origin: Option<&Origin>, permissions: &[Permission], grants: &TabGrants) -> Decision {
        decision(permissions, |p| origin.and_then(|o| self.get(o, p)), |p| grants.allows(origin, p))
    }

    /// Applies a prompt answer and returns whether the request is granted. "Allow while
    /// visiting" stores Allow, or with no origin to store it for, holds this time. "Never
    /// allow" stores Block, or with no origin, denies this time.
    pub fn answer(
        &mut self,
        origin: Option<&Origin>,
        permissions: &[Permission],
        answer: Answer,
        grants: &mut TabGrants,
    ) -> Result<bool, Error> {
        match (answer, origin) {
            (Answer::AllowWhileVisiting, Some(origin)) => {
                self.write(origin, permissions, Some(Setting::Allow))?;
                Ok(true)
            }
            (Answer::AllowWhileVisiting | Answer::AllowThisTime, _) => {
                grants.grant(origin, permissions);
                Ok(true)
            }
            (Answer::NeverAllow, Some(origin)) => {
                self.write(origin, permissions, Some(Setting::Block))?;
                Ok(false)
            }
            (Answer::NeverAllow | Answer::Dismiss, _) => Ok(false),
        }
    }

    fn write(&mut self, origin: &Origin, permissions: &[Permission], setting: Option<Setting>) -> Result<(), Error> {
        if setting == Some(Setting::Allow)
            && let Some(&p) = permissions.iter().find(|p| !p.remembers_allow())
        {
            return Err(Error::AlwaysAsks(p));
        }
        self.p.write(|tx| {
            for &permission in permissions {
                let mut rec = load_record(&tx.sql, origin, permission)?.unwrap_or(SitePermissionRecord {
                    origin: origin.clone(),
                    permission,
                    setting: Lww::new(None, Stamp::ZERO),
                });
                if !tx.set_register(&mut rec.setting, setting) {
                    continue;
                }
                let seq = tx.seq();
                store_record(&tx.sql, &rec, seq)?;
            }
            Ok(())
        })
    }
}

fn effective(p: Permission, stored: Option<Setting>) -> Option<Setting> {
    stored.filter(|s| p.settings().contains(s))
}

const COLUMNS: &str = "origin, permission, setting, setting_at, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, SitePermissionRecord), rusqlite::Error> {
    let origin: String = row.get(0)?;
    let origin = Origin::try_from(origin).map_err(|_| crate::db::bad_column(0, "origin"))?;
    let permission: String = row.get(1)?;
    let permission = Permission::from_key(&permission).ok_or_else(|| crate::db::bad_column(1, "permission"))?;
    let setting: Option<String> = row.get(2)?;
    let setting = match setting {
        Some(text) => Some(Setting::from_key(&text).ok_or_else(|| crate::db::bad_column(2, "setting"))?),
        None => None,
    };
    let at = stamp_col(row, 3)?;
    Ok((seq_col(row, 4)?, SitePermissionRecord { origin, permission, setting: Lww::new(setting, at) }))
}

fn load_record(conn: &rusqlite::Connection, origin: &Origin, p: Permission) -> Result<Option<SitePermissionRecord>, Error> {
    let rec = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM site_permissions WHERE origin = ?1 AND permission = ?2"),
            [origin.as_str(), p.key()],
            row_record,
        )
        .optional()?;
    Ok(rec.map(|(_, r)| r))
}

/// Skips rows this build cannot read (a permission a newer build knows), leaving them stored.
/// Any other error, such as a busy database, fails the whole read.
fn load_all(conn: &rusqlite::Connection) -> Result<Vec<SitePermissionRecord>, Error> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM site_permissions WHERE setting IS NOT NULL"))?;
    let rows = stmt
        .query_map([], row_record)?
        .filter_map(|row| match row {
            Ok((_, r)) => Some(Ok(r)),
            Err(e @ rusqlite::Error::FromSqlConversionFailure(..)) => {
                log::warn!("site settings: skipping a row this build cannot read: {e}");
                None
            }
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn store_record(conn: &rusqlite::Connection, rec: &SitePermissionRecord, seq: Seq) -> Result<(), Error> {
    conn.execute(
        &format!(
            "INSERT INTO site_permissions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(origin, permission) DO UPDATE SET setting = excluded.setting, \
             setting_at = excluded.setting_at, seq = excluded.seq"
        ),
        params![
            rec.origin.as_str(),
            rec.permission.key(),
            rec.setting.v.map(Setting::key),
            rec.setting.at.to_vec(),
            seq.0 as i64
        ],
    )?;
    Ok(())
}

pub(crate) struct SitePermissionsTable;

impl SyncTable for SitePermissionsTable {
    const KIND: Kind = Kind::SitePermissions;
    type Record = SitePermissionRecord;

    fn wire_id(rec: &SitePermissionRecord) -> String {
        format!("{}|{}", rec.permission.key(), rec.origin.as_str())
    }

    fn max_stamp(rec: &SitePermissionRecord) -> Option<Stamp> {
        Some(rec.setting.at)
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<SitePermissionRecord>, Error> {
        let key = wire_id.split_once('|').and_then(|(p, o)| Some((Permission::from_key(p)?, Origin::try_from(o.to_owned()).ok()?)));
        match key {
            Some((p, origin)) => load_record(tx, &origin, p),
            None => Ok(None),
        }
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &SitePermissionRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<ChangedRows<SitePermissionRecord>, Error> {
        changed_rows(conn, "site_permissions", COLUMNS, "1", since, limit, row_record)
    }
}
