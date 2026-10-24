//! Owned by W1.3.
//! `CefEngine`: the `vsesvit_engine::Engine` the shell holds as `Rc<dyn Engine>`. Its state is the
//! main-thread registry, so the value itself is empty. Every method is one line that calls the module
//! that owns the work; methods of features later units build call their stubs (design section 8.1).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use vsesvit_engine::{
    Browsing, Chord, CloseMode, Engine, EngineDownloadId, EngineError, EnginePrefs, EngineVersions, KeyRouting,
    NativeHandle, Origin, PageFocus, PagePoint, ParkedId, Permission, PromptAnswer, PromptError, PromptId, RequestFilter,
    RequestId, Setting, ShellHooks, TabId, TabSpec, Url, Zoom,
};

use crate::handlers::{display, download, find, focus, keyboard, life_span, permission, request, resource_request};
use crate::{app, contexts, devtools, hidden, lifecycle, prefs, prompts, registry, schemes, site_data};

/// The CEF engine.
pub struct CefEngine {
    _private: (),
}

impl CefEngine {
    /// The engine over the registry `initialize` installed on this thread.
    pub fn new() -> Rc<CefEngine> {
        Rc::new(CefEngine { _private: () })
    }
}

impl Engine for CefEngine {
    fn set_hooks(&self, hooks: Rc<dyn ShellHooks>) {
        registry::with(|r| r.hooks = Some(hooks));
    }

    fn set_key_routing(&self, routing: KeyRouting) {
        keyboard::set_routing(routing);
    }

    fn apply_prefs(&self, prefs: &EnginePrefs) {
        prefs::apply_prefs(prefs);
    }

    fn set_request_filter(&self, filter: Arc<dyn RequestFilter>) {
        resource_request::set_request_filter(filter);
    }

    fn set_document_start_scripts(&self, scripts: Vec<String>) {
        lifecycle::set_document_start_scripts(scripts);
    }

    fn set_site_setting(&self, browsing: Browsing, origin: &Origin, permission: Permission, setting: Option<Setting>) {
        permission::set_site_setting(browsing, origin, permission, setting);
    }

    fn begin_private_context(&self) {
        contexts::begin_private();
    }

    fn end_private_context(&self) {
        contexts::end_private();
    }

    fn versions(&self) -> EngineVersions {
        app::versions()
    }

    fn set_hidden_host(&self, host: NativeHandle) {
        hidden::set_hidden_host(host);
    }

    fn create_tab(&self, spec: TabSpec) -> Result<(), EngineError> {
        life_span::create_tab(spec)
    }

    fn close_tab(&self, tab: TabId, mode: CloseMode) {
        life_span::close_tab(tab, mode);
    }

    fn set_visible(&self, tab: TabId, visible: bool) {
        display::set_visible(tab, visible);
    }

    fn notify_moved(&self, tab: TabId) {
        display::notify_moved(tab);
    }

    fn notify_screen_info_changed(&self, tab: TabId) {
        display::notify_screen_info_changed(tab);
    }

    fn set_page_focus(&self, tab: TabId, focus: PageFocus) {
        focus::set_policy(tab, focus);
    }

    fn set_frozen(&self, tab: TabId, frozen: bool) {
        lifecycle::set_frozen(tab, frozen);
    }

    fn focus_page(&self, tab: TabId) {
        focus::focus_page(tab);
    }

    fn blur_page(&self, tab: TabId) {
        focus::blur_page(tab);
    }

    fn detach_tab(&self, tab: TabId) -> Result<ParkedId, EngineError> {
        life_span::detach_tab(tab)
    }

    fn begin_shutdown(&self) {
        life_span::begin_shutdown();
    }

    fn close_internal(&self) {
        life_span::close_internal();
    }

    fn load(&self, tab: TabId, url: &Url) {
        request::load(tab, url);
    }

    fn load_html(&self, tab: TabId, html: String, shown_url: Option<&Url>) {
        schemes::load_html(tab, html, shown_url);
    }

    fn reload(&self, tab: TabId, bypass_cache: bool) {
        request::reload(tab, bypass_cache);
    }

    fn stop(&self, tab: TabId) {
        request::stop(tab);
    }

    fn go_back(&self, tab: TabId) {
        request::go_back(tab);
    }

    fn go_forward(&self, tab: TabId) {
        request::go_forward(tab);
    }

    fn find(&self, tab: TabId, text: &str, forward: bool, find_next: bool) {
        find::find(tab, text, forward, find_next);
    }

    fn stop_finding(&self, tab: TabId, clear_selection: bool) {
        find::stop_finding(tab, clear_selection);
    }

    fn set_zoom(&self, tab: TabId, zoom: Zoom) {
        display::set_zoom(tab, zoom);
    }

    fn zoom(&self, tab: TabId) -> Zoom {
        display::zoom(tab)
    }

    fn set_audio_muted(&self, tab: TabId, muted: bool) {
        display::set_audio_muted(tab, muted);
    }

    fn print(&self, tab: TabId) {
        display::print(tab);
    }

    fn print_to_pdf(&self, tab: TabId, path: PathBuf) -> RequestId {
        devtools::print_to_pdf(tab, path)
    }

    fn show_devtools(&self, tab: TabId, inspect_at: Option<PagePoint>) {
        devtools::show(tab, inspect_at);
    }

    fn show_console(&self, tab: TabId) {
        devtools::show_console(tab);
    }

    fn close_devtools(&self, tab: TabId) {
        devtools::close(tab);
    }

    fn has_devtools(&self, tab: TabId) -> bool {
        devtools::has(tab)
    }

    fn exit_fullscreen(&self, tab: TabId) {
        display::exit_fullscreen(tab);
    }

    fn save_mhtml(&self, tab: TabId, path: PathBuf) -> RequestId {
        devtools::save_mhtml(tab, path)
    }

    fn start_download(&self, tab: TabId, url: &Url) {
        request::start_download(tab, url);
    }

    fn eval(&self, tab: TabId, script: &str) -> RequestId {
        devtools::eval(tab, script)
    }

    fn capture_page(&self, tab: TabId) -> RequestId {
        devtools::capture(tab)
    }

    fn send_key(&self, tab: TabId, chord: Chord) {
        keyboard::send_key(tab, chord);
    }

    fn answer_prompt(&self, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError> {
        prompts::answer(id, answer)
    }

    fn continue_download(&self, id: EngineDownloadId, path: PathBuf) {
        download::continue_download(id, path);
    }

    fn cancel_download(&self, id: EngineDownloadId) {
        download::cancel_download(id);
    }

    fn pause_download(&self, id: EngineDownloadId) {
        download::pause_download(id);
    }

    fn resume_download(&self, id: EngineDownloadId) {
        download::resume_download(id);
    }

    fn cookie_sites(&self, browsing: Browsing) -> RequestId {
        site_data::cookie_sites(browsing)
    }

    fn cookies_for(&self, browsing: Browsing, url: &Url) -> RequestId {
        site_data::cookies_for(browsing, url)
    }

    fn delete_site_data(&self, browsing: Browsing, site: &str) -> RequestId {
        site_data::delete_site_data(browsing, site)
    }

    fn delete_cookies_for_hosts(&self, browsing: Browsing, hosts: &[String]) {
        site_data::delete_cookies_for_hosts(browsing, hosts);
    }

    fn clear_browsing_data(&self, browsing: Browsing) -> RequestId {
        site_data::clear_browsing_data(browsing)
    }

    #[cfg(feature = "self-test")]
    fn test_shutdown(&self, test: vsesvit_engine::TestShutdown) -> Result<(), EngineError> {
        life_span::test_shutdown(test)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use vsesvit_engine::{
        BrowserEvent, InitialPage, KeyboardOwner, NavigationDecision, NavigationRequest, NeutralizeFailed, PhysRect,
        PopupDecision, PopupRequest, RoutePhase, RoutedKey, TabEvent, TabKind,
    };

    use super::*;
    use crate::handlers::keyboard::{KeyInput, KeyKind, PreKey};
    use crate::handlers::{load, request as req};
    use crate::registry::testing::{self, FakeBrowser, Harness};
    use crate::registry::{Arm, BrowserKind, BrowserOps, HiddenRole};

    pub fn spec(tab: TabId) -> TabSpec {
        TabSpec {
            tab,
            kind: TabKind::Page,
            browsing: Browsing::Normal,
            parent: NativeHandle::from_raw(0x42),
            bounds: PhysRect { x: 0, y: 0, width: 800, height: 600 },
            scale_factor: 1.0,
            visible: true,
            page: InitialPage::Url(Url::parse("https://example.com/").expect("url")),
            opener: None,
        }
    }

    /// Opens a tab and finishes its creation.
    fn open(h: &Harness, engine: &CefEngine, tab: TabId) -> Rc<FakeBrowser> {
        engine.create_tab(spec(tab)).expect("created");
        h.created(h.last_creation().id)
    }

    fn count() -> crate::teardown::LiveCount {
        registry::with(|r| r.count()).expect("registry")
    }

    /// CEF closes a browser at once when asked.
    fn closes_at_once(page: &Rc<FakeBrowser>) {
        let react: testing::React = Rc::new(|b: &FakeBrowser, call: &str| {
            if call.starts_with("close") {
                life_span::before_close(b.id);
            }
        });
        *page.react.borrow_mut() = Some(react);
    }

    struct Hooks {
        popup_tab: Cell<Option<TabId>>,
        neutralize: Cell<bool>,
    }

    impl ShellHooks for Hooks {
        fn navigation_policy(&self, request: &NavigationRequest) -> NavigationDecision {
            if request.url.path() == "/blocked" { NavigationDecision::Cancel } else { NavigationDecision::Allow }
        }
        fn popup_policy(&self, _request: &PopupRequest) -> PopupDecision {
            match self.popup_tab.get() {
                Some(tab) => PopupDecision::AllowAsTab {
                    tab,
                    parent: NativeHandle::from_raw(0x42),
                    bounds: PhysRect::default(),
                    scale_factor: 1.0,
                },
                None => PopupDecision::Block,
            }
        }
        fn neutralize_foreign(&self, _window: NativeHandle) -> Result<(), NeutralizeFailed> {
            if self.neutralize.get() { Ok(()) } else { Err(NeutralizeFailed { reason: "gone".into() }) }
        }
    }

    fn hooks(engine: &CefEngine) -> Rc<Hooks> {
        let hooks = Rc::new(Hooks { popup_tab: Cell::new(None), neutralize: Cell::new(true) });
        engine.set_hooks(hooks.clone());
        hooks
    }

    #[test]
    fn a_tab_lives_from_request_to_before_close() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        engine.create_tab(spec(tab)).expect("created");
        assert_eq!(count().user, 1, "counted from the request");
        assert!(h.events().is_empty(), "nothing until CEF created it");
        let page = h.created(h.last_creation().id);
        assert_eq!(h.events(), [BrowserEvent::Tab(tab, TabEvent::Created { native: page.native() })]);
        assert_eq!(engine.create_tab(spec(tab)), Err(EngineError::Failed(format!("tab {} exists", tab.get()))));
        engine.close_tab(tab, CloseMode::RunUnloadHandlers);
        assert!(page.called("close"), "beforeunload runs");
        engine.close_tab(tab, CloseMode::Force);
        assert!(page.called("close(force)"));
        assert_eq!(count().user, 1, "counted until on_before_close");
        life_span::before_close(page.id);
        assert_eq!(h.events(), [BrowserEvent::Tab(tab, TabEvent::Closed)]);
        assert_eq!(count().total(), 0);
    }

    #[test]
    fn a_close_that_finishes_inside_close_browser() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        closes_at_once(&page);
        h.events();
        engine.close_tab(tab, CloseMode::Force);
        assert_eq!(h.events(), [BrowserEvent::Tab(tab, TabEvent::Closed)]);
        assert_eq!(count().total(), 0);
        assert_eq!(registry::conflicts(), 0);
    }

    #[test]
    fn a_page_that_closes_itself_is_an_unrequested_close() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        // `window.close()` on a Chrome-style tab: CEF calls no `do_close` (Alloy only, G16), only
        // `on_before_close`. The glue closes nothing else.
        assert!(!life_span::do_close(page.id), "a Chrome-style browser never closes its window through the glue");
        life_span::before_close(page.id);
        assert_eq!(h.events(), [BrowserEvent::Tab(tab, TabEvent::Closed)]);
        assert!(h.backend.posted.borrow().is_empty(), "nothing posted to the GPUI window");
    }

    #[test]
    fn a_refused_creation_is_uncounted_at_once() {
        let h = testing::install();
        let engine = CefEngine::new();
        h.backend.refuse.set(true);
        assert!(matches!(engine.create_tab(spec(TabId::next())), Err(EngineError::Failed(_))));
        assert_eq!(count().total(), 0);
    }

    #[test]
    fn both_shutdown_phases() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        engine.set_hidden_host(NativeHandle::from_raw(0x77));
        let maintenance = h.created(h.last_creation().id);
        assert!(maintenance.called("was_hidden true"));
        assert_eq!(registry::with(|r| r.entries[&maintenance.id].kind), Some(BrowserKind::Hidden(HiddenRole::Maintenance)));
        let (a, b) = (TabId::next(), TabId::next());
        let (pa, pb) = (open(&h, &engine, a), open(&h, &engine, b));
        h.events();

        engine.begin_shutdown();
        engine.begin_shutdown();
        assert!(pa.called("close(force)") && pb.called("close(force)"));
        assert!(!maintenance.called("close(force)"), "the hidden browsers serve the last jobs");
        assert_eq!(engine.create_tab(spec(TabId::next())), Err(EngineError::ShuttingDown));
        assert!(matches!(hidden::create(HiddenRole::Management, crate::registry::Style::Chrome, "chrome://extensions", Browsing::Normal),
            Err(EngineError::ShuttingDown)));
        life_span::before_close(pa.id);
        assert!(!h.events().contains(&BrowserEvent::UserBrowsersClosed));
        life_span::before_close(pb.id);
        let events = h.events();
        assert_eq!(events.iter().filter(|e| **e == BrowserEvent::UserBrowsersClosed).count(), 1);
        assert!(!events.contains(&BrowserEvent::AllBrowsersClosed));

        engine.close_internal();
        assert!(maintenance.called("close(force)"));
        life_span::before_close(maintenance.id);
        assert_eq!(h.events(), [BrowserEvent::AllBrowsersClosed]);
        assert_eq!(registry::with(|r| r.teardown.phase()), Some(crate::teardown::Phase::Drained));
    }

    #[test]
    fn a_late_creation_is_closed_at_once_and_counted_until_its_close() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        engine.create_tab(spec(tab)).expect("created");
        let creation = h.last_creation().id;
        engine.close_internal();
        assert!(h.events().is_empty(), "the creation in flight keeps the count up");
        let page = h.created(creation);
        assert!(page.called("close(force)"));
        assert!(h.events().is_empty(), "no Created for a late browser");
        life_span::before_close(page.id);
        assert_eq!(
            h.events(),
            [BrowserEvent::Tab(tab, TabEvent::Closed), BrowserEvent::UserBrowsersClosed, BrowserEvent::AllBrowsersClosed]
        );
    }

    #[test]
    fn a_held_creation_starts_at_shutdown_and_is_closed_at_once() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        registry::arm(tab, Arm::HoldCreation);
        engine.create_tab(spec(tab)).expect("created");
        assert!(h.backend.created.borrow().is_empty(), "held");
        assert_eq!(count().user, 1);
        engine.begin_shutdown();
        let page = h.created(h.last_creation().id);
        assert!(page.called("close(force)"));
        life_span::before_close(page.id);
        assert!(h.events().contains(&BrowserEvent::UserBrowsersClosed));
    }

    #[test]
    fn delayed_and_pinned_closes() {
        let h = testing::install();
        let engine = CefEngine::new();
        let (slow, pinned) = (TabId::next(), TabId::next());
        registry::arm(slow, Arm::DelayClose(std::time::Duration::from_secs(8)));
        registry::arm(pinned, Arm::PinOpen);
        let (ps, pp) = (open(&h, &engine, slow), open(&h, &engine, pinned));
        engine.begin_shutdown();
        assert!(!ps.called("close(force)") && !pp.called("close(force)"));
        assert_eq!(h.backend.posted.borrow()[0].0, std::time::Duration::from_secs(8));
        h.run_posted();
        assert!(ps.called("close(force)"), "closed after its delay");
        assert!(!pp.called("close(force)"), "pinned open");
        let live = registry::with(|r| r.live_report()).expect("registry");
        assert_eq!(live.len(), 2);
    }

    #[test]
    fn popups_ask_the_shell_and_abort_uncounts() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let hooks = hooks(&engine);
        let opener = TabId::next();
        let page = open(&h, &engine, opener);
        h.events();
        let url = "https://example.com/popup";
        assert!(life_span::before_popup(page.id, 1, url, vsesvit_engine::OpenDisposition::NewPopup, true).is_none(), "blocked");
        let popup = TabId::next();
        hooks.popup_tab.set(Some(popup));
        let place = life_span::before_popup(page.id, 2, url, vsesvit_engine::OpenDisposition::NewPopup, true).expect("allowed");
        assert_eq!(count().user, 2);
        life_span::popup_aborted(page.id, 2);
        assert_eq!(count().user, 1);
        let place2 = life_span::before_popup(page.id, 3, url, vsesvit_engine::OpenDisposition::NewForegroundTab, true).expect("allowed");
        assert_ne!(place.creation, place2.creation);
        let child = FakeBrowser::new();
        life_span::after_created(Some(place2.creation), child.clone(), child.id);
        let events = h.events();
        assert_eq!(events[0], BrowserEvent::Tab(popup, TabEvent::Created { native: child.native() }));
        assert_eq!(
            events[1],
            BrowserEvent::PopupOpened {
                opener,
                tab: popup,
                url: Url::parse(url).expect("url"),
                disposition: vsesvit_engine::OpenDisposition::NewForegroundTab
            }
        );
        engine.begin_shutdown();
        assert!(life_span::before_popup(page.id, 4, url, vsesvit_engine::OpenDisposition::NewPopup, true).is_none(), "blocked after Running");
    }

    #[test]
    fn chromiums_own_windows_are_neutralized_or_closed() {
        let _h = testing::install();
        let engine = CefEngine::new();
        let hooks = hooks(&engine);
        let kept = FakeBrowser::new();
        life_span::after_created(None, kept.clone(), kept.id);
        assert!(!kept.called("close(force)"), "neutralized");
        assert_eq!(count().user, 1);
        hooks.neutralize.set(false);
        let failed = FakeBrowser::new();
        life_span::after_created(None, failed.clone(), failed.id);
        assert!(failed.called("close(force)"), "closed when neutralize failed");
        engine.begin_shutdown();
        assert!(kept.called("close(force)"), "a user browser for shutdown");
    }

    #[test]
    fn a_detached_tab_ends_but_its_browser_stays_counted() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        let parked = engine.detach_tab(tab).expect("detached");
        assert_eq!(h.events(), [BrowserEvent::Tab(tab, TabEvent::Detached)]);
        assert_eq!(engine.detach_tab(tab), Err(EngineError::UnknownTab(tab)));
        engine.load(tab, &Url::parse("https://example.org/").expect("url"));
        assert!(!page.calls.borrow().iter().any(|c| c.starts_with("load")), "the tab is gone");
        assert_eq!(count().user, 1);
        life_span::close_parked(parked);
        life_span::before_close(page.id);
        assert!(h.events().is_empty(), "no Closed for a detached tab");
        assert_eq!(count().total(), 0);
    }

    #[test]
    fn focus_follows_the_published_policy() {
        let _h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        engine.set_page_focus(tab, PageFocus::NEW_BROWSER);
        let page = open(&_h, &engine, tab);
        assert!(focus::refuse(page.id), "a new browser is refused");
        engine.set_page_focus(tab, PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page });
        assert!(!focus::refuse(page.id));
        engine.set_page_focus(tab, PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Chrome });
        assert!(focus::refuse(page.id), "the omnibox has the keyboard");
        engine.set_hidden_host(NativeHandle::from_raw(0x77));
        let hidden_page = _h.created(_h.last_creation().id);
        assert!(focus::refuse(hidden_page.id), "hidden browsers never take focus");
        engine.focus_page(tab);
        assert!(page.called("set_focus true"));
    }

    #[test]
    fn navigation_asks_the_shell() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        hooks(&engine);
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        let browse = |url: &str| req::Browse { url: url.into(), main_frame: true, is_redirect: false, user_gesture: false, transition: 0 };
        assert!(req::before_browse(page.id, &browse("https://example.com/blocked")));
        assert!(!req::before_browse(page.id, &browse("https://example.com/ok")));
        assert_eq!(
            h.events(),
            [BrowserEvent::Tab(tab, TabEvent::NavigationStarted { url: Url::parse("https://example.com/ok").expect("url"), is_redirect: false })]
        );
        load::committed(page.id, "https://example.com/ok", vsesvit_engine::EngineTransition::Link);
        assert!(matches!(h.events()[..], [BrowserEvent::Tab(t, TabEvent::Committed { .. })] if t == tab));
        engine.reload(tab, false);
        assert_eq!(registry::with(|r| r.entries[&page.id].shell_navigation), Some(true));
        load::committed(page.id, "https://example.com/ok", vsesvit_engine::EngineTransition::Reload);
        assert_eq!(registry::with(|r| r.entries[&page.id].shell_navigation), Some(false));
    }

    #[test]
    fn routed_keys_leave_the_page() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        let new_tab: Chord = "Ctrl+T".parse().expect("chord");
        let find: Chord = "Ctrl+F".parse().expect("chord");
        engine.set_key_routing(KeyRouting {
            entries: vec![
                (new_tab, RoutePhase::BeforePage, RoutedKey::NextTab),
                (find, RoutePhase::AfterPage, RoutedKey::PreviousTab),
            ],
        });
        let press = |vk, kind| KeyInput { kind, windows_key_code: vk, native_key_code: 0, modifiers: 4, character: 0 };
        assert_eq!(keyboard::pre_key(page.id, &press(0x54, KeyKind::RawKeyDown)), PreKey::Swallow);
        assert_eq!(keyboard::pre_key(page.id, &press(0x54, KeyKind::KeyUp)), PreKey::Swallow);
        assert_eq!(h.events(), [BrowserEvent::Shortcut { tab: Some(tab), key: RoutedKey::NextTab }]);
        assert_eq!(keyboard::pre_key(page.id, &press(0x46, KeyKind::RawKeyDown)), PreKey::Shortcut);
        assert!(h.events().is_empty(), "the page sees it first");
        assert!(keyboard::key(page.id, &press(0x46, KeyKind::RawKeyDown)));
        assert_eq!(h.events(), [BrowserEvent::Shortcut { tab: Some(tab), key: RoutedKey::PreviousTab }]);
        assert_eq!(keyboard::pre_key(page.id, &press(0x41, KeyKind::RawKeyDown)), PreKey::Pass);
        engine.send_key(tab, new_tab);
        assert!(page.called("send_key RawKeyDown 0x54") && page.called("send_key KeyUp 0x54"));
    }

    #[test]
    fn page_messages_only_from_an_allowed_main_frame() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        let store = "vsesvit.store";
        assert!(crate::client::page_message(page.id, "https://chromewebstore.google.com/detail/x", true, store, "{}".into()));
        assert!(matches!(&h.events()[..], [BrowserEvent::PageMessage { tab: t, main_frame: true, payload, .. }] if *t == tab && payload == "{}"));
        crate::client::page_message(page.id, "https://evil.example/", true, store, "{}".into());
        crate::client::page_message(page.id, "https://chromewebstore.google.com/", false, store, "{}".into());
        assert!(h.events().is_empty(), "another origin and a subframe are dropped");
        assert!(!crate::client::page_message(page.id, "https://chromewebstore.google.com/", true, "other", "{}".into()));
    }

    #[test]
    fn the_private_context_goes_after_its_last_browser() {
        let h = testing::install();
        let engine = CefEngine::new();
        engine.begin_private_context();
        let tab = TabId::next();
        engine.create_tab(TabSpec { browsing: Browsing::Private, ..spec(tab) }).expect("created");
        let page = h.created(h.last_creation().id);
        engine.end_private_context();
        assert_eq!(*h.backend.private.borrow(), [true], "kept while a private browser lives");
        life_span::before_close(page.id);
        assert_eq!(*h.backend.private.borrow(), [true, false]);
    }

    #[test]
    fn requests_answer_with_their_ids() {
        let mut h = testing::install();
        let engine = CefEngine::new();
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        h.events();
        let id = engine.eval(tab, "1 + 2");
        assert!(page.called("devtools Runtime.evaluate"));
        let message = registry::with(|r| r.devtools.ids()).expect("registry")[0];
        devtools::on_result(message, true, br#"{"result":{"type":"number","value":3}}"#);
        assert_eq!(h.events(), [BrowserEvent::RequestFinished { id, result: vsesvit_engine::RequestResult::Eval(Ok("3".into())) }]);
        let missing = engine.eval(TabId::next(), "1");
        assert!(matches!(&h.events()[..], [BrowserEvent::RequestFinished { id, result: vsesvit_engine::RequestResult::Eval(Err(_)) }] if *id == missing));
    }

    /// Design section 1.6: every `Engine` method is called from inside every UI-thread handler path
    /// that calls out of the glue (a hook, or a call on a browser during which CEF runs callbacks
    /// synchronously); a path that calls nothing out cannot be re-entered. No registry borrow may be
    /// held across those calls, so the registry is never found borrowed.
    #[test]
    fn every_engine_method_runs_inside_every_handler_path() {
        struct Reentrant {
            engine: Rc<CefEngine>,
            depth: Cell<u32>,
            tab: Cell<Option<TabId>>,
        }

        impl Reentrant {
            fn exercise(&self) {
                let Some(tab) = self.tab.get() else { return };
                if self.depth.get() >= 2 {
                    return;
                }
                self.depth.set(self.depth.get() + 1);
                every_method(&*self.engine, tab);
                self.depth.set(self.depth.get() - 1);
            }
        }

        impl ShellHooks for Reentrant {
            fn navigation_policy(&self, _request: &NavigationRequest) -> NavigationDecision {
                self.exercise();
                NavigationDecision::Allow
            }
            fn popup_policy(&self, _request: &PopupRequest) -> PopupDecision {
                self.exercise();
                PopupDecision::AllowAsTab { tab: TabId::next(), parent: NativeHandle::from_raw(1), bounds: PhysRect::default(), scale_factor: 1.0 }
            }
            fn neutralize_foreign(&self, _window: NativeHandle) -> Result<(), NeutralizeFailed> {
                self.exercise();
                Ok(())
            }
        }

        fn every_method(engine: &dyn Engine, tab: TabId) {
            let url = Url::parse("https://example.com/again").expect("url");
            engine.set_key_routing(KeyRouting::default());
            engine.apply_prefs(&EnginePrefs {
                block_popups: true,
                color_scheme: vsesvit_engine::ColorScheme::System,
                spellcheck: None,
                accept_languages: "en".into(),
                cookie_controls: vsesvit_engine::CookieControls::Allow,
                autofill: false,
            });
            engine.set_document_start_scripts(Vec::new());
            engine.begin_private_context();
            engine.end_private_context();
            let _ = engine.versions();
            engine.set_visible(tab, false);
            engine.set_visible(tab, true);
            engine.notify_moved(tab);
            engine.notify_screen_info_changed(tab);
            engine.set_page_focus(tab, PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page });
            engine.set_frozen(tab, false);
            engine.focus_page(tab);
            engine.blur_page(tab);
            engine.load(tab, &url);
            engine.load_html(tab, "<p>".into(), None);
            engine.reload(tab, true);
            engine.stop(tab);
            engine.go_back(tab);
            engine.go_forward(tab);
            engine.find(tab, "x", true, false);
            engine.stop_finding(tab, true);
            engine.set_zoom(tab, Zoom(1.2));
            let _ = engine.zoom(tab);
            engine.set_audio_muted(tab, true);
            engine.print(tab);
            let _ = engine.print_to_pdf(tab, PathBuf::from("x.pdf"));
            engine.show_devtools(tab, None);
            engine.show_console(tab);
            let _ = engine.has_devtools(tab);
            engine.close_devtools(tab);
            engine.exit_fullscreen(tab);
            let _ = engine.save_mhtml(tab, PathBuf::from("x.mhtml"));
            engine.start_download(tab, &url);
            let _ = engine.eval(tab, "1");
            let _ = engine.capture_page(tab);
            engine.send_key(tab, "Ctrl+T".parse().expect("chord"));
            let _ = engine.answer_prompt(PromptId(424_242), PromptAnswer::Menu(None));
            engine.continue_download(EngineDownloadId(1), PathBuf::from("d"));
            engine.cancel_download(EngineDownloadId(1));
            engine.pause_download(EngineDownloadId(1));
            engine.resume_download(EngineDownloadId(1));
            let _ = engine.cookie_sites(Browsing::Normal);
            let _ = engine.cookies_for(Browsing::Normal, &url);
            let _ = engine.delete_site_data(Browsing::Normal, "example.com");
            engine.delete_cookies_for_hosts(Browsing::Normal, &["example.com".into()]);
            let _ = engine.clear_browsing_data(Browsing::Normal);
            let other = TabId::next();
            if engine.create_tab(spec(other)).is_ok() {
                engine.close_tab(other, CloseMode::Force);
            }
        }

        let h = testing::install();
        let engine = CefEngine::new();
        let hooks = Rc::new(Reentrant { engine: engine.clone(), depth: Cell::new(0), tab: Cell::new(None) });
        engine.set_hooks(hooks.clone());
        let tab = TabId::next();
        let page = open(&h, &engine, tab);
        hooks.tab.set(Some(tab));

        // CEF runs callbacks synchronously inside these calls (section 1.6): the fake does the same.
        let react: testing::React = Rc::new(|b: &FakeBrowser, call: &str| {
            let browse = req::Browse { url: "https://example.com/x".into(), main_frame: true, is_redirect: false, user_gesture: false, transition: 0 };
            match call {
                c if c.starts_with("load") || c == "reload" || c == "go_back" || c == "go_forward" => {
                    req::before_browse(b.id, &browse);
                    load::committed(b.id, "https://example.com/x", vsesvit_engine::EngineTransition::Link);
                }
                "set_focus true" => {
                    focus::refuse(b.id);
                    crate::handlers::display::to_tab(b.id, TabEvent::FocusChanged { focused: true });
                }
                _ => {}
            }
        });
        *page.react.borrow_mut() = Some(react);

        let browse = req::Browse { url: "https://example.com/y".into(), main_frame: true, is_redirect: false, user_gesture: true, transition: 0 };
        req::before_browse(page.id, &browse);
        life_span::before_popup(page.id, 7, "https://example.com/p", vsesvit_engine::OpenDisposition::NewPopup, true);
        let foreign = FakeBrowser::new();
        life_span::after_created(None, foreign.clone(), foreign.id);
        every_method(&*engine, tab);

        // The destructive calls, from inside a handler path, then the drain.
        struct Closer(Rc<CefEngine>, TabId);
        impl ShellHooks for Closer {
            fn navigation_policy(&self, _request: &NavigationRequest) -> NavigationDecision {
                let _ = self.0.detach_tab(self.1);
                self.0.close_tab(self.1, CloseMode::Force);
                self.0.begin_shutdown();
                self.0.close_internal();
                NavigationDecision::Allow
            }
            fn popup_policy(&self, _request: &PopupRequest) -> PopupDecision {
                PopupDecision::Block
            }
            fn neutralize_foreign(&self, _window: NativeHandle) -> Result<(), NeutralizeFailed> {
                Ok(())
            }
        }
        engine.set_hooks(Rc::new(Closer(engine.clone(), tab)));
        req::before_browse(page.id, &browse);
        let live: Vec<i32> = registry::with(|r| r.entries.keys().copied().collect()).expect("registry");
        for id in live {
            life_span::before_close(id);
        }
        let pending: Vec<_> = registry::with(|r| r.pending.keys().copied().collect()).expect("registry");
        for creation in pending {
            let late = h.created(creation);
            life_span::before_close(late.id);
        }

        assert_eq!(registry::conflicts(), 0, "no path held the registry across a call out");
        assert_eq!(count().total(), 0);
        assert_eq!(registry::with(|r| r.teardown.phase()), Some(crate::teardown::Phase::Drained));
    }
}
