//! Site permissions in the Windows shell, on core's model and words (`vsesvit_core::permissions`):
//! WebView2's permission requests decided by the stored settings and the tab's one-time grants,
//! and the prompts a tab has waiting.
//!
//! Every request core has a permission for gets `SavesInProfile(false)`: the engine never
//! remembers an answer by itself, so core's settings stay the only record. Pages still read the
//! engine's per-origin state (`Notification.permission`, `navigator.permissions.query`), so
//! [`settings_changed`] copies the stored settings into it, at startup and after every change.
//! Other kinds keep the engine's default handling.
//!
//! A stored Allow lets the engine grant a capture without asking, so a tab on a site that may
//! capture is watched for as long as it stays there (see `capturing`). What the page reports
//! is only its word, so a block or a reset, made here or arriving through sync, that takes back
//! a capture the shell let it make reloads the page ([`reload_allowed_captures`]).
//!
//! Camera and microphone from one `getUserMedia` arrive as two requests, in two turns of the UI
//! thread. A request joins the last prompt of its tab when that is for the same origin, even one
//! already shown (which then asks for both), so they get one prompt and one answer.
//! A request waits (deferred) until its tab is the one shown, as in Chrome; the window shows one
//! prompt at a time. A waiting request is denied when its tab closes or commits a document of
//! another origin.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use vsesvit_core::Url;
use vsesvit_core::permissions::{
    Answer, Capturing, Decision, Origin, Permission, Prompt, Setting, SiteSetting, TabGrants,
    prompt,
};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::browser::Browser;
use crate::exec;
use crate::tab::Tab;

/// The engine's permission kinds that core has a permission for.
const ENGINE_KINDS: [(CoreWebView2PermissionKind, Permission); 6] = [
    (CoreWebView2PermissionKind::Camera, Permission::Camera),
    (
        CoreWebView2PermissionKind::Microphone,
        Permission::Microphone,
    ),
    (
        CoreWebView2PermissionKind::Geolocation,
        Permission::Location,
    ),
    (
        CoreWebView2PermissionKind::Notifications,
        Permission::Notifications,
    ),
    (
        CoreWebView2PermissionKind::ClipboardRead,
        Permission::ClipboardRead,
    ),
    (
        CoreWebView2PermissionKind::MidiSystemExclusiveMessages,
        Permission::Midi,
    ),
];

/// The captures a stored setting can stop.
const CAPTURES: [Permission; 3] = [
    Permission::Camera,
    Permission::Microphone,
    Permission::ScreenShare,
];

/// How long a granted capture is watched for although nothing is live yet: the page may still
/// be opening the device.
const CAPTURE_GRACE: Duration = Duration::from_secs(10);

/// Core's permission for an engine request kind; `None` for kinds the engine keeps handling.
pub(crate) fn permission_of(kind: CoreWebView2PermissionKind) -> Option<Permission> {
    ENGINE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, p)| *p)
}

fn kind_of(permission: Permission) -> Option<CoreWebView2PermissionKind> {
    ENGINE_KINDS
        .iter()
        .find(|(_, p)| *p == permission)
        .map(|(k, _)| *k)
}

/// Picture-in-picture's, in Segoe Fluent Icons (MiniExpand); the toolbar's button has it too.
pub(crate) const PIP_GLYPH: &str = "\u{E93A}";

/// Segoe Fluent Icons.
pub(crate) fn glyph(permission: Permission) -> &'static str {
    match permission {
        Permission::Camera => "\u{E714}",
        Permission::Microphone => "\u{E720}",
        Permission::Location => "\u{E707}",
        Permission::Notifications => "\u{EA8F}",
        Permission::ScreenShare => "\u{E7F4}",
        Permission::ClipboardRead => "\u{E77F}",
        Permission::Midi => "\u{EC4F}",
        Permission::PictureInPicture => PIP_GLYPH,
        Permission::Trackers => "\u{EA18}",
        Permission::Http => "\u{E785}",
        Permission::Cookies => "\u{E7C3}",
    }
}

fn is_capture(permission: Permission) -> bool {
    matches!(permission, Permission::Camera | Permission::Microphone)
}

/// One engine request, held open until it is answered.
struct Request {
    args: CoreWebView2PermissionRequestedEventArgs,
    deferral: Deferral,
    permission: Permission,
}

impl Request {
    fn complete(self, allowed: bool) {
        let state = if allowed {
            CoreWebView2PermissionState::Allow
        } else {
            CoreWebView2PermissionState::Deny
        };
        if let Err(e) = self.args.SetState(state) {
            log::warn!("permission {:?}: {e}", self.permission);
        }
        let _ = self.deferral.Complete();
    }
}

/// The requests of one origin that one prompt answers.
struct Waiting {
    origin: Option<Origin>,
    requests: Vec<Request>,
    /// What the shown prompt asks for; `None` while it is not shown.
    asked: Option<Vec<Permission>>,
}

/// One tab's permissions: its one-time grants, the requests waiting for a prompt, and what its
/// page captures (see `capturing`).
#[derive(Default)]
pub(crate) struct TabPermissions {
    grants: RefCell<TabGrants>,
    waiting: RefCell<VecDeque<Waiting>>,
    capturing: Cell<Capturing>,
    /// A capture was granted: watch the page until then even while nothing is live.
    watch_until: Cell<Option<Instant>>,
    /// The page's site may use the camera or microphone without asking.
    site_may_capture: Cell<bool>,
}

/// What became of an engine request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Requested {
    /// Not a permission core has, or answered from the settings and grants.
    Settled,
    /// Waiting for the tab's prompt.
    Waiting,
}

impl TabPermissions {
    pub fn request(
        &self,
        browser: &Browser,
        args: &CoreWebView2PermissionRequestedEventArgs,
    ) -> Result<Requested> {
        let Some(permission) = permission_of(args.PermissionKind()?) else {
            return Ok(Requested::Settled);
        };
        args.cast::<ICoreWebView2PermissionRequestedEventArgs3>()?
            .SetSavesInProfile(false)?;
        let origin = Origin::parse(&args.Uri()?);
        let request = Request {
            args: args.clone(),
            deferral: args.GetDeferral()?,
            permission,
        };
        match self.decide(browser, origin.as_ref(), &[permission]) {
            Decision::Allow => self.complete(request, true),
            Decision::Block => self.complete(request, false),
            Decision::Ask(_) => {
                let mut waiting = self.waiting.borrow_mut();
                match waiting.back_mut() {
                    Some(last) if last.origin == origin => {
                        last.requests.push(request);
                        last.asked = None;
                    }
                    _ => waiting.push_back(Waiting {
                        origin,
                        requests: vec![request],
                        asked: None,
                    }),
                }
                return Ok(Requested::Waiting);
            }
        }
        Ok(Requested::Settled)
    }

    /// Whether the settings block `permission` for the site. For a screen share, when they do
    /// not, the engine's screen picker is the prompt (as in Chrome).
    pub fn blocks(
        &self,
        browser: &Browser,
        origin: Option<&Origin>,
        permission: Permission,
    ) -> bool {
        self.decide(browser, origin, &[permission]) == Decision::Block
    }

    /// The stored settings changed: waiting requests they now decide are answered. Whether the
    /// shown prompt changed (it went, or asks for less).
    pub fn settle_decided(&self, browser: &Browser) -> bool {
        let mut changed = false;
        let mut kept = VecDeque::new();
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for mut w in waiting {
            let permissions: Vec<Permission> = w.requests.iter().map(|r| r.permission).collect();
            match self.decide(browser, w.origin.as_ref(), &permissions) {
                Decision::Ask(asked) => {
                    if w.asked.as_ref().is_some_and(|shown| *shown != asked) {
                        w.asked = None;
                        changed = true;
                    }
                    kept.push_back(w);
                }
                Decision::Allow | Decision::Block => {
                    changed |= w.asked.is_some();
                    self.settle(browser, w);
                }
            }
        }
        *self.waiting.borrow_mut() = kept;
        changed
    }

    /// Re-reads whether the page at `origin` may capture without asking, after a navigation or
    /// a change of the settings.
    pub fn refresh_site(&self, browser: &Browser, origin: Option<&Origin>) {
        let allowed = |p| self.decide(browser, origin, &[p]) == Decision::Allow;
        self.site_may_capture
            .set(allowed(Permission::Camera) || allowed(Permission::Microphone));
    }

    fn decide(
        &self,
        browser: &Browser,
        origin: Option<&Origin>,
        permissions: &[Permission],
    ) -> Decision {
        let grants = self.grants.borrow();
        browser.core(|p| p.site_permissions().decide(origin, permissions, &grants))
    }

    fn complete(&self, request: Request, allowed: bool) {
        if allowed && is_capture(request.permission) {
            self.watch_until.set(Some(Instant::now() + CAPTURE_GRACE));
        }
        request.complete(allowed);
    }

    /// Answers each request from the settings and grants as they are now; what is still
    /// undecided is denied.
    fn settle(&self, browser: &Browser, waiting: Waiting) {
        for request in waiting.requests {
            let decision = self.decide(browser, waiting.origin.as_ref(), &[request.permission]);
            self.complete(request, decision == Decision::Allow);
        }
    }

    /// The prompt for the first waiting requests, marked shown, and whether it changed since it
    /// was last shown. Requests that the settings decide by now (the user answered the same
    /// site in another tab) are answered on the way.
    pub fn next_prompt(&self, browser: &Browser) -> Option<(Prompt, bool)> {
        loop {
            let (origin, permissions) = {
                let waiting = self.waiting.borrow();
                let head = waiting.front()?;
                if let Some(asked) = &head.asked {
                    return Some((prompt(head.origin.as_ref(), asked), false));
                }
                let permissions: Vec<Permission> =
                    head.requests.iter().map(|r| r.permission).collect();
                (head.origin.clone(), permissions)
            };
            match self.decide(browser, origin.as_ref(), &permissions) {
                Decision::Ask(asked) => {
                    let shown = prompt(origin.as_ref(), &asked);
                    if let Some(head) = self.waiting.borrow_mut().front_mut() {
                        head.asked = Some(asked);
                    }
                    return Some((shown, true));
                }
                Decision::Allow | Decision::Block => {
                    if let Some(head) = self.waiting.borrow_mut().pop_front() {
                        self.settle(browser, head);
                    }
                }
            }
        }
    }

    /// The shown prompt went away unanswered (its tab is no longer the one shown).
    pub fn prompt_hidden(&self) {
        if let Some(head) = self.waiting.borrow_mut().front_mut() {
            head.asked = None;
        }
    }

    /// Applies the user's answer to the shown prompt.
    pub fn answer(&self, browser: &Browser, answer: Answer) {
        let Some(head) = self.waiting.borrow_mut().pop_front() else {
            return;
        };
        let asked = head.asked.clone().unwrap_or_default();
        let answered = {
            let mut grants = self.grants.borrow_mut();
            browser.core(|p| {
                p.site_permissions()
                    .answer(head.origin.as_ref(), &asked, answer, &mut grants)
            })
        };
        if let Err(e) = answered {
            log::warn!("permission answer {answer:?}: {e}");
        }
        self.settle(browser, head);
    }

    /// A committed navigation of the tab: grants end when it leaves their site, and a new
    /// document of another origin denies what the old one still waits for. Whether the shown
    /// prompt went with it.
    pub fn committed(&self, url: &str, new_document: bool) -> bool {
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        self.grants.borrow_mut().committed(&url);
        if !new_document {
            return false;
        }
        let origin = Origin::of(&url);
        let gone: VecDeque<Waiting> = {
            let mut waiting = self.waiting.borrow_mut();
            let (gone, kept) = std::mem::take(&mut *waiting)
                .into_iter()
                .partition(|w| w.origin.is_none() || w.origin != origin);
            *waiting = kept;
            gone
        };
        let shown = gone.iter().any(|w| w.asked.is_some());
        for request in gone.into_iter().flat_map(|w| w.requests) {
            request.complete(false);
        }
        shown
    }

    /// The tab closed: whatever waits is denied.
    pub fn close(&self) {
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for request in waiting.into_iter().flat_map(|w| w.requests) {
            request.complete(false);
        }
    }

    pub fn grants(&self) -> Vec<Permission> {
        self.grants.borrow().granted().collect()
    }

    pub fn revoke(&self, permission: Permission) {
        self.grants.borrow_mut().revoke(permission);
    }

    pub fn capturing(&self) -> Capturing {
        self.capturing.get()
    }

    /// Records what the page captures; whether that changed.
    pub fn set_capturing(&self, capturing: Capturing) -> bool {
        self.capturing.replace(capturing) != capturing
    }

    /// A screen capture is starting, or a capture was granted.
    pub fn watch(&self) {
        self.watch_until.set(Some(Instant::now() + CAPTURE_GRACE));
    }

    /// Whether the page's capture still needs watching.
    pub fn watched(&self) -> bool {
        self.capturing.get().any()
            || self.site_may_capture.get()
            || self.watch_until.get().is_some_and(|t| Instant::now() < t)
    }
}

/// The stored settings changed: a prompt answer, the site-info popup or the Settings page.
/// Every tab's waiting requests that they now decide are answered, captures they now block
/// stop, the engine's copy follows, and so do the tabs' cookie controls.
pub(crate) fn settings_changed(browser: &Rc<Browser>) {
    for window in browser.windows() {
        for tab in window.tabs_in_order() {
            let permissions = tab.permissions();
            let origin = tab.origin();
            if permissions.settle_decided(browser) {
                window.show_next_prompt();
            }
            permissions.refresh_site(browser, origin.as_ref());
            let capturing = permissions.capturing();
            for p in CAPTURES {
                if capturing.uses(p) && permissions.blocks(browser, origin.as_ref(), p) {
                    tab.stop_capture(p);
                }
            }
            tab.watch_capture();
        }
        // Picture-in-picture follows its site setting.
        window.show_media();
    }
    mirror(browser);
    crate::cookies::changed(browser);
}

/// The engine's per-origin permission state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineState {
    Default,
    Allow,
    Deny,
}

impl EngineState {
    fn of(setting: Setting) -> Self {
        match setting {
            Setting::Allow | Setting::ClearOnExit => EngineState::Allow,
            Setting::Block => EngineState::Deny,
        }
    }

    fn engine(self) -> CoreWebView2PermissionState {
        match self {
            EngineState::Default => CoreWebView2PermissionState::Default,
            EngineState::Allow => CoreWebView2PermissionState::Allow,
            EngineState::Deny => CoreWebView2PermissionState::Deny,
        }
    }
}

/// What to set in the engine so that it holds exactly the stored settings of the permissions it
/// has a kind for; `current` is what it holds now (only what is not Default).
fn engine_changes(
    stored: &[SiteSetting],
    current: &[(Permission, Origin, EngineState)],
) -> Vec<(Permission, Origin, EngineState)> {
    let wanted: Vec<(Permission, Origin, EngineState)> = stored
        .iter()
        .filter(|s| kind_of(s.permission).is_some())
        .map(|s| (s.permission, s.origin.clone(), EngineState::of(s.setting)))
        .collect();
    let set = wanted.iter().filter(|w| !current.contains(w)).cloned();
    let cleared = current
        .iter()
        .filter(|(p, o, _)| !wanted.iter().any(|(wp, wo, _)| wp == p && wo == o))
        .map(|(p, o, _)| (*p, o.clone(), EngineState::Default));
    set.chain(cleared).collect()
}

/// Runs of [`mirror`] one after another: a change during a run brings one more.
#[derive(Default)]
pub(crate) struct EngineMirror {
    running: Cell<bool>,
    again: Cell<bool>,
}

/// Brings the engine's permission state in line with the stored settings.
pub(crate) fn mirror(browser: &Rc<Browser>) {
    let state = &browser.site_mirror;
    if state.running.replace(true) {
        state.again.set(true);
        return;
    }
    let browser = browser.clone();
    exec::spawn(async move {
        loop {
            browser.site_mirror.again.set(false);
            if let Err(e) = mirror_once(&browser).await {
                log::warn!("engine permissions: {e}");
            }
            if !browser.site_mirror.again.get() {
                break;
            }
        }
        browser.site_mirror.running.set(false);
    });
}

async fn mirror_once(browser: &Browser) -> Result<()> {
    let profile = browser
        .engine_profile()
        .await
        .ok_or_else(|| windows_core::Error::new(E_FAIL, "no engine profile"))?;
    let listed = profile
        .cast::<CoreWebView2Profile_Manual2>()?
        .GetNonDefaultPermissionSettingsAsync()?
        .await?;
    let mut current = Vec::new();
    for setting in &listed {
        let (Some(p), Some(origin)) = (
            permission_of(setting.PermissionKind()?),
            Origin::parse(&setting.PermissionOrigin()?),
        ) else {
            continue;
        };
        let state = match setting.PermissionState()? {
            CoreWebView2PermissionState::Allow => EngineState::Allow,
            CoreWebView2PermissionState::Deny => EngineState::Deny,
            _ => continue,
        };
        current.push((p, origin, state));
    }
    let stored = browser.core(|p| p.site_permissions().all());
    let changes = engine_changes(&stored, &current);
    let profile = profile.cast::<ICoreWebView2Profile4>()?;
    for (p, origin, state) in &changes {
        let Some(kind) = kind_of(*p) else { continue };
        profile
            .SetPermissionStateAsync(kind, origin.as_str(), state.engine())?
            .await?;
    }
    if !changes.is_empty() {
        log::info!("engine permissions: {} change(s)", changes.len());
    }
    Ok(())
}

/// Ends what the site captures under `permissions`, in every tab showing it: a block, or a
/// reset, while in use.
pub(crate) fn stop_captures(browser: &Browser, origin: &Origin, permissions: &[Permission]) {
    for tab in browser.windows().iter().flat_map(|w| w.tabs_in_order()) {
        if tab.origin().as_ref() != Some(origin) {
            continue;
        }
        let capturing = tab.permissions().capturing();
        for &permission in permissions {
            if capturing.uses(permission) {
                tab.stop_capture(permission);
            }
        }
    }
}

/// Whether blocking or resetting `permission` must reload a tab to end its capture: the shell
/// let the page capture (`allowed`: a grant, or the site's Allow), whatever the page reports.
/// The page can hide a capture from its script or keep the script from stopping it, so only
/// tearing down the document is sure to release the device.
pub(crate) fn must_reload(allowed: bool, permission: Permission) -> bool {
    crate::capturing::stop_script(permission).is_some() && allowed
}

/// After a block or a reset of `permissions` for `origin`: reloads every tab showing the site,
/// in every window, that the shell let capture under one of them (`allowed_before`), so that
/// the capture ends (see [`must_reload`]).
pub(crate) fn reload_allowed_captures(
    browser: &Browser,
    origin: &Origin,
    permissions: &[Permission],
    allowed_before: impl Fn(&Tab, Permission) -> bool,
) {
    for tab in browser.windows().iter().flat_map(|w| w.tabs_in_order()) {
        if tab.origin().as_ref() == Some(origin)
            && permissions
                .iter()
                .any(|&p| must_reload(allowed_before(&tab, p), p))
        {
            tab.reload();
        }
    }
}

/// One stored setting a change moved: the site, the permission, and its setting before and
/// after (`None`: Ask).
type Moved = (Origin, Permission, Option<Setting>, Option<Setting>);

/// Each setting that differs between `before` and `after`, both lists of every stored setting
/// (`SitePermissions::all`).
fn moved_settings(before: &[SiteSetting], after: &[SiteSetting]) -> Vec<Moved> {
    let setting_in = |list: &[SiteSetting], s: &SiteSetting| {
        list.iter()
            .find(|o| o.origin == s.origin && o.permission == s.permission)
            .map(|o| o.setting)
    };
    let changed = before.iter().filter_map(|s| {
        let now = setting_in(after, s);
        (now != Some(s.setting)).then(|| (s.origin.clone(), s.permission, Some(s.setting), now))
    });
    let added = after
        .iter()
        .filter(|s| setting_in(before, s).is_none())
        .map(|s| (s.origin.clone(), s.permission, None, Some(s.setting)));
    changed.chain(added).collect()
}

/// After a sync changed the stored settings from `before`: reloads the tabs whose capture a
/// block or a removal it brought takes back, as the same change on the Settings page does.
pub(crate) fn sync_changed(browser: &Browser, before: &[SiteSetting]) {
    let after = browser.core(|p| p.site_permissions().all());
    for (origin, permission, was, now) in moved_settings(before, &after) {
        reload_taken_back(browser, &origin, permission, was, now);
    }
}

/// After `permission` of `origin` went from `before` to `after`: reloads the tabs of the site
/// the shell let capture under it, when a block or a removal takes that back (see
/// [`must_reload`]). A removal leaves the tabs' grants as they are; a block overrides them.
pub(crate) fn reload_taken_back(
    browser: &Browser,
    origin: &Origin,
    permission: Permission,
    before: Option<Setting>,
    after: Option<Setting>,
) {
    if after == Some(Setting::Allow) {
        return;
    }
    reload_allowed_captures(browser, origin, &[permission], |tab, p| {
        before == Some(Setting::Allow)
            || (after == Some(Setting::Block) && tab.permissions().grants().contains(&p))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sync_moves_the_settings_it_added_removed_or_changed_and_no_others() {
        let a = Origin::parse("https://a.test").unwrap();
        let b = Origin::parse("https://b.test").unwrap();
        let stored = |origin: &Origin, permission, setting| SiteSetting {
            origin: origin.clone(),
            permission,
            setting,
        };
        let moved = moved_settings(
            &[
                stored(&a, Permission::Camera, Setting::Allow),
                stored(&a, Permission::Microphone, Setting::Allow),
                stored(&b, Permission::Camera, Setting::Block),
                stored(&b, Permission::Notifications, Setting::Allow),
            ],
            &[
                stored(&a, Permission::Camera, Setting::Block),
                stored(&b, Permission::Camera, Setting::Block),
                stored(&b, Permission::Microphone, Setting::Block),
                stored(&b, Permission::Notifications, Setting::Allow),
            ],
        );
        assert_eq!(
            moved,
            [
                (
                    a.clone(),
                    Permission::Camera,
                    Some(Setting::Allow),
                    Some(Setting::Block)
                ),
                (a, Permission::Microphone, Some(Setting::Allow), None),
                (b, Permission::Microphone, None, Some(Setting::Block)),
            ]
        );
    }

    #[test]
    fn the_engine_gets_exactly_the_stored_settings() {
        let a = Origin::parse("https://a.test").unwrap();
        let b = Origin::parse("https://b.test").unwrap();
        let stored = |origin: &Origin, permission, setting| SiteSetting {
            origin: origin.clone(),
            permission,
            setting,
        };
        let changes = engine_changes(
            &[
                stored(&a, Permission::Notifications, Setting::Allow),
                stored(&a, Permission::Camera, Setting::Block),
                stored(&a, Permission::ScreenShare, Setting::Block),
            ],
            &[
                (Permission::Camera, a.clone(), EngineState::Deny),
                (Permission::Location, b.clone(), EngineState::Allow),
            ],
        );
        assert_eq!(
            changes,
            [
                (Permission::Notifications, a, EngineState::Allow),
                (Permission::Location, b, EngineState::Default),
            ]
        );
    }

    #[test]
    fn engine_kinds_map_to_core_permissions() {
        assert_eq!(
            permission_of(CoreWebView2PermissionKind::Geolocation),
            Some(Permission::Location)
        );
        assert_eq!(
            permission_of(CoreWebView2PermissionKind::MidiSystemExclusiveMessages),
            Some(Permission::Midi)
        );
        assert_eq!(permission_of(CoreWebView2PermissionKind::Autoplay), None);
    }

    #[test]
    fn a_capture_the_shell_allowed_ends_whatever_the_page_reports() {
        assert!(must_reload(true, Permission::Camera));
        assert!(must_reload(true, Permission::Microphone));
        assert!(!must_reload(false, Permission::Camera));
        assert!(!must_reload(true, Permission::Notifications));
    }
}
