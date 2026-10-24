//! Owned by W0.2.
//! Prompts: one request/answer protocol for every question the engine asks and waits on (design
//! section 1.5). [`PromptLedger`] is its bookkeeping (scope, admission, exactly once), shared by
//! `FakeEngine` and available to the glue's `prompts.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::ids::{MenuCommandId, PromptId, TabId};
use crate::types::{ContextMenuParams, Origin, PagePoint, PermissionAnswer, PermissionRequest, Url};

/// A question the engine waits on. Arrives as `BrowserEvent::Prompt`.
#[allow(clippy::large_enum_variant, reason = "a prompt is rare and moved once; the contract keeps its plain shape")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptRequest {
    /// A context menu: the engine's model plus the shell's own items, shown by the shell.
    ContextMenu {
        /// The tab.
        tab: TabId,
        /// Where it was opened.
        at: PagePoint,
        /// What it was opened on.
        params: ContextMenuParams,
        /// The engine's items, extension items included.
        model: MenuModel,
    },
    /// `alert`, `confirm` or `prompt`.
    JsDialog {
        /// The tab.
        tab: TabId,
        /// The page's origin, if it has one.
        origin: Option<Origin>,
        /// Which dialog.
        kind: JsDialogKind,
        /// The page's text.
        message: String,
    },
    /// `beforeunload`: leave the page or stay.
    BeforeUnload {
        /// The tab.
        tab: TabId,
        /// Whether the page is being reloaded rather than left.
        is_reload: bool,
    },
    /// `<input type=file>` and the File System Access pickers.
    FileChooser {
        /// The tab.
        tab: TabId,
        /// What to pick.
        mode: FileChooserMode,
        /// The page's title for the dialog.
        title: Option<String>,
        /// Where to start.
        default_path: Option<PathBuf>,
        /// The accepted types.
        accept: Vec<AcceptFilter>,
    },
    /// HTTP or proxy authentication. `requester` is CEF's `origin_url`, the page that made the request;
    /// `challenger` is the server or proxy that sent the challenge. The dialog names the challenger (R2-3).
    Auth {
        /// The tab, if the request belongs to one.
        tab: Option<TabId>,
        /// The page that made the request.
        requester: Option<Url>,
        /// Who asks for credentials.
        challenger: AuthChallenger,
        /// The realm.
        realm: Option<String>,
        /// The scheme (`basic`, `digest`); `None` for FTP.
        scheme: Option<String>,
    },
    /// A site asks for permissions.
    Permission(PermissionRequest),
}

impl PromptRequest {
    /// Its kind.
    pub fn kind(&self) -> PromptKind {
        match self {
            PromptRequest::ContextMenu { .. } => PromptKind::ContextMenu,
            PromptRequest::JsDialog { .. } => PromptKind::JsDialog,
            PromptRequest::BeforeUnload { .. } => PromptKind::BeforeUnload,
            PromptRequest::FileChooser { .. } => PromptKind::FileChooser,
            PromptRequest::Auth { .. } => PromptKind::Auth,
            PromptRequest::Permission(_) => PromptKind::Permission,
        }
    }

    /// The tab it belongs to. Only an auth request can belong to none.
    pub fn tab(&self) -> Option<TabId> {
        match self {
            PromptRequest::ContextMenu { tab, .. }
            | PromptRequest::JsDialog { tab, .. }
            | PromptRequest::BeforeUnload { tab, .. }
            | PromptRequest::FileChooser { tab, .. } => Some(*tab),
            PromptRequest::Auth { tab, .. } => *tab,
            PromptRequest::Permission(request) => Some(request.tab),
        }
    }
}

/// The server or proxy that sent an auth challenge (CEF's `host`, `port`, `isProxy`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthChallenger {
    /// Host name.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Whether it is a proxy.
    pub is_proxy: bool,
}

/// Which JavaScript dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsDialogKind {
    /// `alert`.
    Alert,
    /// `confirm`.
    Confirm,
    /// `prompt`, with the page's default text.
    Prompt {
        /// The text field's initial value.
        default: String,
    },
}

/// What a file chooser picks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileChooserMode {
    /// One existing file.
    Open,
    /// Several existing files.
    OpenMultiple,
    /// A folder.
    OpenFolder,
    /// A file to write.
    Save,
}

/// One accepted type of a file chooser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptFilter {
    /// A label for the filter, if the page gave one.
    pub label: Option<String>,
    /// MIME types and extensions: `"image/*"`, `".png"`.
    pub patterns: Vec<String>,
}

/// An owned, engine-neutral copy of Chromium's menu, extension items included (EA section 5).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MenuModel {
    /// The entries, in order.
    pub entries: Vec<MenuEntry>,
}

/// An entry of a [`MenuModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    /// A command; choosing it answers `PromptAnswer::Menu(Some(id))`.
    Command {
        /// The engine's id.
        id: MenuCommandId,
        /// The label.
        label: String,
        /// Whether it can be chosen.
        enabled: bool,
        /// `Some` for a check or radio item.
        checked: Option<bool>,
        /// The radio group, for a radio item.
        radio_group: Option<i32>,
    },
    /// A submenu.
    Submenu {
        /// The label.
        label: String,
        /// Whether it opens.
        enabled: bool,
        /// Its entries.
        entries: Vec<MenuEntry>,
    },
    /// A separator.
    Separator,
}

/// The shell's answer to a prompt. Its kind must match the request's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptAnswer {
    /// An engine command, or `None`: dismissed, or the user chose a shell item (the shell runs it).
    Menu(Option<MenuCommandId>),
    /// Accept or cancel a dialog, with `prompt`'s text.
    JsDialog {
        /// OK rather than Cancel.
        accept: bool,
        /// The text, for `prompt`.
        text: Option<String>,
    },
    /// Leave the page or stay.
    BeforeUnload {
        /// Leave.
        leave: bool,
    },
    /// The chosen paths, or `None` when cancelled.
    Files(Option<Vec<PathBuf>>),
    /// User name and password, or `None` when cancelled.
    Auth(Option<(String, String)>),
    /// The permission answer.
    Permission(PermissionAnswer),
}

impl PromptAnswer {
    /// The kind of prompt it answers.
    pub fn kind(&self) -> PromptKind {
        match self {
            PromptAnswer::Menu(_) => PromptKind::ContextMenu,
            PromptAnswer::JsDialog { .. } => PromptKind::JsDialog,
            PromptAnswer::BeforeUnload { .. } => PromptKind::BeforeUnload,
            PromptAnswer::Files(_) => PromptKind::FileChooser,
            PromptAnswer::Auth(_) => PromptKind::Auth,
            PromptAnswer::Permission(_) => PromptKind::Permission,
        }
    }
}

/// The kinds of prompt, one per [`PromptRequest`] variant.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PromptKind {
    /// [`PromptRequest::ContextMenu`].
    ContextMenu,
    /// [`PromptRequest::JsDialog`].
    JsDialog,
    /// [`PromptRequest::BeforeUnload`].
    BeforeUnload,
    /// [`PromptRequest::FileChooser`].
    FileChooser,
    /// [`PromptRequest::Auth`].
    Auth,
    /// [`PromptRequest::Permission`].
    Permission,
}

impl PromptKind {
    /// Every kind.
    pub const ALL: [PromptKind; 6] = [
        PromptKind::ContextMenu,
        PromptKind::JsDialog,
        PromptKind::BeforeUnload,
        PromptKind::FileChooser,
        PromptKind::Auth,
        PromptKind::Permission,
    ];
}

/// Why the engine ended a prompt before an answer. Arrives as `BrowserEvent::PromptEnded`; any later
/// answer gets `PromptError::AlreadyEnded`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PromptEnd {
    /// The tab's main frame committed a new document.
    Navigated,
    /// The browser closed, or its tab was detached.
    TabClosed,
    /// The engine withdrew a permission prompt.
    Withdrawn,
    /// The engine reset its dialog state.
    EngineReset,
    /// The menu was dismissed in the engine.
    MenuDismissed,
    /// Shutdown began.
    Shutdown,
}

impl PromptEnd {
    /// Whether the engine already gave up the callback, so ending runs no cancel action (design
    /// section 1.5, Ends).
    pub fn engine_side(self) -> bool {
        matches!(self, PromptEnd::Withdrawn | PromptEnd::EngineReset | PromptEnd::MenuDismissed)
    }
}

/// Why `Engine::answer_prompt` was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PromptError {
    /// No prompt ever had this id.
    Unknown,
    /// The answer's kind is not the prompt's. The prompt stays open.
    WrongKind,
    /// The prompt was answered or ended already.
    AlreadyEnded,
}

/// The document a prompt belongs to: its browser, and that browser's document generation when the
/// engine asked. `B` is the engine's browser key (the glue's browser id, the fake's `TabId`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PromptScope<B> {
    /// The browser.
    pub browser: B,
    /// Its generation, raised when the main frame commits a new document.
    pub generation: u64,
}

/// A browser's state at admission.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ScopeState {
    /// Its current generation.
    pub generation: u64,
    /// Whether it is closing.
    pub closing: bool,
}

/// Why admission refused a prompt. The shell never sees it; the caller runs the cancel action.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Shutdown has begun.
    ShuttingDown,
    /// The browser is gone.
    BrowserGone,
    /// The browser is closing.
    BrowserClosing,
    /// The browser has committed a new document since the engine asked.
    NewDocument,
}

struct Open<B, C> {
    kind: PromptKind,
    browser: Option<B>,
    callback: C,
}

/// The open prompts and the protocol's rules: admission by scope, exactly one of answer or end, and
/// no admission after shutdown began. It holds the callbacks `C` and hands each back exactly once; it
/// never calls them, so the caller can release its own borrows before it does.
pub struct PromptLedger<B, C> {
    /// The last id issued; ids start at 1, so an id above it was never issued.
    last: u64,
    open: BTreeMap<PromptId, Open<B, C>>,
    shut: bool,
}

impl<B: Copy + PartialEq, C> Default for PromptLedger<B, C> {
    fn default() -> Self {
        PromptLedger { last: 0, open: BTreeMap::new(), shut: false }
    }
}

impl<B: Copy + PartialEq, C> PromptLedger<B, C> {
    /// An empty ledger that admits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Admits a prompt asked for in `scope` (`None` for a request that belongs to no browser), given
    /// the scope's browser's state now (`state` returns `None` when it is gone). A refusal hands the
    /// callback back for its cancel action.
    pub fn admit(
        &mut self,
        kind: PromptKind,
        scope: Option<PromptScope<B>>,
        state: impl FnOnce(B) -> Option<ScopeState>,
        callback: C,
    ) -> Result<PromptId, (Refused, C)> {
        if self.shut {
            return Err((Refused::ShuttingDown, callback));
        }
        if let Some(scope) = scope {
            match state(scope.browser) {
                None => return Err((Refused::BrowserGone, callback)),
                Some(now) if now.closing => return Err((Refused::BrowserClosing, callback)),
                Some(now) if now.generation != scope.generation => return Err((Refused::NewDocument, callback)),
                Some(_) => {}
            }
        }
        self.last += 1;
        let id = PromptId(self.last);
        self.open.insert(id, Open { kind, browser: scope.map(|s| s.browser), callback });
        Ok(id)
    }

    /// Takes the prompt out for `answer`, whose action the caller then runs on the returned callback. A
    /// wrong kind leaves the prompt open.
    pub fn answer(&mut self, id: PromptId, answer: &PromptAnswer) -> Result<C, PromptError> {
        match self.open.get(&id) {
            None => Err(self.missing(id)),
            Some(open) if open.kind != answer.kind() => Err(PromptError::WrongKind),
            Some(_) => Ok(self.open.remove(&id).expect("looked up above").callback),
        }
    }

    /// Takes the prompt out for an end.
    pub fn end(&mut self, id: PromptId) -> Result<C, PromptError> {
        self.open.remove(&id).map(|open| open.callback).ok_or_else(|| self.missing(id))
    }

    /// Takes out every prompt of `browser`, for `Navigated` or `TabClosed`.
    pub fn end_browser(&mut self, browser: B) -> Vec<(PromptId, C)> {
        self.take_where(|open| open.browser == Some(browser))
    }

    /// Stops admission and takes out every open prompt, for `Shutdown`. Idempotent.
    pub fn shut(&mut self) -> Vec<(PromptId, C)> {
        self.shut = true;
        self.take_where(|_| true)
    }

    /// Whether `id` is open.
    pub fn is_open(&self, id: PromptId) -> bool {
        self.open.contains_key(&id)
    }

    /// The open prompts' ids, oldest first.
    pub fn open_ids(&self) -> Vec<PromptId> {
        self.open.keys().copied().collect()
    }

    fn missing(&self, id: PromptId) -> PromptError {
        if id.0 >= 1 && id.0 <= self.last { PromptError::AlreadyEnded } else { PromptError::Unknown }
    }

    fn take_where(&mut self, take: impl Fn(&Open<B, C>) -> bool) -> Vec<(PromptId, C)> {
        let ids: Vec<PromptId> = self.open.iter().filter(|(_, open)| take(open)).map(|(id, _)| *id).collect();
        ids.into_iter().map(|id| (id, self.open.remove(&id).expect("listed above").callback)).collect()
    }
}
