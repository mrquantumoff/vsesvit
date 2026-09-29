//! Site permissions in the Windows shell, on core's model and words (`vsesvit_core::permissions`):
//! WebView2's permission requests decided by the stored settings and the tab's one-time grants,
//! the prompts a tab has waiting, and the rows the site-info popup lists.
//!
//! Every request core has a permission for gets `SavesInProfile(false)`: the engine never
//! remembers an answer and asks again each time, so core's settings stay the only record. Other
//! kinds keep the engine's default handling.
//!
//! Camera and microphone from one `getUserMedia` arrive as two requests, in two turns of the UI
//! thread. A request joins the last prompt of its tab when that is for the same origin, even one
//! already shown (which then asks for both), so they get one prompt and one answer.
//! A request waits (deferred) until its tab is the one shown, as in Chrome; the window shows one
//! prompt at a time. A waiting request is denied when its tab closes or commits a document of
//! another origin.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use vsesvit_core::Url;
use vsesvit_core::permissions::{
    Answer, Capturing, Decision, Origin, Permission, Prompt, Setting, TabGrants, prompt,
};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::browser::Browser;

/// How long a granted capture is watched for although nothing is live yet: the page may still
/// be opening the device.
const CAPTURE_GRACE: Duration = Duration::from_secs(10);

/// Core's permission for an engine request kind; `None` for kinds the engine keeps handling.
pub(crate) fn permission_of(kind: CoreWebView2PermissionKind) -> Option<Permission> {
    match kind {
        CoreWebView2PermissionKind::Camera => Some(Permission::Camera),
        CoreWebView2PermissionKind::Microphone => Some(Permission::Microphone),
        CoreWebView2PermissionKind::Geolocation => Some(Permission::Location),
        CoreWebView2PermissionKind::Notifications => Some(Permission::Notifications),
        CoreWebView2PermissionKind::ClipboardRead => Some(Permission::ClipboardRead),
        CoreWebView2PermissionKind::MidiSystemExclusiveMessages => Some(Permission::Midi),
        _ => None,
    }
}

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

    /// Whether the site may not share the screen. Otherwise the engine's screen picker, which
    /// is the prompt for it (as in Chrome), asks.
    pub fn blocks_screen_share(&self, browser: &Browser, origin: Option<&Origin>) -> bool {
        self.decide(browser, origin, &[Permission::ScreenShare]) == Decision::Block
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
        self.capturing.get().any() || self.watch_until.get().is_some_and(|t| Instant::now() < t)
    }
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
            if crate::capturing::uses(capturing, permission) {
                tab.stop_capture(permission);
            }
        }
    }
}

/// A site permission's state in the site-info popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    AllowedThisTime,
    Ask,
    Allow,
    Block,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::AllowedThisTime => "Allowed this time",
            Choice::Ask => "Ask",
            Choice::Allow => "Allow",
            Choice::Block => "Block",
        }
    }

    pub fn setting(self) -> Option<Setting> {
        match self {
            Choice::Allow => Some(Setting::Allow),
            Choice::Block => Some(Setting::Block),
            Choice::AllowedThisTime | Choice::Ask => None,
        }
    }
}

/// One row of the site-info popup's Permissions section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub permission: Permission,
    pub current: Choice,
    /// In the order the choice box lists them.
    pub choices: Vec<Choice>,
    /// The page captures what this permission governs: the row has a Stop button.
    pub live: bool,
}

/// A row for every permission the site has a setting for, was granted this time, or uses.
/// `storable`: the page has an origin, so Allow and Block can be remembered for it.
pub(crate) fn site_rows(
    storable: bool,
    stored: &[(Permission, Setting)],
    granted: &[Permission],
    capturing: Capturing,
) -> Vec<Row> {
    Permission::ALL
        .iter()
        .filter_map(|&permission| {
            let setting = stored
                .iter()
                .find(|(p, _)| *p == permission)
                .map(|(_, s)| *s);
            let granted = granted.contains(&permission);
            let live = crate::capturing::uses(capturing, permission);
            let current = match (setting, granted) {
                (Some(Setting::Allow), _) => Choice::Allow,
                (Some(Setting::Block), _) => Choice::Block,
                (None, true) => Choice::AllowedThisTime,
                (None, false) if live => Choice::Ask,
                (None, false) => return None,
            };
            let mut choices = Vec::new();
            if current == Choice::AllowedThisTime {
                choices.push(Choice::AllowedThisTime);
            }
            choices.push(Choice::Ask);
            if storable && permission.remembers_allow() {
                choices.push(Choice::Allow);
            }
            if storable {
                choices.push(Choice::Block);
            }
            Some(Row {
                permission,
                current,
                choices,
                live,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_list_what_is_stored_granted_or_in_use() {
        let stored = [
            (Permission::Location, Setting::Allow),
            (Permission::Notifications, Setting::Block),
        ];
        let capturing = Capturing {
            screen: true,
            ..Capturing::default()
        };
        let rows = site_rows(true, &stored, &[Permission::Camera], capturing);
        let summary: Vec<(Permission, Choice, bool)> = rows
            .iter()
            .map(|r| (r.permission, r.current, r.live))
            .collect();
        assert_eq!(
            summary,
            [
                (Permission::Camera, Choice::AllowedThisTime, false),
                (Permission::Location, Choice::Allow, false),
                (Permission::Notifications, Choice::Block, false),
                (Permission::ScreenShare, Choice::Ask, true),
            ]
        );
        assert_eq!(
            rows[0].choices,
            [
                Choice::AllowedThisTime,
                Choice::Ask,
                Choice::Allow,
                Choice::Block
            ]
        );
        assert_eq!(rows[1].choices, [Choice::Ask, Choice::Allow, Choice::Block]);
        assert_eq!(rows[3].choices, [Choice::Ask, Choice::Block]);
        assert!(site_rows(true, &[], &[], Capturing::default()).is_empty());
    }

    #[test]
    fn a_page_without_an_origin_offers_nothing_to_remember() {
        let rows = site_rows(false, &[], &[Permission::Location], Capturing::default());
        assert_eq!(rows[0].choices, [Choice::AllowedThisTime, Choice::Ask]);
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
}
