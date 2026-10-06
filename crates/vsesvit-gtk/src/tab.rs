//! One browser tab: a WebKit web view plus what the shell tracks about it.
//!
//! A tab reports changes to whichever window currently holds it (tabs can be dragged between
//! windows), looked up through the widget tree at the time of the change. Every tab has two
//! identities: the runtime's [`TabId`], which `chrome.tabs` sees, and a session id that
//! keys its saved back/forward state in the profile.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::history::Transition;
use vsesvit_core::https_only::{self, Cause, Next, Upgrades};
use vsesvit_core::permissions::{Capturing, Origin};
use vsesvit_core::session::TabId as SessionTabId;
use vsesvit_core::{Url, view_source};
use vsesvit_webext::{Gate, Runtime, TabId};
use webkit::prelude::*;

use crate::address_bar::Security;
use crate::browser::{self, Browser};
use crate::error_page;
use crate::page_menu;
use crate::permissions::{self, TabPermissions};
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
    /// Camera, microphone or screen capture started or stopped.
    Capture,
    /// The page started or stopped playing sound, or was muted or unmuted.
    Audio,
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
    /// HTTPS-only's warning that the site has no secure connection.
    HttpsOnly,
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
        pub(super) browser: OnceCell<Weak<browser::Inner>>,
        pub(super) runtime: OnceCell<Runtime>,
        /// Which navigations the view may make (see `vsesvit_webext::gate`).
        pub(super) gate: RefCell<Gate>,
        pub(super) session_id: Cell<SessionTabId>,
        pub(super) last_active_ms: Cell<i64>,
        /// When the tab was opened, last selected or its window activated, on the browser's clock
        /// (`Browser::tab_used`): tab search lists the most recently used tabs first.
        pub(super) used: Cell<u64>,
        /// How the next committed navigation reached this tab, when the shell knows
        /// (typed in the address bar, chosen from bookmarks); otherwise it is a link.
        pub(super) pending_transition: Cell<Option<Transition>>,
        pub(super) link_preview: gtk::Label,
        /// Over the page while it shares the screen.
        pub(super) sharing: adw::Banner,
        pub(super) permissions: RefCell<TabPermissions>,
        pub(super) load: Cell<LoadPhase>,
        pub(super) committed_uri: RefCell<Option<String>>,
        pub(super) error_page_pending: Cell<Option<ErrorPage>>,
        pub(super) error_page_shown: Cell<Option<ErrorPage>>,
        /// HTTPS-only's upgrades and warning page in the tab.
        pub(super) https: RefCell<Upgrades>,
        /// The target of the navigation HTTPS-only judged when WebKit asked about it, so its
        /// load is not judged again when it starts.
        pub(super) https_judged: RefCell<Option<String>>,
        /// The warning HTTPS-only shows once WebKit reports the load it stopped for it failed.
        pub(super) https_warning_due: RefCell<Option<Url>>,
        pub(super) typed: RefCell<Option<String>>,
        /// The text last selected in the page, for its context menu.
        pub(super) selection: RefCell<String>,
        /// Every target whose navigation this view refused, for tests to wait on.
        #[cfg(test)]
        pub(super) refused: RefCell<Vec<String>>,
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
    /// A tab whose view carries the extension runtime's content for its id, tracking
    /// protection's blocker and the cookie rules.
    pub(crate) fn new(browser: &Browser) -> Self {
        let id = browser.allocate_tab_id();
        let content = browser.runtime().user_content_manager(id);
        browser.trackers().attach(&content);
        browser.cookies().attach(&content);
        Self::wrap(browser.engine().web_view(&content), id, browser)
    }

    pub(crate) fn new_related(browser: &Browser, opener: &Tab) -> Self {
        let id = browser.allocate_tab_id();
        let content = browser.runtime().user_content_manager(id);
        browser.trackers().attach(&content);
        browser.cookies().attach(&content);
        let popup = Self::wrap(
            browser.engine().related_web_view(opener.web_view(), &content),
            id,
            browser,
        );
        let gate = Gate::opened_by(&opener.imp().gate.borrow());
        popup.imp().gate.replace(gate);
        popup
    }

    fn wrap(web_view: webkit::WebView, id: TabId, browser: &Browser) -> Self {
        let tab: Self = glib::Object::new();
        let imp = tab.imp();
        imp.id.set(id).expect("wrap runs once");
        imp.browser.set(Rc::downgrade(&browser.0)).expect("wrap runs once");
        assert!(imp.runtime.set(browser.runtime().clone()).is_ok(), "wrap runs once");
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
        let sharing = &imp.sharing;
        sharing.set_button_label(Some("Stop sharing"));
        sharing.connect_button_clicked(glib::clone!(
            #[weak]
            web_view,
            move |_| web_view.set_display_capture_state(webkit::MediaCaptureState::None)
        ));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(sharing);
        content.append(&overlay);
        tab.set_child(Some(&content));

        imp.web_view.set(web_view).expect("wrap runs once");
        tab.connect_web_view();
        page_menu::attach(&tab);
        browser.tab_used(&tab);
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

    pub(crate) fn used(&self) -> u64 {
        self.imp().used.get()
    }

    pub(crate) fn set_used(&self, used: u64) {
        self.imp().used.set(used);
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

    /// Its one-time grants and the permission requests waiting for the user.
    pub(crate) fn permissions(&self) -> &RefCell<TabPermissions> {
        &self.imp().permissions
    }

    /// What the page captures right now.
    pub(crate) fn capturing(&self) -> Capturing {
        permissions::capturing(self.web_view())
    }

    fn capture_changed(&self) {
        let sharing = &self.imp().sharing;
        let screen = self.capturing().screen;
        if screen {
            sharing.set_title(&permissions::sharing_title(self.committed_uri().as_deref()));
        }
        sharing.set_revealed(screen);
        self.notify(TabChange::Capture);
    }

    /// Loads `uri` for the browser (the address bar, a bookmark, `tabs.update`, ...), which
    /// may open any page.
    pub(crate) fn load(&self, uri: &str) {
        self.imp().gate.borrow_mut().browser_load(uri);
        self.web_view().load_uri(uri);
    }

    /// The browser's reload: the button, the key or the menu.
    pub(crate) fn reload(&self) {
        self.browser_load(self.reloaded_uri());
        self.web_view().reload();
    }

    pub(crate) fn reload_bypass_cache(&self) {
        self.browser_load(self.reloaded_uri());
        self.web_view().reload_bypass_cache();
    }

    /// What a reload loads: the document on screen, else the one still loading. WebKit may show
    /// the target of a navigation it refused as the view's URI meanwhile.
    fn reloaded_uri(&self) -> Option<String> {
        self.committed_uri().or_else(|| self.web_view().uri().map(String::from))
    }

    pub(crate) fn go_back(&self) {
        let list = self.web_view().back_forward_list();
        self.browser_load(list.and_then(|l| l.back_item()).and_then(|i| i.uri()));
        self.web_view().go_back();
    }

    pub(crate) fn go_forward(&self) {
        let list = self.web_view().back_forward_list();
        self.browser_load(list.and_then(|l| l.forward_item()).and_then(|i| i.uri()));
        self.web_view().go_forward();
    }

    fn browser_load(&self, target: Option<impl AsRef<str>>) {
        if let Some(target) = target {
            self.imp().gate.borrow_mut().browser_load(target.as_ref());
        }
    }

    /// Stops the load in flight. A stopped load commits nothing, so the transition set for
    /// it must not go to a later visit.
    pub(crate) fn stop(&self) {
        self.imp().pending_transition.take();
        self.imp().gate.borrow_mut().stop();
        self.web_view().stop_loading();
    }

    /// WebKit's print dialog for the page, over the tab's window.
    pub(crate) fn print(&self) {
        webkit::PrintOperation::new(self.web_view()).run_dialog(self.window().as_ref());
    }

    /// The `view-source:` address of the page on screen, when it has a source to show.
    pub(crate) fn source_url(&self) -> Option<String> {
        self.committed_uri().as_deref().and_then(view_source::source_url)
    }

    /// The page's source in a new tab next to this one.
    pub(crate) fn view_source(&self) {
        if let (Some(url), Some(window)) = (self.source_url(), self.window()) {
            window.open_tab(Some(&url), Some(self), Focus::Foreground);
        }
    }

    /// WebKit's inspector, closed when it is open: Chrome's F12.
    pub(crate) fn toggle_inspector(&self) {
        let Some(inspector) = self.web_view().inspector() else { return };
        if is_open(&inspector) {
            inspector.close();
        } else {
            inspector.show();
        }
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn inspector_open(&self) -> bool {
        self.web_view().inspector().is_some_and(|inspector| is_open(&inspector))
    }

    pub(crate) fn show_inspector(&self) {
        if let Some(inspector) = self.web_view().inspector() {
            inspector.show();
        }
    }

    /// The URI of the document on screen, as opposed to one still being requested.
    pub(crate) fn committed_uri(&self) -> Option<String> {
        self.imp().committed_uri.borrow().clone()
    }

    /// What the session saves for this tab: the committed URI, else the one still loading,
    /// as for a restored or background tab whose first page has not arrived yet.
    pub(crate) fn session_uri(&self) -> Option<String> {
        self.committed_uri().or_else(|| {
            let requested = self.web_view().uri()?;
            (!requested.is_empty() && requested != "about:blank").then(|| requested.into())
        })
    }

    /// Whether the document on screen is one of our error pages, shown under the URI that
    /// failed.
    pub(crate) fn shows_error_page(&self) -> bool {
        self.imp().error_page_shown.get().is_some()
    }

    /// What the address bar should say about the connection. An error page is never "secure",
    /// even under an `https` URI, and a certificate error page is flagged as insecure.
    pub(crate) fn security(&self) -> Security {
        match self.imp().error_page_shown.get() {
            Some(ErrorPage::Certificate | ErrorPage::HttpsOnly) => Security::Insecure,
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

    /// The address of the page on screen, for copying; `None` for a blank tab or the new tab
    /// page.
    pub(crate) fn link(&self) -> Option<String> {
        self.committed_uri().filter(|uri| uri != "about:blank")
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

    pub(crate) fn selection(&self) -> String {
        self.imp().selection.borrow().clone()
    }

    pub(crate) fn set_selection(&self, text: String) {
        self.imp().selection.replace(text);
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
        let decoded = state.and_then(decode_session_state);
        if state.is_some() && decoded.is_none() {
            log::info!("this WebKit cannot read the saved history of {uri}; loading the page alone");
        }
        self.restore(decoded.as_ref(), uri);
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
                self.browser_load(item.uri());
                web_view.go_to_back_forward_list_item(&item);
                return;
            }
        }
        self.load(uri);
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
        web_view.connect_is_playing_audio_notify(notify_on(TabChange::Audio));
        web_view.connect_is_muted_notify(notify_on(TabChange::Audio));
        let capture_changed = || {
            glib::clone!(
                #[weak(rename_to = tab)]
                self,
                move |_: &webkit::WebView| tab.capture_changed()
            )
        };
        web_view.connect_camera_capture_state_notify(capture_changed());
        web_view.connect_microphone_capture_state_notify(capture_changed());
        web_view.connect_display_capture_state_notify(capture_changed());

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
                if tab.https_failed() {
                    return true;
                }
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
        vsesvit_webext::connect_create(web_view, glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            None,
            move |_, action| {
                let target = action.request().and_then(|r| r.uri());
                if let Some(target) = target.filter(|target| !tab.may_navigate(action, target, true)) {
                    tab.refused(target.into());
                    return None;
                }
                tab.create_related()
            }
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
        web_view.connect_query_permission_state(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, query| permissions::query(&tab, query)
        ));
        web_view.connect_show_notification(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_, notification| {
                if permissions::notification_allowed(&tab) {
                    return false;
                }
                notification.close();
                true
            }
        ));
        web_view.connect_enter_fullscreen(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_| {
                if let Some(window) = tab.window() {
                    let site = tab.committed_uri().as_deref().and_then(Origin::parse);
                    let site = site.map_or_else(|| "This page".to_owned(), |o| o.host_for_display());
                    window.show_fullscreen_notice(&site);
                }
                // WebKit then makes the window full screen.
                false
            }
        ));
        web_view.connect_leave_fullscreen(glib::clone!(
            #[weak(rename_to = tab)]
            self,
            #[upgrade_or]
            false,
            move |_| {
                if let Some(window) = tab.window() {
                    window.hide_fullscreen_notice();
                }
                false
            }
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
                self.https_started(event == webkit::LoadEvent::Redirected);
            }
            webkit::LoadEvent::Committed => {
                imp.load.set(LoadPhase::Committed);
                imp.https.borrow_mut().committed();
                let uri = self.web_view().uri().map(String::from);
                imp.gate.borrow_mut().committed(self.runtime(), uri.as_deref().unwrap_or_default());
                let error_page = imp.error_page_pending.take();
                imp.error_page_shown.set(error_page);
                let commit = if error_page.is_some() {
                    Commit::ErrorPage
                } else {
                    Commit::Document
                };
                imp.committed_uri.replace(uri);
                permissions::committed(self);
                self.notify(TabChange::Committed(commit));
            }
            webkit::LoadEvent::Finished => {
                imp.load.set(LoadPhase::Idle);
                // A fragment or History API navigation made while the page still loaded
                // (images, say) was waiting for this.
                self.check_same_document_commit();
            }
            _ => {}
        }
    }

    /// WebKit emits no load events for fragment and History API navigations. The URI changing
    /// while no load is in flight, to the back/forward list's current entry, is that commit;
    /// one made during a load commits when the load finishes. Both signals involved call
    /// this, because their order is not specified.
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
        permissions::committed(self);
        self.notify(TabChange::Committed(Commit::SameDocument));
    }

    fn load_failed(&self, event: webkit::LoadEvent, uri: &str, error: &glib::Error) -> bool {
        // An error page that itself fails to load (for example under a port WebKit refuses)
        // is not replaced by another one, which would fail the same way.
        let error_page_failed = self.imp().error_page_pending.take().is_some();
        let benign = error.matches(webkit::NetworkError::Cancelled)
            || error.matches(webkit::PolicyError::FrameLoadInterruptedByPolicyChange)
            || error.matches(webkit::MediaError::Load);
        let warning_due = self.imp().https_warning_due.take();
        if let Some(url) = warning_due.filter(|_| benign) {
            self.show_https_warning(&url);
            return true;
        }
        if benign || error_page_failed || event != webkit::LoadEvent::Started {
            return false;
        }
        if self.https_failed() {
            return true;
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
                let target = action.request().and_then(|r| r.uri());
                let new_window = kind == webkit::PolicyDecisionType::NewWindowAction;
                let browser_load = target
                    .as_deref()
                    .is_some_and(|target| self.imp().gate.borrow().started_by_browser(target));
                if let Some(target) = target.as_deref().filter(|target| !self.may_navigate(&action, target, new_window)) {
                    self.refused(target.to_owned());
                    decision.ignore();
                    return true;
                }
                let modifiers = gdk::ModifierType::from_bits_truncate(action.modifiers());
                let Some(focus) =
                    new_tab_for_click(action.navigation_type(), action.mouse_button(), modifiers)
                else {
                    return !new_window
                        && target.is_some_and(|target| {
                            self.https_policy(decision, &action, &target, browser_load)
                        });
                };
                let (Some(uri), Some(window)) = (target, self.window()) else {
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
                    // The page stays, so the transition set for this load goes to no visit.
                    self.imp().pending_transition.take();
                    decision.download();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Whether a navigation of this view (`new_window`: a window it opens) may go to `target`:
    /// a web page reaches an extension's pages only where they are web-accessible to it, as in
    /// Chrome. The scheme handler cannot tell who asked for a load without a `Referer`, and
    /// WebKit does not say who started a navigation, so the view's gate keeps track of what
    /// it can (see `vsesvit_webext::gate`).
    fn may_navigate(&self, action: &webkit::NavigationAction, target: &str, new_window: bool) -> bool {
        let redirect = action.is_redirect();
        self.imp().gate.borrow_mut().decide(self.runtime(), target, redirect, new_window)
    }

    /// HTTPS-only's say on a navigation WebKit asks about where it is surely the main frame's,
    /// which WebKit doesn't tell: one the browser started (`browser_load`), or any while the
    /// document on screen can hold no http frame. The rest are judged as they start loading.
    fn https_policy(
        &self,
        decision: &webkit::PolicyDecision,
        action: &webkit::NavigationAction,
        target: &str,
        browser_load: bool,
    ) -> bool {
        if !browser_load && !self.holds_no_http_frame() {
            return false;
        }
        let cause = if action.is_redirect() {
            Cause::Redirect
        } else if !browser_load && action.navigation_type() == webkit::NavigationType::LinkClicked {
            Cause::Link
        } else {
            Cause::Other
        };
        self.imp().https_judged.replace(Some(target.to_owned()));
        self.https_next(target, cause, || decision.ignore())
    }

    /// Whether the document on screen can hold no http frame, so that a navigation WebKit asks
    /// about is the main frame's: an https page, whose http frames would be blocked mixed
    /// content, a blank page or the new tab page, or one of our error pages.
    fn holds_no_http_frame(&self) -> bool {
        self.shows_error_page()
            || self
                .committed_uri()
                .is_none_or(|uri| uri == "about:blank" || uri.starts_with("https:"))
    }

    /// HTTPS-only's say on a main-frame load that started without being judged when WebKit asked
    /// about it. Its http request may have gone out by now, but its page never shows.
    fn https_started(&self, redirect: bool) {
        let Some(uri) = self.web_view().uri() else { return };
        if self.imp().https_judged.take().as_deref() == Some(uri.as_str()) {
            return;
        }
        let cause = if redirect { Cause::Redirect } else { Cause::Other };
        self.https_next(&uri, cause, || self.web_view().stop_loading());
    }

    /// Feeds HTTPS-only a navigation to `uri` and does what it answers; `stop` stops the
    /// navigation. Whether it stopped it.
    fn https_next(&self, uri: &str, cause: Cause, stop: impl FnOnce()) -> bool {
        let (Some(browser), Ok(url)) = (self.browser(), Url::parse(uri)) else {
            return false;
        };
        let upgrade = browser.https_upgrade(&url);
        let next = self.imp().https.borrow_mut().starting(&url, cause, upgrade);
        match next {
            Next::Load => false,
            Next::Allow(url) => {
                log::info!("continuing to {url} without a secure connection");
                if let Err(e) = https_only::allow(&mut browser.core().borrow_mut(), &url) {
                    log::warn!("HTTPS-only exception for {url}: {e}");
                }
                false
            }
            Next::Upgrade(https) => {
                log::debug!("upgrading {uri} to {https}");
                stop();
                self.load(https.as_str());
                true
            }
            Next::Warn(http) => {
                // Shown in place of the stopped load once WebKit reports it failed, as the other
                // error pages are.
                self.imp().https_warning_due.replace(Some(http));
                stop();
                true
            }
        }
    }

    /// Shows HTTPS-only's warning in place of an upgraded load that failed. Whether there was
    /// one.
    fn https_failed(&self) -> bool {
        let warning = self.imp().https.borrow_mut().finished(false);
        warning.inspect(|url| self.show_https_warning(url)).is_some()
    }

    fn show_https_warning(&self, url: &Url) {
        log::info!("{url} has no secure connection");
        self.show_error_page(url.as_str(), &https_only::warning_page(url), ErrorPage::HttpsOnly);
    }

    fn browser(&self) -> Option<Browser> {
        self.imp().browser.get()?.upgrade().map(Browser)
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        self.imp().runtime.get().expect("set in Tab::wrap")
    }

    fn refused(&self, target: String) {
        log::debug!("refused a navigation to {target}");
        #[cfg(test)]
        self.imp().refused.borrow_mut().push(target);
    }

    #[cfg(test)]
    fn refused_targets(&self) -> Vec<String> {
        self.imp().refused.borrow().clone()
    }

    /// `window.open` and `target=_blank`: WebKit wants the new view now and shows it after
    /// `ready-to-show`, so the window keeps it aside until then.
    fn create_related(&self) -> Option<webkit::WebView> {
        let window = self.window()?;
        let popup = Tab::new_related(window.browser(), self);
        window.adopt_popup(self, &popup);
        Some(popup.web_view().clone())
    }
}

/// Saved back/forward state, or `None` when this WebKit cannot decode it: a format from
/// another WebKitGTK version, or a damaged row. The binding's `WebViewSessionState::new`
/// assumes a result, but WebKit returns NULL for such data.
fn decode_session_state(bytes: &[u8]) -> Option<webkit::WebViewSessionState> {
    use glib::translate::{FromGlibPtrFull, ToGlibPtr};
    let bytes = glib::Bytes::from(bytes);
    // SAFETY: `bytes` outlives the call, and a non-NULL result is a reference we own.
    unsafe {
        let state = webkit::ffi::webkit_web_view_session_state_new(bytes.to_glib_none().0);
        (!state.is_null()).then(|| webkit::WebViewSessionState::from_glib_full(state))
    }
}

/// Whether the inspector has a view, which it has from opening until it closes. The binding's
/// `WebInspector::web_view` would sink that view while it is still floating (before WebKit puts
/// it in a window or attaches it), and dropping the reference then destroys it, closing the
/// inspector.
fn is_open(inspector: &webkit::WebInspector) -> bool {
    use glib::translate::ToGlibPtr;
    // SAFETY: the pointer is borrowed and only compared with NULL.
    unsafe { !webkit::ffi::webkit_web_inspector_get_web_view(inspector.to_glib_none().0).is_null() }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::test_support::{Reply, Server, browser, scratch_dir, wait_until};
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

    #[gtk::test]
    fn saved_state_the_engine_cannot_read_falls_back_to_the_url() {
        let server = Server::start("127.0.0.1", |_| Reply::Page("Restored"));
        let window = BrowserWindow::new(&browser());
        let tab = window.open_tab(None, None, Focus::Background);
        let url = server.url("/");
        tab.restore_saved(Some(b"not a WebKit session state"), &url);
        wait_until("the saved URL to load", || {
            tab.committed_uri().as_deref() == Some(url.as_str())
        });
        window.destroy();
    }

    #[gtk::test]
    fn a_history_navigation_made_while_the_page_loads_commits_when_it_finishes() {
        let stalled = Arc::new(AtomicBool::new(true));
        let server = Server::start("127.0.0.1", {
            let stalled = stalled.clone();
            move |path| match path {
                "/" => Reply::Body(
                    "text/html",
                    b"<img src=/slow><script>history.pushState(null, '', '/moved')</script>".to_vec(),
                ),
                "/slow" => {
                    while stalled.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Reply::NotFound
                }
                _ => Reply::NotFound,
            }
        });
        let (first, moved) = (server.url("/"), server.url("/moved"));
        let window = BrowserWindow::new(&browser());
        let tab = window.open_tab(Some(&first), None, Focus::Background);
        wait_until("the page to move while it loads", || {
            tab.web_view().uri().as_deref() == Some(moved.as_str())
        });
        let loading = tab.web_view().is_loading();
        stalled.store(false, Ordering::SeqCst);
        wait_until("the load to finish", || !tab.web_view().is_loading());
        let committed = tab.committed_uri();
        window.destroy();
        assert!(loading, "the image held the load open");
        assert_eq!(committed.as_deref(), Some(moved.as_str()));
    }

    #[gtk::test]
    fn a_stopped_load_leaves_no_transition_behind() {
        let asked = Arc::new(AtomicBool::new(false));
        let server = Server::start("127.0.0.1", {
            let asked = asked.clone();
            move |_| {
                asked.store(true, Ordering::SeqCst);
                Reply::Hang
            }
        });
        let window = BrowserWindow::new(&browser());
        let tab = window.open_tab(None, None, Focus::Foreground);
        window.navigate_with(&server.url("/never"), Transition::Typed);
        wait_until("the request", || asked.load(Ordering::SeqCst));
        gtk::gio::prelude::ActionGroupExt::activate_action(&window, "stop", None);
        wait_until("the load to end", || !tab.web_view().is_loading());
        let left = tab.take_pending_transition();
        window.destroy();
        assert_eq!(left, None, "the next visit would count as typed");
    }

    #[gtk::test]
    fn a_download_leaves_no_transition_behind() {
        use vsesvit_core::prefs::keys;

        let server = Server::start("127.0.0.1", |_| Reply::StalledFile);
        let browser = browser();
        let dir = scratch_dir("typed-download");
        browser.core().borrow_mut().prefs().set(&keys::DOWNLOADS_DIR, &Some(dir.clone())).unwrap();
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(None, None, Focus::Foreground);
        window.navigate_with(&server.url("/file.bin"), Transition::Typed);
        let downloads = browser.downloads().clone();
        let ours = || downloads.list().into_iter().find(|d| d.path.parent() == Some(dir.as_path()));
        wait_until("the download", || ours().is_some());
        wait_until("the load to end", || !tab.web_view().is_loading());
        let left = tab.take_pending_transition();
        downloads.cancel(ours().expect("the download").id);
        window.destroy();
        browser.core().borrow_mut().prefs().reset(&keys::DOWNLOADS_DIR).unwrap();
        assert_eq!(left, None, "the next visit would count as typed");
    }

    #[gtk::test]
    fn an_address_typed_over_a_load_still_counts_as_typed() {
        let asked = Arc::new(AtomicBool::new(false));
        let server = Server::start("127.0.0.1", {
            let asked = asked.clone();
            move |path| match path {
                "/typed-over" => Reply::Page("Typed"),
                _ => {
                    asked.store(true, Ordering::SeqCst);
                    Reply::Hang
                }
            }
        });
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let tab = window.open_tab(Some(&server.url("/slow")), None, Focus::Foreground);
        wait_until("the first request", || asked.load(Ordering::SeqCst));
        let typed = server.url("/typed-over");
        window.navigate_with(&typed, Transition::Typed);
        wait_until("the typed page", || tab.committed_uri().as_deref() == Some(typed.as_str()));
        window.destroy();
        let found = browser.core().borrow_mut().history().search(&typed, 1).unwrap();
        assert_eq!(found.first().map(|e| e.typed_count), Some(1));
    }

    /// Installs an extension whose options page is not web-accessible and whose `public.html`
    /// is, to every site. Returns it with the URLs of both pages.
    fn install_private_options(browser: &Browser, name: &str) -> (vsesvit_core::extensions::InstalledExtension, String, String) {
        use vsesvit_core::extensions::InstallSource;

        let dir = scratch_dir(name);
        let manifest = r#"{ "manifest_version": 3, "name": "Private options", "version": "1.0",
            "options_page": "options.html",
            "web_accessible_resources": [{ "resources": ["public.html"], "matches": ["<all_urls>"] }] }"#;
        std::fs::write(dir.join("manifest.json"), manifest).unwrap();
        std::fs::write(dir.join("options.html"), "<!doctype html><title>Private options</title>").unwrap();
        std::fs::write(dir.join("public.html"), "<!doctype html><title>Public page</title>").unwrap();
        let source = InstallSource::from_path(&dir).unwrap();
        let installed = glib::MainContext::default().block_on(browser.install(source, |_| {})).unwrap().expect("installed");
        let base = format!("chrome-extension://{}/", installed.id.as_str());
        let (options, public) = (format!("{base}options.html"), format!("{base}public.html"));
        (installed, options, public)
    }

    #[gtk::test]
    fn a_web_page_cannot_open_an_extension_page_that_is_not_web_accessible() {
        let browser = browser();
        let (installed, options, public) = install_private_options(&browser, "private-options");

        // Every way a page can reach the options page without sending a Referer: window.open
        // and a target=_blank link with noreferrer, and a no-referrer navigation of its own
        // tab, which the user then reloads. The web-accessible page and a site still open.
        let server = Server::start("127.0.0.1", {
            let (options, public) = (options.clone(), public.clone());
            move |path| {
                let script = match path {
                    "/open" => format!(
                        "<a id=link href='{options}' target=_blank rel=noreferrer>x</a><script>
                        window.open('{options}', '_blank', 'noreferrer');
                        document.getElementById('link').click();
                        window.open('{public}', '_blank', 'noreferrer');
                        window.open('/opened', '_blank', 'noreferrer');</script>"
                    ),
                    "/lure" => format!(
                        "<meta name=referrer content=no-referrer><title>Lure</title>
                        <script>setTimeout(() => location.href = '{options}', 100)</script>"
                    ),
                    "/opened" => return Reply::Page("Opened"),
                    _ => return Reply::NotFound,
                };
                Reply::Body("text/html", script.into_bytes())
            }
        });
        let settings = browser.engine().settings().clone();
        let popups_allowed = settings.is_javascript_can_open_windows_automatically();
        settings.set_javascript_can_open_windows_automatically(true);
        let window = BrowserWindow::new(&browser);
        let titles = |window: &BrowserWindow| -> Vec<String> {
            window.tabs().iter().map(|t| t.web_view().title().map(String::from).unwrap_or_default()).collect()
        };

        window.open_tab(Some(&server.url("/open")), None, Focus::Foreground);
        wait_until("the site and the web-accessible page to open", || {
            let titles = titles(&window);
            titles.iter().any(|t| t == "Opened") && titles.iter().any(|t| t == "Public page")
        });
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        let opened = titles(&window);

        let lure = window.open_tab(Some(&server.url("/lure")), None, Focus::Foreground);
        wait_until("the lure", || lure.web_view().title().as_deref() == Some("Lure"));
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        lure.reload();
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        let lured = lure.web_view().title().map(String::from);

        // Nor through a site that redirects there with no Referer, even from the address bar.
        let bounce = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bounce_url = format!("http://{}/", bounce.local_addr().unwrap());
        std::thread::spawn({
            let options = options.clone();
            move || {
                for stream in bounce.incoming().flatten() {
                    use std::io::{BufRead, Write};
                    let mut reader = std::io::BufReader::new(&stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                        line.clear();
                    }
                    let reply = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {options}?bounced\r\nReferrer-Policy: no-referrer\r\n\
                         Content-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = (&stream).write_all(reply.as_bytes());
                }
            }
        });
        let bounced = window.open_tab(None, None, Focus::Foreground);
        window.navigate_with(&bounce_url, Transition::Typed);
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        bounced.reload();
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        let bounced = bounced.web_view().title().map(String::from);

        let typed = window.open_tab(None, None, Focus::Foreground);
        window.navigate_with(&options, Transition::Typed);
        wait_until("the typed options page", || typed.web_view().title().as_deref() == Some("Private options"));
        let typed_uri = typed.committed_uri();
        // Not even when the extension's own page sent the user to that site.
        typed.load(&bounce_url);
        crate::test_support::settle(std::time::Duration::from_millis(1500));
        let bounced_back = typed.committed_uri();
        // A tab restored with its history (a closed tab, the last session) shows it again.
        let restored = window.open_tab(None, None, Focus::Foreground);
        restored.restore(typed.web_view().session_state().as_ref(), &options);
        wait_until("the restored options page", || restored.committed_uri().as_deref() == Some(options.as_str()));

        window.destroy();
        settings.set_javascript_can_open_windows_automatically(popups_allowed);
        browser.uninstall_extension(&installed.id).ok();
        assert_ne!(lured.as_deref(), Some("Private options"));
        assert_ne!(bounced.as_deref(), Some("Private options"));
        assert_eq!(opened.len(), 3, "only the site and the web-accessible page opened: {opened:?}");
        assert!(!opened.iter().any(|t| t == "Private options"), "{opened:?}");
        assert_eq!(typed_uri.as_deref(), Some(options.as_str()));
        assert_eq!(bounced_back, typed_uri, "the options page stays");
    }

    #[gtk::test]
    fn a_page_cannot_send_a_window_it_holds_to_an_extension_page_that_is_not_web_accessible() {
        use std::rc::Rc;

        let browser = browser();
        let (installed, options, public) = install_private_options(&browser, "held-window");
        let server = Server::start("127.0.0.1", |path| match path {
            "/opener" => Reply::Page("Opener"),
            "/parent" => Reply::Page("Parent"),
            "/child" => Reply::Page("Child"),
            "/elsewhere" => Reply::Page("Elsewhere"),
            _ => Reply::NotFound,
        });
        let settings = browser.engine().settings().clone();
        let popups_allowed = settings.is_javascript_can_open_windows_automatically();
        settings.set_javascript_can_open_windows_automatically(true);
        let window = BrowserWindow::new(&browser);
        let title = |tab: &Tab| tab.web_view().title().map(String::from).unwrap_or_default();
        let showing = |shown: &str| window.tabs().into_iter().find(|tab| title(tab) == shown);
        let run = |tab: &Tab, script: &str| {
            let ran = glib::MainContext::default().block_on(tab.web_view().evaluate_javascript_future(script, None, None));
            ran.unwrap_or_else(|e| panic!("{script}: {e}"));
        };
        // Waits until a tab refused `target`, or a tab went there or to the options page, and
        // says whether it was refused.
        let refused = |target: &str| {
            let went = |t: &Tab| title(t) == "Private options" || t.committed_uri().as_deref() == Some(target);
            let refused = |t: &Tab| t.refused_targets().iter().any(|r| r == target);
            wait_until(target, || window.tabs().iter().any(|t| refused(t) || went(t)));
            window.tabs().iter().any(refused) && !window.tabs().iter().any(went)
        };

        // A page opens the web-accessible page in a window it keeps a reference to, then sends
        // that window on to the options page by the reference, and by the window's name.
        let opener = window.open_tab(Some(&server.url("/opener")), None, Focus::Foreground);
        wait_until("the opener", || title(&opener) == "Opener");
        run(&opener, &format!("window.held = window.open('{public}', 'held'); 0"));
        wait_until("the held window", || showing("Public page").is_some());
        let held = showing("Public page").expect("the held window");
        let by_reference = format!("{options}?by-reference");
        run(&opener, &format!("held.location = '{by_reference}'; 0"));
        let refused_by_reference = refused(&by_reference);
        let by_name = format!("{options}?by-name");
        run(&opener, &format!("window.open('{by_name}', 'held'); 0"));
        let refused_by_name = refused(&by_name);

        // A page sends its own tab to the web-accessible page, and a window it opened, which
        // keeps the tab as its opener, sends the tab on.
        let parent = window.open_tab(Some(&server.url("/parent")), None, Focus::Foreground);
        wait_until("the parent", || title(&parent) == "Parent");
        run(&parent, "window.child = window.open('/child', 'child'); 0");
        wait_until("the child", || showing("Child").is_some());
        let child = showing("Child").expect("the child");
        run(&parent, &format!("location = '{public}'; 0"));
        wait_until("the parent at the public page", || parent.committed_uri().as_deref() == Some(public.as_str()));
        let via_opener = format!("{options}?via-opener");
        run(&child, &format!("opener.location = '{via_opener}'; 0"));
        let refused_via_opener = refused(&via_opener);

        // The browser's own loads still reach the options page in the held window, typed; the
        // page that holds the window still cannot send it on from there...
        let commits = Rc::new(Cell::new(0));
        held.web_view().connect_load_changed({
            let commits = commits.clone();
            move |_, event| {
                if event == webkit::LoadEvent::Committed {
                    commits.set(commits.get() + 1);
                }
            }
        });
        held.load(&options);
        wait_until("the typed options page", || title(&held) == "Private options");
        let typed_over = format!("{options}?typed-over");
        run(&opener, &format!("held.location = '{typed_over}'; 0"));
        let at = |tab: &Tab, uri: &str| tab.committed_uri().as_deref() == Some(uri);
        wait_until(&typed_over, || held.refused_targets().contains(&typed_over) || at(&held, &typed_over));
        let refused_typed_over = !at(&held, &typed_over);
        // ...and a reload, back and forward, and a restore of its history reach it.
        let before = commits.get();
        held.reload();
        wait_until("the reloaded options page", || commits.get() > before && at(&held, &options));
        held.load(&server.url("/elsewhere"));
        wait_until("a site", || title(&held) == "Elsewhere");
        held.go_back();
        wait_until("the options page again", || at(&held, &options));
        held.go_forward();
        wait_until("the site again", || title(&held) == "Elsewhere");
        held.go_back();
        wait_until("the options page once more", || at(&held, &options));
        let restored = window.open_tab(None, None, Focus::Background);
        restored.restore(held.web_view().session_state().as_ref(), &options);
        wait_until("the restored options page", || at(&restored, &options));

        window.destroy();
        drop(child);
        settings.set_javascript_can_open_windows_automatically(popups_allowed);
        browser.uninstall_extension(&installed.id).ok();
        assert!(refused_by_reference, "the held window went to the options page by its reference");
        assert!(refused_by_name, "the held window went to the options page by its name");
        assert!(refused_via_opener, "the opener went to the options page from a window it opened");
        assert!(refused_typed_over, "the held window went on from the typed options page");
    }

    #[gtk::test]
    fn a_tab_a_page_opened_lets_its_view_go_when_it_closes() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/opener" => Reply::Page("Opener"),
            "/opened" => Reply::Page("Opened"),
            _ => Reply::NotFound,
        });
        let window = BrowserWindow::new(&browser());
        // Shown, as a window that was never shown keeps the tabs closed in it, and with a tab
        // that stays, as a window keeps its last tab until the window itself goes.
        window.present();
        window.open_tab(None, None, Focus::Background);
        let opener = window.open_tab(Some(&server.url("/opener")), None, Focus::Foreground);
        wait_until("the opener", || opener.web_view().title().as_deref() == Some("Opener"));
        let script = "window.open('/opened'); 0";
        let ran = glib::MainContext::default().block_on(opener.web_view().evaluate_javascript_future(script, None, None));
        ran.expect("window.open runs");
        let opened = || window.tabs().into_iter().find(|tab| tab.web_view().title().as_deref() == Some("Opened"));
        wait_until("the opened tab", || opened().is_some());
        let opened = opened().expect("the opened tab");
        let views = [opened.web_view().downgrade(), opener.web_view().downgrade()];

        for tab in [opened, opener] {
            window.close_tab(&tab);
        }
        wait_until("the opened tab's view to go", || views[0].upgrade().is_none());
        wait_until("the opener's view to go", || views[1].upgrade().is_none());
        window.destroy();
    }
}
