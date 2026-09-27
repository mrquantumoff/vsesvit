//! One browser tab: a WebKit web view plus what the shell tracks about it.
//!
//! A tab reports changes to whichever window currently holds it (tabs can be dragged between
//! windows), looked up through the widget tree at the time of the change. Every tab has two
//! identities: the runtime's [`TabId`], which `chrome.tabs` sees, and a session id that
//! keys its saved back/forward state in the profile.

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::history::Transition;
use vsesvit_core::session::TabId as SessionTabId;
use vsesvit_webext::TabId;
use webkit::prelude::*;

use crate::address_bar::Security;
use crate::browser::Browser;
use crate::error_page;
use crate::permissions;
use crate::window::{BrowserWindow, Focus};

/// Something about a tab the window may need to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabChange {
    Title,
    Favicon,
    Loading,
    Progress,
    Committed(Commit),
    History,
    Zoom,
    Find(FindResult),
}

/// What a committed main-frame navigation was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Commit {
    /// A new document from the network or disk.
    Document,
    /// A fragment or History API navigation within the current document.
    SameDocument,
    /// One of our own error pages, shown under the URI that failed.
    ErrorPage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindResult {
    Matches(u32),
    NotFound,
}

/// Which of our error pages a tab is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorPage {
    Certificate,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LoadPhase {
    #[default]
    Idle,
    Provisional,
    Committed,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Tab {
        pub(super) web_view: OnceCell<webkit::WebView>,
        pub(super) id: OnceCell<TabId>,
        pub(super) session_id: Cell<SessionTabId>,
        pub(super) last_active_ms: Cell<i64>,
        /// How the next committed navigation reached this tab, when the shell knows
        /// (typed in the address bar, chosen from bookmarks); otherwise it is a link.
        pub(super) pending_transition: Cell<Option<Transition>>,
        pub(super) link_preview: gtk::Label,
        pub(super) load: Cell<LoadPhase>,
        pub(super) committed_uri: RefCell<Option<String>>,
        pub(super) error_page_pending: Cell<Option<ErrorPage>>,
        pub(super) error_page_shown: Cell<Option<ErrorPage>>,
        pub(super) typed: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Tab {
        const NAME: &'static str = "VsesvitTab";
        type Type = super::Tab;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for Tab {}
    impl WidgetImpl for Tab {}
    impl BinImpl for Tab {}
}

glib::wrapper! {
    pub struct Tab(ObjectSubclass<imp::Tab>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Tab {
    /// A tab whose view carries the extension runtime's content for its id.
    pub(crate) fn new(browser: &Browser) -> Self {
        let id = browser.allocate_tab_id();
        let content = browser.runtime().user_content_manager(id);
        Self::wrap(browser.engine().web_view(&content), id)
    }

    pub(crate) fn new_related(browser: &Browser, opener: &Tab) -> Self {
        let id = browser.allocate_tab_id();
        let content = browser.runtime().user_content_manager(id);
        Self::wrap(
            browser.engine().related_web_view(opener.web_view(), &content),
            id,
        )
    }

    fn wrap(web_view: webkit::WebView, id: TabId) -> Self {
        let tab: Self = glib::Object::new();
        let imp = tab.imp();
        imp.id.set(id).expect("wrap runs once");
        web_view.set_hexpand(true);
        web_view.set_vexpand(true);

        let preview = &imp.link_preview;
        preview.set_halign(gtk::Align::Start);
        preview.set_valign(gtk::Align::End);
        preview.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        preview.set_max_width_chars(90);
        preview.set_can_target(false);
        preview.set_visible(false);
        preview.add_css_class("link-preview");

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&web_view));
        overlay.add_overlay(preview);
        tab.set_child(Some(&overlay));

        imp.web_view.set(web_view).expect("wrap runs once");
        tab.connect_web_view();
        tab
    }

    pub(crate) fn web_view(&self) -> &webkit::WebView {
        self.imp().web_view.get().expect("set in Tab::wrap")
    }

    /// The id the extension runtime knows this tab by.
    pub(crate) fn id(&self) -> TabId {
        *self.imp().id.get().expect("set in Tab::wrap")
    }

    /// The key of this tab's saved state in the profile's session.
    pub(crate) fn session_id(&self) -> SessionTabId {
        self.imp().session_id.get()
    }

    pub(crate) fn set_session_id(&self, id: SessionTabId) {
        self.imp().session_id.set(id);
    }

    pub(crate) fn last_active_ms(&self) -> i64 {
        self.imp().last_active_ms.get()
    }

    pub(crate) fn mark_active(&self, now_ms: i64) {
        self.imp().last_active_ms.set(now_ms);
    }

    pub(crate) fn set_pending_transition(&self, transition: Transition) {
        self.imp().pending_transition.set(Some(transition));
    }

    pub(crate) fn take_pending_transition(&self) -> Option<Transition> {
        self.imp().pending_transition.take()
    }

    /// The window holding this tab, if it is in one.
    pub(crate) fn window(&self) -> Option<BrowserWindow> {
        self.root().and_downcast()
    }

    pub(crate) fn load(&self, uri: &str) {
        self.web_view().load_uri(uri);
    }

    /// The URI of the document on screen, as opposed to one still being requested.
    pub(crate) fn committed_uri(&self) -> Option<String> {
        self.imp().committed_uri.borrow().clone()
    }

    /// What the address bar should say about the connection. An error page is never "secure",
    /// even under an `https` URI, and a certificate error page is flagged as insecure.
    pub(crate) fn security(&self) -> Security {
        match self.imp().error_page_shown.get() {
            Some(ErrorPage::Certificate) => Security::Insecure,
            Some(ErrorPage::Other) => Security::NotApplicable,
            None => Security::of(self.committed_uri().as_deref()),
        }
    }

    pub(crate) fn display_title(&self) -> String {
        if let Some(title) = self.web_view().title().filter(|t| !t.trim().is_empty()) {
            return title.into();
        }
        match self.committed_uri() {
            Some(uri) if uri != "about:blank" => display_uri(&uri),
            _ => "New Tab".to_owned(),
        }
    }

    /// Nothing requested or shown yet: a new tab waiting for an address.
    pub(crate) fn is_blank(&self) -> bool {
        self.web_view()
            .uri()
            .is_none_or(|uri| uri.is_empty() || uri == "about:blank")
    }

    /// Address-bar text typed in this tab but not submitted, kept while another tab is shown.
    pub(crate) fn set_typed(&self, text: Option<String>) {
        self.imp().typed.replace(text);
    }

    pub(crate) fn take_typed(&self) -> Option<String> {
        self.imp().typed.take()
    }

    /// The engine's opaque back/forward state, for the session store.
    pub(crate) fn session_state_bytes(&self) -> Option<Vec<u8>> {
        self.web_view()
            .session_state()?
            .serialize()
            .map(|bytes| bytes.to_vec())
    }

    /// Restores a tab from the profile's session: its saved back/forward state when there
    /// is one, else its URI.
    pub(crate) fn restore_saved(&self, state: Option<&[u8]>, uri: &str) {
        let state = state.map(|bytes| webkit::WebViewSessionState::new(&glib::Bytes::from(bytes)));
        self.restore(state.as_ref(), uri);
    }

    /// Restores a closed tab: its back/forward history when the engine gave us one, else its URI.
    pub(crate) fn restore(&self, state: Option<&webkit::WebViewSessionState>, uri: &str) {
        let web_view = self.web_view();
        if let Some(state) = state {
            web_view.restore_session_state(state);
            if let Some(item) = web_view
                .back_forward_list()
                .and_then(|list| list.current_item())
            {
                web_view.go_to_back_forward_list_item(&item);
                return;
            }
        }
        web_view.load_uri(uri);
    }

    fn notify(&self, change: TabChange) {
        if let Some(window) = self.window() {
            window.tab_changed(self, change);
        }
    }

    fn connect_web_view(&self) {
        let web_view = self.web_view();
        let notify_on = |change: TabChange| {
            glib::clone!(
                #[weak(rename_to = tab)]
                self,
                move |_: &webkit::WebView| tab.notify(change)
            )
        };
        web_view.connect_title_notify(notify_on(TabChange::Title));
        web_view.connect_favicon_notify(notify_on(TabChange::Favicon));
        web_view.connect_is_loading_notify(notify_on(TabChange::Loading));
        web_view.connect_estimated_load_progress_notify(notify_on(TabChange::Progress));
        web_view.connect_zoom_level_notify(notify_on(TabChange::Zoom));

        web_view.connect_load_changed(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            move |_, event| tab.load_changed(event)
        ));
        web_view.connect_uri_notify(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            move |_| tab.check_same_document_commit()
        ));
        if let Some(list) = web_view.back_forward_list() {
            list.connect_local(
                "changed",
                false,
                glib::clone!(
                    #[weak(rename_to = tab)]
                    self,
                    #[upgrade_or]
                    None,
                    move |_| {
                        tab.check_same_document_commit();
                        tab.notify(TabChange::History);
                        None
                    }
                ),
            );
        }

        web_view.connect_load_failed(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, event, uri, error| tab.load_failed(event, uri, error)
        ));
        web_view.connect_load_failed_with_tls_errors(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, uri, _certificate, errors| {
                tab.show_error_page(
                    uri,
                    &error_page::tls_error(uri, errors),
                    ErrorPage::Certificate,
                );
                true
            }
        ));
        web_view.connect_web_process_terminated(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            move |web_view, reason| {
                if reason == webkit::WebProcessTerminationReason::TerminatedByApi {
                    return;
                }
                let uri = web_view.uri().map(String::from).unwrap_or_default();
                log::warn!("the web process showing {uri} ended: {reason:?}");
                tab.show_error_page(&uri, &error_page::crashed(&uri), ErrorPage::Other);
            }
        ));

        web_view.connect_decide_policy(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, decision, kind| tab.decide_policy(decision, kind)
        ));
        web_view.connect_create(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            None,
            move |_, _action| tab.create_related()
        ));
        web_view.connect_close(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            move |_| {
                if let Some(window) = tab.window() {
                    window.close_tab(&tab);
                }
            }
        ));
        web_view.connect_permission_request(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, request| permissions::handle(&tab, request)
        ));
        web_view.connect_mouse_target_changed(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            move |_, hit, _modifiers| {
                let link = hit.context_is_link().then(|| hit.link_uri()).flatten();
                let preview = &tab.imp().link_preview;
                if let Some(link) = link {
                    preview.set_label(&display_uri(&link));
                }
                preview.set_visible(hit.context_is_link());
            }
        ));

        if let Some(find) = web_view.find_controller() {
            find.connect_found_text(glib::clone!(
                #[weak(rename_to = tab)]
                self,
                move |_, count| tab.notify(TabChange::Find(FindResult::Matches(count)))
            ));
            find.connect_counted_matches(glib::clone!(
                #[weak(rename_to = tab)]
                self,
                move |_, count| tab.notify(TabChange::Find(FindResult::Matches(count)))
            ));
            find.connect_failed_to_find_text(glib::clone!(
                #[weak(rename_to = tab)]
                self,
                move |_| tab.notify(TabChange::Find(FindResult::NotFound))
            ));
        }
    }

    fn load_changed(&self, event: webkit::LoadEvent) {
        let imp = self.imp();
        match event {
            webkit::LoadEvent::Started | webkit::LoadEvent::Redirected => {
                imp.load.set(LoadPhase::Provisional);
            }
            webkit::LoadEvent::Committed => {
                imp.load.set(LoadPhase::Committed);
                let error_page = imp.error_page_pending.take();
                imp.error_page_shown.set(error_page);
                let commit = if error_page.is_some() {
                    Commit::ErrorPage
                } else {
                    Commit::Document
                };
                imp.committed_uri
                    .replace(self.web_view().uri().map(String::from));
                self.notify(TabChange::Committed(commit));
            }
            webkit::LoadEvent::Finished => imp.load.set(LoadPhase::Idle),
            _ => {}
        }
    }

    /// WebKit emits no load events for fragment and History API navigations. The URI changing
    /// while no load is in flight, to the back/forward list's current entry, is that commit.
    /// Both signals involved call this, because their order is not specified.
    fn check_same_document_commit(&self) {
        let imp = self.imp();
        let web_view = self.web_view();
        if imp.load.get() == LoadPhase::Provisional || web_view.is_loading() {
            return;
        }
        let Some(uri) = web_view.uri().map(String::from) else {
            return;
        };
        let current = web_view
            .back_forward_list()
            .and_then(|l| l.current_item())
            .and_then(|i| i.uri());
        if current.as_deref() != Some(uri.as_str())
            || imp.committed_uri.borrow().as_deref() == Some(&uri)
        {
            return;
        }
        imp.committed_uri.replace(Some(uri));
        self.notify(TabChange::Committed(Commit::SameDocument));
    }

    fn load_failed(&self, event: webkit::LoadEvent, uri: &str, error: &glib::Error) -> bool {
        // An error page that itself fails to load (for example under a port WebKit refuses)
        // is not replaced by another one, which would fail the same way.
        let error_page_failed = self.imp().error_page_pending.take().is_some();
        let benign = error.matches(webkit::NetworkError::Cancelled)
            || error.matches(webkit::PolicyError::FrameLoadInterruptedByPolicyChange)
            || error.matches(webkit::MediaError::Load);
        if benign || error_page_failed || event != webkit::LoadEvent::Started {
            return false;
        }
        self.show_error_page(
            uri,
            &error_page::load_failed(uri, error.message()),
            ErrorPage::Other,
        );
        true
    }

    fn show_error_page(&self, uri: &str, html: &str, kind: ErrorPage) {
        self.imp().error_page_pending.set(Some(kind));
        self.web_view().load_alternate_html(html, uri, None);
    }

    fn decide_policy(
        &self,
        decision: &webkit::PolicyDecision,
        kind: webkit::PolicyDecisionType,
    ) -> bool {
        match kind {
            webkit::PolicyDecisionType::NavigationAction
            | webkit::PolicyDecisionType::NewWindowAction => {
                let Some(action) = decision
                    .downcast_ref::<webkit::NavigationPolicyDecision>()
                    .and_then(|d| d.navigation_action())
                else {
                    return false;
                };
                let modifiers = gdk::ModifierType::from_bits_truncate(action.modifiers());
                let Some(focus) =
                    new_tab_for_click(action.navigation_type(), action.mouse_button(), modifiers)
                else {
                    return false;
                };
                let (Some(uri), Some(window)) =
                    (action.request().and_then(|r| r.uri()), self.window())
                else {
                    return false;
                };
                window.open_tab(Some(&uri), Some(self), focus);
                decision.ignore();
                true
            }
            webkit::PolicyDecisionType::Response => {
                let Some(response) = decision.downcast_ref::<webkit::ResponsePolicyDecision>()
                else {
                    return false;
                };
                if response.is_main_frame_main_resource() && !response.is_mime_type_supported() {
                    decision.download();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// `window.open` and `target=_blank`: WebKit wants the new view now and shows it after
    /// `ready-to-show`, so the window keeps it aside until then.
    fn create_related(&self) -> Option<gtk::Widget> {
        let window = self.window()?;
        let popup = Tab::new_related(window.browser(), self);
        window.adopt_popup(self, &popup);
        Some(popup.web_view().clone().upcast())
    }
}

/// Middle click, or Ctrl+click, on a link opens it in a new tab; Shift also switches to it.
fn new_tab_for_click(
    kind: webkit::NavigationType,
    button: u32,
    modifiers: gdk::ModifierType,
) -> Option<Focus> {
    let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
    let new_tab = kind == webkit::NavigationType::LinkClicked
        && (button == gdk::BUTTON_MIDDLE || (button == gdk::BUTTON_PRIMARY && ctrl));
    new_tab.then(|| {
        if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
            Focus::Foreground
        } else {
            Focus::Background
        }
    })
}

/// How a URI reads in the address bar and in titles: `about:blank` is empty, and punycode and
/// percent-escapes are decoded for display.
pub(crate) fn display_uri(uri: &str) -> String {
    if uri == "about:blank" {
        return String::new();
    }
    webkit::functions::uri_for_display(uri).map_or_else(|| uri.to_owned(), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::ModifierType as M;
    use webkit::NavigationType::{LinkClicked, Other};

    #[test]
    fn plain_clicks_stay_in_the_tab() {
        assert_eq!(
            new_tab_for_click(LinkClicked, gdk::BUTTON_PRIMARY, M::empty()),
            None
        );
        assert_eq!(
            new_tab_for_click(LinkClicked, gdk::BUTTON_PRIMARY, M::SHIFT_MASK),
            None
        );
    }

    #[test]
    fn middle_and_ctrl_clicks_open_background_tabs() {
        assert_eq!(
            new_tab_for_click(LinkClicked, gdk::BUTTON_MIDDLE, M::empty()),
            Some(Focus::Background)
        );
        assert_eq!(
            new_tab_for_click(LinkClicked, gdk::BUTTON_PRIMARY, M::CONTROL_MASK),
            Some(Focus::Background)
        );
        assert_eq!(
            new_tab_for_click(
                LinkClicked,
                gdk::BUTTON_PRIMARY,
                M::CONTROL_MASK | M::SHIFT_MASK
            ),
            Some(Focus::Foreground)
        );
    }

    #[test]
    fn only_link_clicks_count() {
        assert_eq!(
            new_tab_for_click(Other, gdk::BUTTON_MIDDLE, M::empty()),
            None
        );
    }
}
