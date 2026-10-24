//! A tab's permission requests waiting for the user: the queue behind the prompt.
//!
//! [`Queue`] holds the requests the engine made and the shell has not answered, oldest first,
//! with the tab's one-time grants ([`TabGrants`]). It never touches the engine: the request is a
//! handle `R` the shell chose (WebView2's deferral, CEF's callback), and every method that
//! settles requests returns them as [`Answered`] for the shell to complete, after it let go of
//! the profile. What it decides it decides from `Profile`'s stored settings.
//!
//! - **Groups.** Requests of the same origin that follow each other (camera and microphone from
//!   one `getUserMedia` arrive as two) join the last group, even one already shown, so they get
//!   one prompt and one answer.
//! - **At most [`MAX_WAITING`]** requests wait per tab; a page that asks for more is denied.
//! - **Per document.** What the user turned down (dismissed, or "never allow") is not asked
//!   again by the same document; a new document may ask again.
//! - **Every request gets an answer**, now or later: when the tab closes, or commits a document
//!   of another origin, what waits is denied.
//!
//! The prompt shows the first group, and is its [`Queue::answer`]. The pure rules of the shell's
//! prompt are here too: how soon after it shows its buttons count ([`INPUT_GUARD`]), when a
//! click elsewhere dismisses it or only withdraws it ([`closing`]), and when an allow still
//! holds ([`holds`]).

use std::collections::VecDeque;
use std::time::Duration;

use super::{Answer, Decision, Origin, Permission, Prompt, TabGrants, prompt};
use crate::private::Browsing;
use crate::{Profile, Url};

/// Requests a tab keeps waiting for the user at most.
pub const MAX_WAITING: usize = 8;

/// Answers this soon after a prompt shows or changes were aimed at something else (Chrome's
/// input guard).
pub const INPUT_GUARD: Duration = Duration::from_millis(500);

/// A click that closes the prompt may be the press of a tab switch or of the site-info button;
/// both take the prompt before this passes.
pub const CLOSE_GRACE: Duration = Duration::from_millis(250);

/// One engine request, held open until it is answered.
#[derive(Debug)]
pub struct Entry<R> {
    pub request: R,
    /// What it asks for, usually one permission; camera and microphone may come as one.
    pub permissions: Vec<Permission>,
}

/// A request and the answer it gets.
#[derive(Debug)]
pub struct Answered<R> {
    pub request: R,
    pub permissions: Vec<Permission>,
    pub allowed: bool,
}

impl<R> Answered<R> {
    fn new(entry: Entry<R>, allowed: bool) -> Self {
        Answered { request: entry.request, permissions: entry.permissions, allowed }
    }
}

/// What became of a request just made.
#[derive(Debug)]
pub enum Requested<R> {
    /// Answered from the settings and grants, or because the tab has too many waiting.
    Answered(Answered<R>),
    /// Waiting for the tab's prompt.
    Waiting,
}

/// Requests an operation settled.
#[derive(Debug)]
pub struct Settled<R> {
    pub answers: Vec<Answered<R>>,
    /// Whether the shown prompt changed: it went, or it asks for less.
    pub prompt_changed: bool,
}

/// The prompt the first waiting group needs.
#[derive(Debug)]
pub struct NextPrompt<R> {
    /// `None` when nothing waits.
    pub prompt: Option<Prompt>,
    /// Whether it differs from the one last shown.
    pub changed: bool,
    /// Requests the settings decided by now, which were answered on the way.
    pub answers: Vec<Answered<R>>,
}

/// The requests of one origin that one prompt answers.
struct Group<R> {
    origin: Option<Origin>,
    entries: Vec<Entry<R>>,
    /// What the shown prompt asks for; `None` while it is not shown.
    asked: Option<Vec<Permission>>,
}

impl<R> Group<R> {
    fn permissions(&self) -> Vec<Permission> {
        self.entries.iter().flat_map(|e| e.permissions.iter().copied()).collect()
    }
}

/// One tab's one-time grants and waiting requests.
pub struct Queue<R> {
    /// The tab's kind, whose settings decide its requests.
    browsing: Browsing,
    grants: TabGrants,
    waiting: VecDeque<Group<R>>,
    /// What the user turned down in the document on screen.
    dismissed: Vec<Permission>,
}

impl<R> Queue<R> {
    pub fn new(browsing: Browsing) -> Self {
        Queue { browsing, grants: TabGrants::default(), waiting: VecDeque::new(), dismissed: Vec::new() }
    }

    /// The permissions the tab's page was granted for this visit.
    pub fn grants(&self) -> Vec<Permission> {
        self.grants.granted().collect()
    }

    /// Takes back a one-time grant.
    pub fn revoke(&mut self, permission: Permission) {
        self.grants.revoke(permission);
    }

    /// How many requests wait.
    pub fn len(&self) -> usize {
        self.waiting.iter().map(|g| g.entries.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// Whether `permissions` for `origin` may go ahead as the settings and grants stand.
    pub fn decide(&self, p: &mut Profile, origin: Option<&Origin>, permissions: &[Permission]) -> Decision {
        p.site_permissions_in(self.browsing).decide(origin, permissions, &self.grants)
    }

    /// [`decide`](Self::decide) for a request of the page on screen, which is refused what the
    /// user turned down in this document rather than asked again.
    fn decide_request(&self, p: &mut Profile, origin: Option<&Origin>, permissions: &[Permission]) -> Decision {
        match self.decide(p, origin, permissions) {
            Decision::Ask(asked) if asked.iter().all(|a| self.dismissed.contains(a)) => Decision::Block,
            decision => decision,
        }
    }

    /// A request from `origin` for `permissions`: answered now, or queued.
    pub fn request(&mut self, p: &mut Profile, origin: Option<Origin>, permissions: Vec<Permission>, request: R) -> Requested<R> {
        let entry = Entry { request, permissions };
        let allowed = match self.decide_request(p, origin.as_ref(), &entry.permissions) {
            Decision::Allow => true,
            Decision::Block => false,
            Decision::Ask(_) if self.len() >= MAX_WAITING => false,
            Decision::Ask(_) => {
                match self.waiting.back_mut() {
                    Some(last) if last.origin == origin => {
                        last.entries.push(entry);
                        last.asked = None;
                    }
                    _ => self.waiting.push_back(Group { origin, entries: vec![entry], asked: None }),
                }
                return Requested::Waiting;
            }
        };
        Requested::Answered(Answered::new(entry, allowed))
    }

    /// The stored settings changed: waiting requests they now decide are answered.
    pub fn settle_decided(&mut self, p: &mut Profile) -> Settled<R> {
        let mut settled = Settled { answers: Vec::new(), prompt_changed: false };
        let mut kept = VecDeque::new();
        for mut group in std::mem::take(&mut self.waiting) {
            match self.decide_request(p, group.origin.as_ref(), &group.permissions()) {
                Decision::Ask(asked) => {
                    if group.asked.as_ref().is_some_and(|shown| *shown != asked) {
                        group.asked = None;
                        settled.prompt_changed = true;
                    }
                    kept.push_back(group);
                }
                Decision::Allow | Decision::Block => {
                    settled.prompt_changed |= group.asked.is_some();
                    settled.answers.extend(self.settle(p, group));
                }
            }
        }
        self.waiting = kept;
        settled
    }

    /// Answers each request of `group` from the settings and grants as they are now; what is
    /// still undecided is denied.
    fn settle(&self, p: &mut Profile, group: Group<R>) -> Vec<Answered<R>> {
        group
            .entries
            .into_iter()
            .map(|entry| {
                let allowed = self.decide(p, group.origin.as_ref(), &entry.permissions) == Decision::Allow;
                Answered::new(entry, allowed)
            })
            .collect()
    }

    /// The prompt for the first waiting group, marked shown. Groups the settings decide by now
    /// (the user answered the same site in another tab) are answered on the way.
    pub fn next_prompt(&mut self, p: &mut Profile) -> NextPrompt<R> {
        let mut answers = Vec::new();
        loop {
            let Some(head) = self.waiting.front() else {
                return NextPrompt { prompt: None, changed: false, answers };
            };
            if let Some(asked) = &head.asked {
                return NextPrompt { prompt: Some(prompt(head.origin.as_ref(), asked)), changed: false, answers };
            }
            match self.decide_request(p, head.origin.as_ref(), &head.permissions()) {
                Decision::Ask(asked) => {
                    let shown = prompt(head.origin.as_ref(), &asked);
                    if let Some(head) = self.waiting.front_mut() {
                        head.asked = Some(asked);
                    }
                    return NextPrompt { prompt: Some(shown), changed: true, answers };
                }
                Decision::Allow | Decision::Block => {
                    if let Some(head) = self.waiting.pop_front() {
                        answers.extend(self.settle(p, head));
                    }
                }
            }
        }
    }

    /// The shown prompt went away unanswered (its tab is no longer the one shown): it shows
    /// again from the start when its tab does.
    pub fn prompt_hidden(&mut self) {
        if let Some(head) = self.waiting.front_mut() {
            head.asked = None;
        }
    }

    /// Applies the user's answer to the shown prompt. A dismissal, or "never allow", also keeps
    /// the document from asking for the same again.
    pub fn answer(&mut self, p: &mut Profile, answer: Answer) -> Vec<Answered<R>> {
        let Some(head) = self.waiting.pop_front() else {
            return Vec::new();
        };
        let asked = head.asked.clone().unwrap_or_default();
        if matches!(answer, Answer::Dismiss | Answer::NeverAllow) {
            self.dismissed.extend_from_slice(&asked);
        }
        if let Err(e) = p.site_permissions_in(self.browsing).answer(head.origin.as_ref(), &asked, answer, &mut self.grants) {
            log::warn!("permission answer {answer:?}: {e}");
        }
        self.settle(p, head)
    }

    /// A committed navigation of the tab: grants end when it leaves their site, and a new
    /// document of another origin denies what the old one still waits for (and may ask again for
    /// what the last one was refused).
    pub fn committed(&mut self, url: &Url, new_document: bool) -> Settled<R> {
        self.grants.committed(url);
        let mut settled = Settled { answers: Vec::new(), prompt_changed: false };
        if !new_document {
            return settled;
        }
        self.dismissed.clear();
        let origin = Origin::of(url);
        let (gone, kept): (VecDeque<Group<R>>, VecDeque<Group<R>>) =
            std::mem::take(&mut self.waiting).into_iter().partition(|g| g.origin.is_none() || g.origin != origin);
        self.waiting = kept;
        settled.prompt_changed = gone.iter().any(|g| g.asked.is_some());
        settled.answers = gone.into_iter().flat_map(|g| g.entries).map(|e| Answered::new(e, false)).collect();
        settled
    }

    /// The tab closed: whatever waits is denied.
    pub fn close(&mut self) -> Vec<Answered<R>> {
        std::mem::take(&mut self.waiting).into_iter().flat_map(|g| g.entries).map(|e| Answered::new(e, false)).collect()
    }
}

/// Whether a button pressed `since_shown` after the prompt showed or changed counts.
pub fn guarded(since_shown: Duration) -> bool {
    since_shown < INPUT_GUARD
}

/// What the prompt closing without a button means.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Closing {
    /// Esc, "Not now" or a click elsewhere that left the prompt's tab selected.
    Dismiss,
    /// It went because of something else (its tab lost the selection, a click inside the
    /// input guard): the requests keep waiting and the prompt comes back.
    Withdraw,
}

/// What a close `since_shown` after the prompt showed means, when `tab_selected` says whether
/// its tab is still the one shown. As in Chrome, a request is never denied because the user did
/// something else.
pub fn closing(tab_selected: bool, since_shown: Duration) -> Closing {
    if tab_selected && !guarded(since_shown) { Closing::Dismiss } else { Closing::Withdraw }
}

/// An Allow counts only while the site the prompt named is still the one on screen.
pub fn holds(granted: bool, asked_by: Option<&str>, on_screen: Option<&str>) -> bool {
    granted && asked_by.is_some() && site_of(asked_by) == site_of(on_screen)
}

fn site_of(uri: Option<&str>) -> Option<(String, Option<String>, Option<u16>)> {
    let url = Url::parse(uri?).ok()?;
    Some((url.scheme().to_owned(), url.host_str().map(str::to_owned), url.port_or_known_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_count_once_the_guard_has_passed() {
        assert!(guarded(Duration::ZERO));
        assert!(guarded(INPUT_GUARD - Duration::from_millis(1)));
        assert!(!guarded(INPUT_GUARD));
    }

    #[test]
    fn a_close_dismisses_only_for_the_selected_tab_after_the_guard() {
        assert_eq!(closing(true, INPUT_GUARD), Closing::Dismiss);
        assert_eq!(closing(true, Duration::from_millis(10)), Closing::Withdraw);
        assert_eq!(closing(false, Duration::from_secs(5)), Closing::Withdraw);
    }

    #[test]
    fn an_allow_holds_for_the_site_that_was_asked_only() {
        let asked = Some("https://a.test/page");
        assert!(holds(true, asked, Some("https://a.test/other")));
        assert!(holds(true, asked, Some("https://a.test:443/")), "the default port is the same site");
        assert!(!holds(false, asked, asked));
        assert!(!holds(true, asked, Some("https://b.test/")));
        assert!(!holds(true, asked, Some("http://a.test/")));
        assert!(!holds(true, asked, Some("https://a.test:8443/")));
        assert!(!holds(true, asked, None));
        assert!(!holds(true, None, None), "no page asked");
    }
}
