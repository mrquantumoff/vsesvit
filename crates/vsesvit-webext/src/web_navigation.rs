//! What `chrome.webNavigation` knows of a tab ([`Frames`]): its frames, the document each
//! shows, and the events their loads fire. The shell reports the top frame's loads
//! ([`Load`], from WebKit's load events); a script in every frame of the tab reports the
//! rest ([`Report`]: a subframe's document arriving, any document's `DOMContentLoaded`,
//! `load`, same-document navigations and departure). WebKit gives frames no ids, so a
//! subframe is known by its place in the tree, its index among its parent's frames at each
//! level from the top, and keeps its id while documents come and go there, as in Chrome.

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use vsesvit_core::history::Transition;

use crate::tabs::TabId;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameId(pub u32);

impl FrameId {
    /// A tab's top frame, as Chrome numbers it.
    pub const TOP: FrameId = FrameId(0);

    pub fn from_json(v: &Value) -> Option<FrameId> {
        v.as_u64().and_then(|n| u32::try_from(n).ok()).map(FrameId)
    }
}

/// A load of a tab's top frame, as the shell reports it from WebKit's load events. An
/// error page the shell shows for a failed load is not one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load<'a> {
    Started(&'a str),
    Redirected(&'a str),
    Committed(&'a str, Transition),
    Finished,
    Failed(&'a str, NetError),
}

/// Why a navigation failed, as Chrome names it in `onErrorOccurred`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    /// Stopped, replaced by another navigation, or turned into a download.
    Aborted,
    NameNotResolved,
    ConnectionRefused,
    FileNotFound,
    UnknownUrlScheme,
    CertificateInvalid,
    Failed,
}

impl NetError {
    pub const fn name(self) -> &'static str {
        match self {
            NetError::Aborted => "net::ERR_ABORTED",
            NetError::NameNotResolved => "net::ERR_NAME_NOT_RESOLVED",
            NetError::ConnectionRefused => "net::ERR_CONNECTION_REFUSED",
            NetError::FileNotFound => "net::ERR_FILE_NOT_FOUND",
            NetError::UnknownUrlScheme => "net::ERR_UNKNOWN_URL_SCHEME",
            NetError::CertificateInvalid => "net::ERR_CERT_INVALID",
            NetError::Failed => "net::ERR_FAILED",
        }
    }
}

#[cfg(target_os = "linux")]
impl NetError {
    /// The error of WebKit's `load-failed`.
    pub fn of(error: &webkit::glib::Error) -> NetError {
        use webkit::gio;
        if error.matches(webkit::NetworkError::Cancelled) || error.matches(webkit::PolicyError::FrameLoadInterruptedByPolicyChange) {
            NetError::Aborted
        } else if error.matches(gio::ResolverError::NotFound) || error.matches(gio::ResolverError::TemporaryFailure) {
            NetError::NameNotResolved
        } else if error.matches(gio::IOErrorEnum::ConnectionRefused) {
            NetError::ConnectionRefused
        } else if error.matches(webkit::NetworkError::FileDoesNotExist) {
            NetError::FileNotFound
        } else if error.matches(webkit::NetworkError::UnknownProtocol) {
            NetError::UnknownUrlScheme
        } else {
            NetError::Failed
        }
    }
}

/// What the frame script (`src/js/frames.js`) says about the document it runs in.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Report {
    /// The document's own random token, which tells its reports from a later document's.
    #[serde(rename = "d")]
    pub token: String,
    /// The frame's index among its parent's frames at each level, from the top; empty for
    /// the top frame.
    pub path: Vec<u32>,
    pub url: String,
    #[serde(flatten)]
    pub kind: ReportKind,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum ReportKind {
    /// The document began.
    Start,
    /// The back/forward cache brought the document back.
    Shown,
    /// `DOMContentLoaded`.
    Ready,
    /// The window's `load`.
    Load,
    /// A same-document navigation: to a fragment, or by the History API (`traverse` for
    /// either through the back/forward list).
    Same { hash: bool, traverse: bool },
    /// `pagehide`: the document left, or its frame went away.
    Gone,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    BeforeNavigate,
    Committed,
    DomContentLoaded,
    Completed,
    ErrorOccurred,
    HistoryStateUpdated,
    ReferenceFragmentUpdated,
    CreatedNavigationTarget,
}

impl EventKind {
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::BeforeNavigate => "webNavigation.onBeforeNavigate",
            EventKind::Committed => "webNavigation.onCommitted",
            EventKind::DomContentLoaded => "webNavigation.onDOMContentLoaded",
            EventKind::Completed => "webNavigation.onCompleted",
            EventKind::ErrorOccurred => "webNavigation.onErrorOccurred",
            EventKind::HistoryStateUpdated => "webNavigation.onHistoryStateUpdated",
            EventKind::ReferenceFragmentUpdated => "webNavigation.onReferenceFragmentUpdated",
            EventKind::CreatedNavigationTarget => "webNavigation.onCreatedNavigationTarget",
        }
    }

    /// Whether the event is about a document that exists, which its details name.
    const fn has_document(self) -> bool {
        !matches!(self, EventKind::BeforeNavigate | EventKind::ErrorOccurred | EventKind::CreatedNavigationTarget)
    }
}

/// One event, with its details but the `timeStamp`, which the runtime adds as it fires it.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub frame: FrameId,
    pub details: Map<String, Value>,
}

impl Event {
    /// `onCreatedNavigationTarget`: `source`'s top frame opened `tab` at `url` (WebKit does
    /// not say which frame did).
    pub fn created_navigation_target(source: TabId, tab: TabId, url: &str) -> Event {
        let details = json!({ "sourceTabId": source.0, "sourceProcessId": -1, "sourceFrameId": 0, "tabId": tab.0, "url": url });
        Event { kind: EventKind::CreatedNavigationTarget, frame: FrameId::TOP, details: object(details) }
    }

    fn with(mut self, key: &str, value: Value) -> Event {
        self.details.insert(key.to_owned(), value);
        self
    }
}

/// How far a document has loaded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Stage {
    Loading,
    /// `DOMContentLoaded` fired.
    Ready,
    /// `onCompleted` fired, or the load ended without it.
    Complete,
}

#[derive(Clone, Debug)]
struct Document {
    id: String,
    /// The frame script's token, once its first report arrives.
    token: Option<String>,
    stage: Stage,
}

impl Document {
    fn new(token: Option<&str>, stage: Stage) -> Document {
        Document { id: new_document_id(), token: token.map(str::to_owned), stage }
    }
}

#[derive(Clone, Debug)]
struct Frame {
    parent: Option<FrameId>,
    path: Vec<u32>,
    url: String,
    /// `None` once the frame's document left and no other arrived.
    document: Option<Document>,
    /// The frame's last navigation failed.
    error: bool,
    /// The frame has navigated, beyond its initial empty document.
    navigated: bool,
}

/// The top frame's navigation between its start and its commit.
#[derive(Clone, Debug)]
struct Pending {
    /// Where it goes, once WebKit says: it starts loading a window a page opened before it
    /// does, so that window's `onBeforeNavigate` waits for the URL.
    url: Option<String>,
    redirected: bool,
}

/// One tab's frames.
#[derive(Clone, Debug)]
pub struct Frames {
    tab: TabId,
    next: u32,
    frames: BTreeMap<FrameId, Frame>,
    pending: Option<Pending>,
}

impl Frames {
    /// A tab holds its initial empty document.
    pub fn new(tab: TabId) -> Frames {
        let top = Frame { parent: None, path: Vec::new(), url: "about:blank".into(), document: Some(Document::new(None, Stage::Complete)), error: false, navigated: false };
        Frames { tab, next: 1, frames: BTreeMap::from([(FrameId::TOP, top)]), pending: None }
    }

    pub fn load(&mut self, load: Load) -> Vec<Event> {
        match load {
            Load::Started(url) => {
                let url = Some(url.to_owned()).filter(|u| !u.is_empty());
                let events = url.iter().map(|url| self.event(EventKind::BeforeNavigate, FrameId::TOP, url)).collect();
                self.pending = Some(Pending { url, redirected: false });
                events
            }
            Load::Redirected(url) => {
                let Some(pending) = &mut self.pending else { return Vec::new() };
                let announced = pending.url.replace(url.to_owned()).is_some();
                pending.redirected = true;
                if announced { Vec::new() } else { vec![self.event(EventKind::BeforeNavigate, FrameId::TOP, url)] }
            }
            Load::Committed(url, transition) => {
                let pending = self.pending.take();
                // A load nothing announced, such as a document restored with the session.
                let announced = pending.as_ref().is_some_and(|p| p.url.is_some());
                let mut events = if announced { Vec::new() } else { vec![self.event(EventKind::BeforeNavigate, FrameId::TOP, url)] };
                self.frames.retain(|id, _| *id == FrameId::TOP);
                let top = self.frame_mut(FrameId::TOP);
                top.url = url.to_owned();
                top.error = false;
                top.navigated = true;
                top.document = Some(Document::new(None, Stage::Loading));
                let (kind, mut qualifiers) = chrome_transition(transition);
                if pending.is_some_and(|p| p.redirected) {
                    qualifiers.push("server_redirect");
                }
                events.push(self.event(EventKind::Committed, FrameId::TOP, url).with("transitionType", json!(kind)).with("transitionQualifiers", json!(qualifiers)));
                events
            }
            Load::Finished => self.complete(FrameId::TOP),
            // Before the commit the navigation fails; after it, the document's load ends
            // short, which Chrome reports as nothing.
            Load::Failed(url, error) => match &self.pending {
                Some(pending) if pending.url.as_deref().is_none_or(|u| u == url) => {
                    let announced = pending.url.is_some();
                    self.pending = None;
                    self.frame_mut(FrameId::TOP).error = true;
                    let mut events = if announced { Vec::new() } else { vec![self.event(EventKind::BeforeNavigate, FrameId::TOP, url)] };
                    events.push(self.event(EventKind::ErrorOccurred, FrameId::TOP, url).with("error", json!(error.name())));
                    events
                }
                Some(_) => Vec::new(),
                None => {
                    if let Some(document) = &mut self.frame_mut(FrameId::TOP).document {
                        document.stage = Stage::Complete;
                    }
                    Vec::new()
                }
            },
        }
    }

    pub fn report(&mut self, report: &Report) -> Vec<Event> {
        match report.kind {
            ReportKind::Start | ReportKind::Shown if report.path.is_empty() => {
                // The shell reported the commit before the document's script could run.
                if let Some(document) = self.frame_mut(FrameId::TOP).document.as_mut().filter(|d| d.token.is_none()) {
                    document.token = Some(report.token.clone());
                }
                Vec::new()
            }
            // A new frame's initial empty document is no navigation, as in Chrome.
            ReportKind::Start => {
                let initial = report.url == "about:blank" && self.at_path(&report.path).is_none();
                self.subframe_document(report, !initial)
            }
            ReportKind::Shown => self.subframe_document(report, false),
            kind => {
                let Some(id) = self.by_token(&report.token) else { return Vec::new() };
                self.frame_mut(id).path.clone_from(&report.path);
                match kind {
                    ReportKind::Ready => self.ready(id),
                    // The shell's `Finished` completes the top frame.
                    ReportKind::Load if id != FrameId::TOP => self.complete(id),
                    ReportKind::Same { hash, traverse } => {
                        self.frame_mut(id).url.clone_from(&report.url);
                        let kind = if hash { EventKind::ReferenceFragmentUpdated } else { EventKind::HistoryStateUpdated };
                        let qualifiers: &[&str] = if traverse { &["forward_back"] } else { &[] };
                        let transition = if id == FrameId::TOP { "link" } else { "auto_subframe" };
                        vec![self.event(kind, id, &report.url).with("transitionType", json!(transition)).with("transitionQualifiers", json!(qualifiers))]
                    }
                    ReportKind::Gone if id != FrameId::TOP => {
                        self.frame_mut(id).document = None;
                        self.remove_descendants(id);
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
        }
    }

    /// `getFrame`'s details of a frame that shows a document.
    pub fn frame_details(&self, id: FrameId) -> Option<Map<String, Value>> {
        let frame = self.frames.get(&id)?;
        let document = frame.document.as_ref()?;
        let mut details = self.base(id, &frame.url);
        details.remove("tabId");
        details.remove("frameId");
        details.remove("processId");
        details.insert("documentId".into(), json!(document.id));
        details.insert("errorOccurred".into(), json!(frame.error));
        Some(details)
    }

    /// `getAllFrames`: every frame that shows a document, the top first.
    pub fn all_frames(&self) -> Vec<Value> {
        self.frames
            .keys()
            .filter_map(|id| {
                let mut details = self.frame_details(*id)?;
                details.insert("frameId".into(), json!(id.0));
                details.insert("processId".into(), json!(-1));
                Some(Value::Object(details))
            })
            .collect()
    }

    /// The frame showing the document `document_id`.
    pub fn find_document(&self, document_id: &str) -> Option<FrameId> {
        self.frames.iter().find(|(_, f)| f.document.as_ref().is_some_and(|d| d.id == document_id)).map(|(id, _)| *id)
    }

    /// A subframe's new document: the frame at its place gets it (a new frame when there
    /// is none), and loses the frames the previous document held. `announce` is false for a
    /// document that arrived by no navigation: a frame's initial one, or one the back/forward
    /// cache restores.
    fn subframe_document(&mut self, report: &Report, announce: bool) -> Vec<Event> {
        let Some((_, parent_path)) = report.path.split_last() else { return Vec::new() };
        let Some(parent) = self.at_path(parent_path).filter(|p| self.frames[p].document.is_some()) else { return Vec::new() };
        let existing = self.at_path(&report.path);
        let id = existing.unwrap_or_else(|| {
            let id = FrameId(self.next);
            self.next += 1;
            id
        });
        self.remove_descendants(id);
        let navigated = existing.is_some_and(|id| self.frames[&id].navigated);
        let stage = if announce { Stage::Loading } else { Stage::Complete };
        let document = Some(Document::new(Some(&report.token), stage));
        let frame = Frame { parent: Some(parent), path: report.path.clone(), url: report.url.clone(), document, error: false, navigated: navigated || announce };
        self.frames.insert(id, frame);
        if !announce {
            return Vec::new();
        }
        // Chrome's names for a frame's first navigation and for the ones after it.
        let transition = if navigated { "manual_subframe" } else { "auto_subframe" };
        vec![
            self.event(EventKind::BeforeNavigate, id, &report.url),
            self.event(EventKind::Committed, id, &report.url).with("transitionType", json!(transition)).with("transitionQualifiers", json!([])),
        ]
    }

    fn ready(&mut self, id: FrameId) -> Vec<Event> {
        let Some(document) = self.frame_mut(id).document.as_mut().filter(|d| d.stage == Stage::Loading) else { return Vec::new() };
        document.stage = Stage::Ready;
        vec![self.event(EventKind::DomContentLoaded, id, &self.frames[&id].url)]
    }

    /// `onCompleted`, after `onDOMContentLoaded` if that is still to come.
    fn complete(&mut self, id: FrameId) -> Vec<Event> {
        let mut events = self.ready(id);
        let Some(document) = self.frame_mut(id).document.as_mut().filter(|d| d.stage == Stage::Ready) else { return events };
        document.stage = Stage::Complete;
        events.push(self.event(EventKind::Completed, id, &self.frames[&id].url));
        events
    }

    /// The frame at `path`, one showing a document before one that no longer does.
    fn at_path(&self, path: &[u32]) -> Option<FrameId> {
        self.frames.iter().filter(|(_, f)| f.path == path).max_by_key(|(_, f)| f.document.is_some()).map(|(id, _)| *id)
    }

    fn by_token(&self, token: &str) -> Option<FrameId> {
        self.frames.iter().find(|(_, f)| f.document.as_ref().and_then(|d| d.token.as_deref()) == Some(token)).map(|(id, _)| *id)
    }

    fn remove_descendants(&mut self, id: FrameId) {
        let mut parents = vec![id];
        while let Some(parent) = parents.pop() {
            let children: Vec<FrameId> = self.frames.iter().filter(|(_, f)| f.parent == Some(parent)).map(|(child, _)| *child).collect();
            for child in children {
                self.frames.remove(&child);
                parents.push(child);
            }
        }
    }

    fn frame_mut(&mut self, id: FrameId) -> &mut Frame {
        self.frames.get_mut(&id).expect("the frames hold every id they hand out, the top's always")
    }

    /// The details every event about a frame starts from.
    fn base(&self, id: FrameId, url: &str) -> Map<String, Value> {
        let frame = &self.frames[&id];
        let parent_document = frame.parent.and_then(|p| self.frames.get(&p)).and_then(|p| p.document.as_ref());
        let mut details = object(json!({
            "tabId": self.tab.0,
            "frameId": id.0,
            "parentFrameId": frame.parent.map_or(-1, |p| i64::from(p.0)),
            "processId": -1,
            "url": url,
            "frameType": if id == FrameId::TOP { "outermost_frame" } else { "sub_frame" },
            "documentLifecycle": "active",
        }));
        if let Some(parent) = parent_document {
            details.insert("parentDocumentId".into(), json!(parent.id));
        }
        details
    }

    fn event(&self, kind: EventKind, id: FrameId, url: &str) -> Event {
        let mut details = self.base(id, url);
        if kind.has_document()
            && let Some(document) = &self.frames[&id].document
        {
            details.insert("documentId".into(), json!(document.id));
        }
        Event { kind, frame: id, details }
    }
}

/// `getFrame`'s details: a document, or a tab and a frame in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameQuery {
    Document { id: String, tab: Option<TabId>, frame: Option<FrameId> },
    Frame { tab: TabId, frame: FrameId },
}

impl FrameQuery {
    pub fn parse(v: &Value) -> Result<FrameQuery, String> {
        let tab = TabId::from_json(&v["tabId"]);
        let frame = FrameId::from_json(&v["frameId"]);
        match (&v["documentId"], tab, frame) {
            (Value::String(id), tab, frame) if is_document_id(id) => Ok(FrameQuery::Document { id: id.clone(), tab, frame }),
            (Value::Null, Some(tab), Some(frame)) => Ok(FrameQuery::Frame { tab, frame }),
            (Value::Null, ..) => Err("Either documentId or both tabId and frameId must be specified.".into()),
            _ => Err("Invalid documentId.".into()),
        }
    }

    /// Whether the frame found for a document is the one the query also names by tab and
    /// frame, if it does.
    pub fn agrees(&self, tab: TabId, frame: FrameId) -> Result<(), String> {
        match self {
            FrameQuery::Document { tab: t, frame: f, .. } if t.is_some_and(|t| t != tab) || f.is_some_and(|f| f != frame) => {
                Err("tabId and frameId mismatch with documentId.".into())
            }
            _ => Ok(()),
        }
    }
}

fn is_document_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Chrome's `transitionType` and `transitionQualifiers` for what history recorded.
fn chrome_transition(transition: Transition) -> (&'static str, Vec<&'static str>) {
    match transition {
        Transition::Link => ("link", Vec::new()),
        Transition::Typed => ("typed", vec!["from_address_bar"]),
        Transition::Bookmark => ("auto_bookmark", Vec::new()),
        Transition::Reload => ("reload", Vec::new()),
        Transition::Redirect => ("link", vec!["client_redirect"]),
        Transition::FormSubmit => ("form_submit", Vec::new()),
    }
}

/// A 32-digit hex id, like Chrome's, unique in the process.
fn new_document_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    static SALT: OnceLock<u64> = OnceLock::new();
    let salt = *SALT.get_or_init(|| RandomState::new().hash_one(0u8));
    format!("{salt:016X}{:016X}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAB: TabId = TabId(4);

    fn report(kind: ReportKind, token: &str, path: &[u32], url: &str) -> Report {
        Report { token: token.into(), path: path.to_vec(), url: url.into(), kind }
    }

    fn names(events: &[Event]) -> Vec<(&'static str, u32)> {
        events.iter().map(|e| (e.kind.name().trim_start_matches("webNavigation."), e.frame.0)).collect()
    }

    /// A tab that loaded `http://a.test/` and whose document's script reported in as `top`.
    fn loaded() -> Frames {
        let mut frames = Frames::new(TAB);
        frames.load(Load::Started("http://a.test/"));
        frames.load(Load::Committed("http://a.test/", Transition::Typed));
        frames.report(&report(ReportKind::Start, "top", &[], "http://a.test/"));
        frames
    }

    #[test]
    fn a_top_frame_load_fires_chromes_events_in_order() {
        let mut frames = Frames::new(TAB);
        let mut events = frames.load(Load::Started("http://a.test/"));
        events.extend(frames.load(Load::Redirected("https://a.test/")));
        events.extend(frames.load(Load::Committed("https://a.test/", Transition::Typed)));
        events.extend(frames.report(&report(ReportKind::Start, "top", &[], "https://a.test/")));
        events.extend(frames.report(&report(ReportKind::Ready, "top", &[], "https://a.test/")));
        events.extend(frames.report(&report(ReportKind::Load, "top", &[], "https://a.test/")));
        events.extend(frames.load(Load::Finished));
        events.extend(frames.load(Load::Finished));
        assert_eq!(names(&events), [("onBeforeNavigate", 0), ("onCommitted", 0), ("onDOMContentLoaded", 0), ("onCompleted", 0)]);
        let before = &events[0].details;
        assert_eq!(Value::Object(before.clone()), json!({ "tabId": 4, "frameId": 0, "parentFrameId": -1, "processId": -1, "url": "http://a.test/", "frameType": "outermost_frame", "documentLifecycle": "active" }));
        let committed = &events[1].details;
        assert_eq!(committed["url"], "https://a.test/");
        assert_eq!(committed["transitionType"], "typed");
        assert_eq!(committed["transitionQualifiers"], json!(["from_address_bar", "server_redirect"]));
        let document = committed["documentId"].as_str().unwrap();
        assert!(is_document_id(document), "{document}");
        assert!(events[2..].iter().all(|e| e.details["documentId"] == document));
    }

    #[test]
    fn the_shell_completes_a_top_frame_whose_script_never_reported() {
        let mut frames = Frames::new(TAB);
        frames.load(Load::Started("data:text/plain,x"));
        frames.load(Load::Committed("data:text/plain,x", Transition::Link));
        assert_eq!(names(&frames.load(Load::Finished)), [("onDOMContentLoaded", 0), ("onCompleted", 0)]);
    }

    #[test]
    fn a_commit_nothing_announced_is_announced_with_it() {
        let mut frames = Frames::new(TAB);
        assert_eq!(names(&frames.load(Load::Committed("http://a.test/", Transition::Reload))), [("onBeforeNavigate", 0), ("onCommitted", 0)]);
    }

    #[test]
    fn a_load_started_before_webkit_says_where_is_announced_once_it_does() {
        let mut frames = Frames::new(TAB);
        assert!(frames.load(Load::Started("")).is_empty());
        let events = frames.load(Load::Committed("http://a.test/", Transition::Link));
        assert_eq!(names(&events), [("onBeforeNavigate", 0), ("onCommitted", 0)]);
        assert_eq!(events[0].details["url"], "http://a.test/");
        frames.load(Load::Started(""));
        assert_eq!(names(&frames.load(Load::Redirected("http://b.test/"))), [("onBeforeNavigate", 0)]);
        frames.load(Load::Started(""));
        assert_eq!(names(&frames.load(Load::Failed("http://c.test/", NetError::Failed))), [("onBeforeNavigate", 0), ("onErrorOccurred", 0)]);
    }

    #[test]
    fn a_failed_navigation_reports_its_error_and_completes_nothing() {
        let mut frames = loaded();
        frames.load(Load::Finished);
        frames.load(Load::Started("http://gone.test/"));
        let failed = frames.load(Load::Failed("http://gone.test/", NetError::NameNotResolved));
        assert_eq!(names(&failed), [("onErrorOccurred", 0)]);
        assert_eq!(failed[0].details["error"], "net::ERR_NAME_NOT_RESOLVED");
        assert_eq!(failed[0].details["url"], "http://gone.test/");
        assert!(failed[0].details.get("documentId").is_none());
        assert!(frames.load(Load::Finished).is_empty());
        assert_eq!(frames.frame_details(FrameId::TOP).unwrap()["errorOccurred"], true);
        frames.load(Load::Committed("http://b.test/", Transition::Link));
        assert_eq!(frames.frame_details(FrameId::TOP).unwrap()["errorOccurred"], false);
    }

    /// The failure of a load another one replaced belongs to neither the new navigation nor
    /// the document on screen.
    #[test]
    fn a_failure_ends_only_its_own_load() {
        let mut frames = loaded();
        frames.load(Load::Started("http://b.test/"));
        assert!(frames.load(Load::Failed("http://a.test/", NetError::Aborted)).is_empty());
        assert_eq!(names(&frames.load(Load::Committed("http://b.test/", Transition::Link))), [("onCommitted", 0)]);
        assert!(frames.load(Load::Failed("http://b.test/", NetError::Failed)).is_empty());
        assert!(frames.load(Load::Finished).is_empty(), "a load that ended short does not complete");
    }

    #[test]
    fn subframes_are_numbered_by_their_place_and_nest() {
        let mut frames = loaded();
        let mut events = frames.report(&report(ReportKind::Start, "child", &[0], "http://b.test/child"));
        events.extend(frames.report(&report(ReportKind::Start, "grand", &[0, 0], "http://c.test/grand")));
        events.extend(frames.report(&report(ReportKind::Ready, "grand", &[0, 0], "http://c.test/grand")));
        events.extend(frames.report(&report(ReportKind::Load, "grand", &[0, 0], "http://c.test/grand")));
        events.extend(frames.report(&report(ReportKind::Load, "child", &[0], "http://b.test/child")));
        assert_eq!(
            names(&events),
            [("onBeforeNavigate", 1), ("onCommitted", 1), ("onBeforeNavigate", 2), ("onCommitted", 2), ("onDOMContentLoaded", 2), ("onCompleted", 2), ("onDOMContentLoaded", 1), ("onCompleted", 1)]
        );
        let child = &events[1].details;
        assert_eq!(child["parentFrameId"], 0);
        assert_eq!(child["frameType"], "sub_frame");
        assert_eq!(child["transitionType"], "auto_subframe");
        assert_eq!(child["parentDocumentId"], frames.frame_details(FrameId::TOP).unwrap()["documentId"]);
        assert_eq!(events[3].details["parentFrameId"], 1);
        let all = frames.all_frames();
        let ids: Vec<(u64, i64)> = all.iter().map(|f| (f["frameId"].as_u64().unwrap(), f["parentFrameId"].as_i64().unwrap())).collect();
        assert_eq!(ids, [(0, -1), (1, 0), (2, 1)]);
        assert_eq!(all[2]["url"], "http://c.test/grand");
        assert_eq!(all[2]["processId"], -1);
        let document = all[2]["documentId"].as_str().unwrap();
        assert_eq!(frames.find_document(document), Some(FrameId(2)));
    }

    /// A frame keeps its id across its documents, and its old document's late reports
    /// change nothing.
    #[test]
    fn a_subframe_keeps_its_id_as_it_navigates() {
        let mut frames = loaded();
        frames.report(&report(ReportKind::Start, "first", &[0], "http://b.test/1"));
        frames.report(&report(ReportKind::Start, "inner", &[0, 0], "http://b.test/inner"));
        let events = frames.report(&report(ReportKind::Start, "second", &[0], "http://b.test/2"));
        assert_eq!(names(&events), [("onBeforeNavigate", 1), ("onCommitted", 1)]);
        assert_eq!(events[1].details["transitionType"], "manual_subframe");
        assert!(frames.report(&report(ReportKind::Gone, "first", &[0], "http://b.test/1")).is_empty());
        assert!(frames.report(&report(ReportKind::Load, "first", &[0], "http://b.test/1")).is_empty());
        let all = frames.all_frames();
        assert_eq!(all.len(), 2, "the old document's frame went with it: {all:?}");
        assert_eq!(all[1]["url"], "http://b.test/2");
    }

    #[test]
    fn a_new_frames_initial_empty_document_is_listed_but_not_announced() {
        let mut frames = loaded();
        assert!(frames.report(&report(ReportKind::Start, "blank", &[0], "about:blank")).is_empty());
        assert!(frames.report(&report(ReportKind::Load, "blank", &[0], "about:blank")).is_empty());
        assert_eq!(frames.all_frames()[1]["url"], "about:blank");
        let events = frames.report(&report(ReportKind::Start, "page", &[0], "http://b.test/"));
        assert_eq!(names(&events), [("onBeforeNavigate", 1), ("onCommitted", 1)]);
        assert_eq!(events[1].details["transitionType"], "auto_subframe", "its first navigation");
        let blank = frames.report(&report(ReportKind::Start, "blank-again", &[0], "about:blank"));
        assert_eq!(names(&blank), [("onBeforeNavigate", 1), ("onCommitted", 1)], "a navigation to about:blank is one");
        assert_eq!(blank[1].details["transitionType"], "manual_subframe");
    }

    #[test]
    fn a_frame_that_goes_leaves_the_list_and_a_new_document_there_takes_its_id() {
        let mut frames = loaded();
        frames.report(&report(ReportKind::Start, "child", &[0], "http://b.test/"));
        frames.report(&report(ReportKind::Start, "grand", &[0, 0], "http://c.test/"));
        frames.report(&report(ReportKind::Gone, "child", &[0], "http://b.test/"));
        assert_eq!(frames.all_frames().len(), 1);
        assert_eq!(frames.frame_details(FrameId(1)), None);
        let events = frames.report(&report(ReportKind::Start, "again", &[0], "http://d.test/"));
        assert_eq!(names(&events), [("onBeforeNavigate", 1), ("onCommitted", 1)]);
        assert!(frames.report(&report(ReportKind::Gone, "top", &[], "http://a.test/")).is_empty());
        assert_eq!(frames.all_frames().len(), 2, "the top frame stays");
    }

    #[test]
    fn a_new_top_document_takes_the_subframes_with_the_old_one() {
        let mut frames = loaded();
        frames.report(&report(ReportKind::Start, "child", &[0], "http://b.test/"));
        frames.load(Load::Started("http://e.test/"));
        assert_eq!(frames.all_frames().len(), 2, "the old document stays until the commit");
        frames.load(Load::Committed("http://e.test/", Transition::Link));
        assert_eq!(frames.all_frames().len(), 1);
        assert!(frames.report(&report(ReportKind::Ready, "child", &[0], "http://b.test/")).is_empty());
        assert!(frames.report(&report(ReportKind::Ready, "top", &[], "http://a.test/")).is_empty(), "the old top document's report");
        let events = frames.report(&report(ReportKind::Start, "child2", &[0], "http://b.test/"));
        assert_eq!(events[0].frame, FrameId(2), "ids are not reused within a tab");
    }

    #[test]
    fn a_subframe_whose_parent_is_unknown_is_not_placed() {
        let mut frames = loaded();
        assert!(frames.report(&report(ReportKind::Start, "orphan", &[3, 1], "http://b.test/")).is_empty());
        assert_eq!(frames.all_frames().len(), 1);
    }

    #[test]
    fn same_document_navigations_are_fragments_or_history() {
        let mut frames = loaded();
        frames.report(&report(ReportKind::Start, "child", &[0], "http://b.test/"));
        let fragment = frames.report(&report(ReportKind::Same { hash: true, traverse: false }, "top", &[], "http://a.test/#x"));
        assert_eq!(names(&fragment), [("onReferenceFragmentUpdated", 0)]);
        assert_eq!(fragment[0].details["url"], "http://a.test/#x");
        assert_eq!(fragment[0].details["transitionType"], "link");
        assert_eq!(fragment[0].details["transitionQualifiers"], json!([]));
        let back = frames.report(&report(ReportKind::Same { hash: false, traverse: true }, "child", &[0], "http://b.test/state"));
        assert_eq!(names(&back), [("onHistoryStateUpdated", 1)]);
        assert_eq!(back[0].details["transitionType"], "auto_subframe");
        assert_eq!(back[0].details["transitionQualifiers"], json!(["forward_back"]));
        assert_eq!(frames.all_frames()[1]["url"], "http://b.test/state");
    }

    #[test]
    fn a_restored_subframe_is_listed_without_events() {
        let mut frames = loaded();
        assert!(frames.report(&report(ReportKind::Shown, "cached", &[0], "http://b.test/")).is_empty());
        assert_eq!(frames.all_frames().len(), 2);
        assert!(frames.report(&report(ReportKind::Load, "cached", &[0], "http://b.test/")).is_empty(), "it had loaded");
    }

    #[test]
    fn navigation_targets_name_both_tabs() {
        let event = Event::created_navigation_target(TabId(1), TabId(2), "http://a.test/");
        assert_eq!(event.kind.name(), "webNavigation.onCreatedNavigationTarget");
        assert_eq!(Value::Object(event.details), json!({ "sourceTabId": 1, "sourceProcessId": -1, "sourceFrameId": 0, "tabId": 2, "url": "http://a.test/" }));
    }

    #[test]
    fn frame_queries_take_a_document_or_a_tab_and_frame() {
        let document = "0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(FrameQuery::parse(&json!({ "tabId": 3, "frameId": 0 })), Ok(FrameQuery::Frame { tab: TabId(3), frame: FrameId::TOP }));
        assert_eq!(FrameQuery::parse(&json!({ "documentId": document })), Ok(FrameQuery::Document { id: document.into(), tab: None, frame: None }));
        assert_eq!(FrameQuery::parse(&json!({ "tabId": 3 })), Err("Either documentId or both tabId and frameId must be specified.".into()));
        assert_eq!(FrameQuery::parse(&json!({ "documentId": "nope" })), Err("Invalid documentId.".into()));
        let query = FrameQuery::parse(&json!({ "documentId": document, "tabId": 3 })).unwrap();
        assert_eq!(query.agrees(TabId(3), FrameId(5)), Ok(()));
        assert_eq!(query.agrees(TabId(4), FrameId(5)), Err("tabId and frameId mismatch with documentId.".into()));
    }

    #[test]
    fn reports_parse_from_the_frame_script() {
        let parsed: Report = serde_json::from_str(r#"{"k":"same","d":"t","path":[1,0],"url":"http://a.test/#y","hash":true,"traverse":false}"#).unwrap();
        assert_eq!(parsed, report(ReportKind::Same { hash: true, traverse: false }, "t", &[1, 0], "http://a.test/#y"));
        let parsed: Report = serde_json::from_str(r#"{"k":"start","d":"t","path":[],"url":"about:blank"}"#).unwrap();
        assert_eq!(parsed.kind, ReportKind::Start);
        assert!(serde_json::from_str::<Report>(r#"{"k":"other","d":"t","path":[],"url":""}"#).is_err());
    }
}
