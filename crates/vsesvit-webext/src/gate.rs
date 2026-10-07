//! The navigation policy a shell keeps for each tab view: which navigations WebKit may make in
//! it. As in Chrome, a web page reaches an extension's pages only where
//! `web_accessible_resources` lets it; the extension itself and the browser reach them all.
//! Only the browser opens a `view-source:` page, which no page reaches, not even a local one
//! that the scheme's own rules would let in.
//!
//! WebKitGTK does not say who started a navigation, so the gate judges one by the page that may
//! have: the document on screen, whose script still runs while the next one loads, or for a
//! window a page opened that has shown nothing yet, that page. Three things make that page an
//! untrusted judge, and the gate keeps track of each:
//!
//! - A load the browser starts (the address bar, a reload, history, a restored session,
//!   `tabs.update`) is not the page's. The shell says so with [`Gate::browser_load`] before it
//!   starts one, and WebKit's next decision for that target is allowed. A server redirect is
//!   never the browser's: only a web server redirects, so the web page on screen asked, or no
//!   page did.
//! - A window that a web page opened (`window.open`, a link's target) can be navigated by that
//!   page for as long as both live, by reference or by name. Every navigation there that the
//!   browser did not start must be one that page may make too.
//! - An extension page a web page reached through `web_accessible_resources` still speaks for
//!   that web page, which may hold its window and send it on. One whose way in is unknown and
//!   that some web page may reach speaks for no one, so it reaches no extension page.
//!
//! What is left: a web page in a window that an extension page opened can still navigate that
//! extension page through `window.opener`, as can a page that finds an extension page's window
//! by a name the extension gave it, and a web frame inside an extension page can navigate it or
//! open windows at its extension's pages, since WebKit asks that view either way. A web page's
//! own `history.back()` to an extension page that is not web-accessible is refused, and so is a
//! back or forward swipe there, which WebKit starts without telling the shell.

/// What the gate asks about extensions; `Runtime` answers on Linux.
pub trait Policy {
    /// `Runtime::may_navigate`: may the page at `source` navigate to `target`?
    fn may_navigate(&self, source: &str, target: &str) -> bool;
    /// Whether `url` is an extension page that some page outside the extension may load.
    fn web_reachable(&self, url: &str) -> bool;
}

/// One view's navigation policy. The shell asks [`Gate::decide`] from WebKit's
/// `decide-policy` (navigations and new windows) and `create`, and reports every committed
/// load with [`Gate::committed`].
#[derive(Clone, Debug, Default)]
pub struct Gate {
    /// For a view a page opened, the page every navigation the browser did not start must
    /// suit too (see [`Gate::opened_by`]).
    opener: Option<String>,
    /// The target of the load the browser started last, until WebKit asks about it.
    browser_load: Option<String>,
    /// The target of the navigation allowed last, and the page its document will speak for.
    pending: Option<(String, String)>,
    /// The page the document on screen speaks for; `None` before the first commit.
    page: Option<String>,
}

impl Gate {
    /// The gate of a view that the page in `opener`'s view opened. It is judged by that page
    /// until it commits, and every navigation the browser does not start must suit that page
    /// too, or the page that opened `opener`'s view, when one did.
    pub fn opened_by(opener: &Gate) -> Gate {
        Gate { opener: Some(opener.opener.clone().unwrap_or_else(|| opener.source())), ..Gate::default() }
    }

    /// The browser is about to load `target` in the view: WebKit's next decision to navigate
    /// there is allowed, and the page it commits speaks for itself.
    pub fn browser_load(&mut self, target: &str) {
        self.browser_load = Some(target.to_owned());
    }

    /// Whether `target` is the load the browser started last, which WebKit has not asked about.
    pub fn started_by_browser(&self, target: &str) -> bool {
        self.browser_load.as_deref().is_some_and(|load| same_url(load, target))
    }

    /// The browser stopped the view's load, so a load it started may never be asked about.
    pub fn stop(&mut self) {
        self.browser_load = None;
    }

    /// Whether the view may navigate to `target` (`new_window`: open a window there), which
    /// WebKit asks before it loads anything: a server redirect asks again (`redirect`).
    pub fn decide(&mut self, policy: &impl Policy, target: &str, redirect: bool, new_window: bool) -> bool {
        if !new_window && !redirect && self.started_by_browser(target) {
            self.browser_load = None;
            self.pending = Some((target.to_owned(), target.to_owned()));
            return true;
        }
        if is_view_source(target) {
            return false;
        }
        let mut source = self.source();
        if redirect && source.starts_with("chrome-extension:") {
            source.clear();
        }
        let allowed = policy.may_navigate(&source, target) && self.opener.as_deref().is_none_or(|opener| policy.may_navigate(opener, target));
        if allowed && !new_window {
            // Reached only through web_accessible_resources, it speaks for the page that came.
            let speaks_for = if crate::patterns::may_enter(&source, target) { target } else { &source };
            self.pending = Some((target.to_owned(), speaks_for.to_owned()));
        }
        allowed
    }

    /// The view committed a load of `uri`. A load the browser started that WebKit has not asked
    /// about by now never will be: another took its place.
    pub fn committed(&mut self, policy: &impl Policy, uri: &str) {
        self.browser_load = None;
        let page = match self.pending.take() {
            Some((target, speaks_for)) if same_url(&target, uri) => speaks_for,
            _ if policy.web_reachable(uri) => String::new(),
            _ => uri.to_owned(),
        };
        self.page = Some(page);
    }

    /// The page the view's document speaks for when it navigates or opens a window: itself,
    /// the web page that reached it, or before its first commit, the page that opened the
    /// view; empty for none.
    pub fn source(&self) -> String {
        self.page.clone().or_else(|| self.opener.clone()).unwrap_or_default()
    }
}

fn is_view_source(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| url.scheme() == "view-source")
}

/// The same URL, however each is spelled (`chrome-extension://id` and `chrome-extension://id/`).
fn same_url(a: &str, b: &str) -> bool {
    let parse = |url: &str| {
        let mut url = url::Url::parse(url).ok()?;
        if url.path().is_empty() {
            url.set_path("/");
        }
        Some(url)
    };
    a == b || matches!((parse(a), parse(b)), (Some(a), Some(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONS: &str = "chrome-extension://ext/options.html";
    const OTHER: &str = "chrome-extension://ext/other.html";
    const PUBLIC: &str = "chrome-extension://ext/public.html";
    const EVIL: &str = "https://evil.test/";

    /// One extension, `ext`, whose `public.html` every web page may load.
    struct Ext;

    impl Policy for Ext {
        fn may_navigate(&self, source: &str, target: &str) -> bool {
            crate::patterns::may_enter(source, target) || (self.web_reachable(target) && source.starts_with("https:"))
        }
        fn web_reachable(&self, url: &str) -> bool {
            url.starts_with(PUBLIC)
        }
    }

    /// A gate whose view shows `uri`, loaded by the browser.
    fn showing(uri: &str) -> Gate {
        let mut gate = Gate::default();
        gate.browser_load(uri);
        assert!(gate.decide(&Ext, uri, false, false));
        gate.committed(&Ext, uri);
        gate
    }

    #[test]
    fn the_browser_reaches_every_page_and_an_extension_page_it_opened_reaches_its_own() {
        let mut gate = showing(EVIL);
        gate.browser_load("chrome-extension://ext");
        assert!(gate.decide(&Ext, "chrome-extension://ext/", false, false), "however it is spelled");
        gate.committed(&Ext, "chrome-extension://ext/");
        assert!(gate.decide(&Ext, OPTIONS, false, false));
        let mut public = showing(PUBLIC);
        assert!(public.decide(&Ext, OPTIONS, false, false), "the browser opened it");
    }

    #[test]
    fn a_web_page_reaches_only_web_accessible_pages_and_no_redirect_takes_it_further() {
        let mut gate = showing(EVIL);
        assert!(!gate.decide(&Ext, OPTIONS, false, false));
        assert!(!gate.decide(&Ext, OPTIONS, false, true), "nor in a new window");
        assert!(gate.decide(&Ext, PUBLIC, false, false));
        // A server redirect, even of a load the browser started, is the web's.
        gate.browser_load("https://bounce.test/");
        assert!(gate.decide(&Ext, "https://bounce.test/", false, false));
        assert!(!gate.decide(&Ext, OPTIONS, true, false));
        let mut options = showing(OPTIONS);
        options.browser_load("https://bounce.test/");
        assert!(options.decide(&Ext, "https://bounce.test/", false, false));
        assert!(!options.decide(&Ext, OPTIONS, true, false), "the extension page does not redirect");
    }

    #[test]
    fn a_load_the_browser_started_and_then_stopped_is_no_longer_its() {
        let mut gate = showing(EVIL);
        gate.browser_load(OPTIONS);
        gate.stop();
        assert!(!gate.decide(&Ext, OPTIONS, false, false));
        gate.browser_load(OPTIONS);
        assert!(gate.decide(&Ext, OPTIONS, false, false));
        assert!(!gate.decide(&Ext, OPTIONS, false, false), "once only");
        // Nor one that another load replaced, as a reload of the page on screen does while
        // the address WebKit shows is still a refused one.
        let mut gate = showing(EVIL);
        gate.browser_load(OPTIONS);
        assert!(gate.decide(&Ext, EVIL, false, false));
        gate.committed(&Ext, EVIL);
        assert!(!gate.decide(&Ext, OPTIONS, false, false));
    }

    #[test]
    fn only_the_browser_opens_a_source() {
        let source = "view-source:file:///home/me/page.html";
        let mut gate = showing("file:///home/me/page.html");
        assert!(!gate.decide(&Ext, source, false, false));
        assert!(!gate.decide(&Ext, "VIEW-SOURCE:https://evil.test/", false, false), "however it is spelled");
        assert!(!gate.decide(&Ext, source, false, true), "nor in a new window");
        gate.browser_load(source);
        assert!(!gate.decide(&Ext, source, true, false), "nor through a redirect");
        assert!(gate.decide(&Ext, source, false, false));
        gate.committed(&Ext, source);
        assert!(!gate.decide(&Ext, "view-source:https://evil.test/", false, false), "a source page opens no other");
    }

    #[test]
    fn a_web_accessible_page_a_web_page_reached_speaks_for_that_page() {
        let mut gate = showing(EVIL);
        assert!(gate.decide(&Ext, PUBLIC, false, false));
        gate.committed(&Ext, PUBLIC);
        assert!(!gate.decide(&Ext, OPTIONS, false, false));
        assert!(gate.decide(&Ext, &format!("{PUBLIC}#more"), false, false), "what the web page may load itself");
        // One whose way in the gate did not see speaks for no one.
        let mut unseen = Gate::default();
        unseen.committed(&Ext, PUBLIC);
        assert!(!unseen.decide(&Ext, OPTIONS, false, false));
        assert!(!unseen.decide(&Ext, PUBLIC, false, false));
        assert!(unseen.decide(&Ext, EVIL, false, false));
        let mut options = Gate::default();
        options.committed(&Ext, OPTIONS);
        assert!(options.decide(&Ext, OTHER, false, false), "a page no web page reaches is its extension's");
    }

    #[test]
    fn a_window_a_web_page_opened_goes_only_where_that_page_may_send_it() {
        let opener = showing(EVIL);
        let mut held = Gate::opened_by(&opener);
        assert!(!held.decide(&Ext, OPTIONS, false, false));
        assert!(held.decide(&Ext, PUBLIC, false, false));
        held.committed(&Ext, PUBLIC);
        assert!(!held.decide(&Ext, &format!("{OPTIONS}?attack"), false, false));
        // The browser still loads what it likes there, but the page the window shows then
        // goes no further than its opener could send it.
        held.browser_load(OPTIONS);
        assert!(held.decide(&Ext, OPTIONS, false, false));
        held.committed(&Ext, OPTIONS);
        assert!(!held.decide(&Ext, OTHER, false, false));
        assert!(held.decide(&Ext, PUBLIC, false, false));
        // Nor the windows it opens in turn.
        let mut next = Gate::opened_by(&held);
        assert!(!next.decide(&Ext, OTHER, false, false));
        // A window an extension page opened is that page's.
        let mut own = Gate::opened_by(&showing(OPTIONS));
        assert!(own.decide(&Ext, OTHER, false, false));
        own.committed(&Ext, OTHER);
        assert!(own.decide(&Ext, OPTIONS, false, false));
    }
}
