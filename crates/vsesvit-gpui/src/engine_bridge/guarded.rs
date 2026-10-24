//! Owned by W1.6.
//! The engine as the rest of the shell holds it. It forwards every call, except that it publishes the
//! page-focus policy first wherever section 1.5 needs one, so no caller can create a tab or start a
//! navigation without it: `PageFocus::NEW_BROWSER` before `create_tab`, the tab's policy before
//! `load`, `load_html`, `reload`, `go_back` and `go_forward`. Focus itself goes through [`Focus`]:
//! `focus_page` and `blur_page` ask the policy, and `set_page_focus` is the policy's alone.
//!
//! `test_shutdown` is not forwarded, so it answers `Unsupported` (the trait's default): the trait has it
//! only with `vsesvit-engine/self-test`, which this crate's features do not turn on yet (W1.8).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use vsesvit_engine::{
    Browsing, Chord, CloseMode, Engine, EngineDownloadId, EngineError, EnginePrefs, EngineVersions, KeyRouting, NativeHandle,
    Origin, PageFocus, PagePoint, ParkedId, Permission, PromptAnswer, PromptError, PromptId, RequestFilter, RequestId,
    Setting, ShellHooks, TabId, TabSpec, Url, Zoom,
};

use super::focus::Focus;

pub(crate) struct GuardedEngine {
    inner: Rc<dyn Engine>,
    focus: Focus,
}

impl GuardedEngine {
    pub fn new(inner: Rc<dyn Engine>, focus: Focus) -> GuardedEngine {
        GuardedEngine { inner, focus }
    }
}

impl Engine for GuardedEngine {
    fn set_hooks(&self, hooks: Rc<dyn ShellHooks>) {
        self.inner.set_hooks(hooks)
    }

    fn set_key_routing(&self, routing: KeyRouting) {
        self.inner.set_key_routing(routing)
    }

    fn apply_prefs(&self, prefs: &EnginePrefs) {
        self.inner.apply_prefs(prefs)
    }

    fn set_request_filter(&self, filter: Arc<dyn RequestFilter>) {
        self.inner.set_request_filter(filter)
    }

    fn set_document_start_scripts(&self, scripts: Vec<String>) {
        self.inner.set_document_start_scripts(scripts)
    }

    fn set_site_setting(&self, browsing: Browsing, origin: &Origin, permission: Permission, setting: Option<Setting>) {
        self.inner.set_site_setting(browsing, origin, permission, setting)
    }

    fn begin_private_context(&self) {
        self.inner.begin_private_context()
    }

    fn end_private_context(&self) {
        self.inner.end_private_context()
    }

    fn versions(&self) -> EngineVersions {
        self.inner.versions()
    }

    fn set_hidden_host(&self, host: NativeHandle) {
        self.inner.set_hidden_host(host)
    }

    /// A tab whose browser already exists keeps its policy; the engine refuses the second creation.
    fn create_tab(&self, spec: TabSpec) -> Result<(), EngineError> {
        if !self.focus.has_page(spec.tab) {
            self.focus.before_create(spec.tab);
        }
        self.inner.create_tab(spec)
    }

    fn close_tab(&self, tab: TabId, mode: CloseMode) {
        self.inner.close_tab(tab, mode)
    }

    fn set_visible(&self, tab: TabId, visible: bool) {
        self.inner.set_visible(tab, visible)
    }

    fn notify_moved(&self, tab: TabId) {
        self.inner.notify_moved(tab)
    }

    fn notify_screen_info_changed(&self, tab: TabId) {
        self.inner.notify_screen_info_changed(tab)
    }

    fn set_page_focus(&self, tab: TabId, focus: PageFocus) {
        log::error!("engine: set_page_focus({}, {focus:?}) from outside the focus policy ignored", tab.get());
    }

    fn set_frozen(&self, tab: TabId, frozen: bool) {
        self.inner.set_frozen(tab, frozen)
    }

    fn focus_page(&self, tab: TabId) {
        self.focus.page_wants_keyboard(tab)
    }

    fn blur_page(&self, tab: TabId) {
        self.focus.page_gives_up_keyboard(tab)
    }

    fn detach_tab(&self, tab: TabId) -> Result<ParkedId, EngineError> {
        self.inner.detach_tab(tab)
    }

    fn begin_shutdown(&self) {
        self.inner.begin_shutdown()
    }

    fn close_internal(&self) {
        self.inner.close_internal()
    }

    fn load(&self, tab: TabId, url: &Url) {
        self.focus.before_navigation(tab);
        self.inner.load(tab, url)
    }

    fn load_html(&self, tab: TabId, html: String, shown_url: Option<&Url>) {
        self.focus.before_navigation(tab);
        self.inner.load_html(tab, html, shown_url)
    }

    fn reload(&self, tab: TabId, bypass_cache: bool) {
        self.focus.before_navigation(tab);
        self.inner.reload(tab, bypass_cache)
    }

    fn stop(&self, tab: TabId) {
        self.inner.stop(tab)
    }

    fn go_back(&self, tab: TabId) {
        self.focus.before_navigation(tab);
        self.inner.go_back(tab)
    }

    fn go_forward(&self, tab: TabId) {
        self.focus.before_navigation(tab);
        self.inner.go_forward(tab)
    }

    fn find(&self, tab: TabId, text: &str, forward: bool, find_next: bool) {
        self.inner.find(tab, text, forward, find_next)
    }

    fn stop_finding(&self, tab: TabId, clear_selection: bool) {
        self.inner.stop_finding(tab, clear_selection)
    }

    fn set_zoom(&self, tab: TabId, zoom: Zoom) {
        self.inner.set_zoom(tab, zoom)
    }

    fn zoom(&self, tab: TabId) -> Zoom {
        self.inner.zoom(tab)
    }

    fn set_audio_muted(&self, tab: TabId, muted: bool) {
        self.inner.set_audio_muted(tab, muted)
    }

    fn print(&self, tab: TabId) {
        self.inner.print(tab)
    }

    fn print_to_pdf(&self, tab: TabId, path: PathBuf) -> RequestId {
        self.inner.print_to_pdf(tab, path)
    }

    fn show_devtools(&self, tab: TabId, inspect_at: Option<PagePoint>) {
        self.inner.show_devtools(tab, inspect_at)
    }

    fn show_console(&self, tab: TabId) {
        self.inner.show_console(tab)
    }

    fn close_devtools(&self, tab: TabId) {
        self.inner.close_devtools(tab)
    }

    fn has_devtools(&self, tab: TabId) -> bool {
        self.inner.has_devtools(tab)
    }

    fn exit_fullscreen(&self, tab: TabId) {
        self.inner.exit_fullscreen(tab)
    }

    fn save_mhtml(&self, tab: TabId, path: PathBuf) -> RequestId {
        self.inner.save_mhtml(tab, path)
    }

    fn start_download(&self, tab: TabId, url: &Url) {
        self.inner.start_download(tab, url)
    }

    fn eval(&self, tab: TabId, script: &str) -> RequestId {
        self.inner.eval(tab, script)
    }

    fn capture_page(&self, tab: TabId) -> RequestId {
        self.inner.capture_page(tab)
    }

    fn send_key(&self, tab: TabId, chord: Chord) {
        self.inner.send_key(tab, chord)
    }

    fn answer_prompt(&self, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError> {
        self.inner.answer_prompt(id, answer)
    }

    fn continue_download(&self, id: EngineDownloadId, path: PathBuf) {
        self.inner.continue_download(id, path)
    }

    fn cancel_download(&self, id: EngineDownloadId) {
        self.inner.cancel_download(id)
    }

    fn pause_download(&self, id: EngineDownloadId) {
        self.inner.pause_download(id)
    }

    fn resume_download(&self, id: EngineDownloadId) {
        self.inner.resume_download(id)
    }

    fn cookie_sites(&self, browsing: Browsing) -> RequestId {
        self.inner.cookie_sites(browsing)
    }

    fn cookies_for(&self, browsing: Browsing, url: &Url) -> RequestId {
        self.inner.cookies_for(browsing, url)
    }

    fn delete_site_data(&self, browsing: Browsing, site: &str) -> RequestId {
        self.inner.delete_site_data(browsing, site)
    }

    fn delete_cookies_for_hosts(&self, browsing: Browsing, hosts: &[String]) {
        self.inner.delete_cookies_for_hosts(browsing, hosts)
    }

    fn clear_browsing_data(&self, browsing: Browsing) -> RequestId {
        self.inner.clear_browsing_data(browsing)
    }
}
