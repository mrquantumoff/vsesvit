//! Owned by W1.3.
//! Browser lifetimes (design sections 1.5, 3.6 and 3.7): creation requests and their accounting,
//! `on_before_popup` and its aborts, `on_after_created`, `do_close`, `on_before_close`, `detach_tab`, and
//! both shutdown phases with their late-creation rules. Every browser is counted from the moment the
//! glue asks CEF for it until its `on_before_close`.

use std::rc::Rc;
use std::sync::Arc;

use cef::*;
use vsesvit_engine::{
    BrowserEvent, Browsing, CloseMode, EngineError, InitialPage, NativeHandle, OpenDisposition, PageFocus, ParkedId,
    PhysRect, PopupDecision, PopupRequest, PromptEnd, TabEvent, TabId, TabKind, TabSpec, Url,
};

use crate::client::{self, CefBrowser, HandlerCtx};
use crate::registry::{self, AfterCreate, Arm, BrowserId, BrowserKind, BrowserOps, Creation, CreationId, Entry, Pending, Style};
use crate::{contexts, prompts, strings};

/// What to ask CEF for.
pub struct Request {
    /// What the browser will be.
    pub kind: BrowserKind,
    /// Its context.
    pub browsing: Browsing,
    /// Its style.
    pub style: Style,
    /// Its parent window.
    pub parent: NativeHandle,
    /// Where in the parent.
    pub bounds: PhysRect,
    /// The parent's scale.
    pub scale: f32,
    /// The first URL.
    pub url: String,
    /// What to do once it exists.
    pub then: AfterCreate,
}

/// Asks CEF for a browser and counts it at once. Refused once shutdown began; a refusal by CEF
/// uncounts it at once.
pub fn request(req: Request) -> Result<CreationId, EngineError> {
    let asked = registry::with(|r| {
        if !r.teardown.admits() {
            return Err(EngineError::ShuttingDown);
        }
        if let BrowserKind::Tab(tab) = req.kind
            && (r.tabs.contains_key(&tab) || r.pending.values().any(|p| p.kind == req.kind))
        {
            return Err(EngineError::Failed(format!("tab {} exists", tab.get())));
        }
        if req.browsing == Browsing::Private {
            r.private_open = true;
        }
        let id = r.next_creation();
        let creation = Creation {
            id,
            parent: req.parent,
            bounds: req.bounds,
            scale: req.scale,
            style: req.style,
            browsing: req.browsing,
            url: req.url,
        };
        let held = matches!(req.kind, BrowserKind::Tab(tab) if r.arms.get(&tab) == Some(&Arm::HoldCreation));
        let pending = Pending {
            kind: req.kind,
            browsing: req.browsing,
            style: req.style,
            counted: true,
            requested: std::time::Instant::now(),
            then: req.then,
        };
        r.pending.insert(id, pending);
        if held {
            r.held.push(creation);
            return Ok((id, None));
        }
        Ok((id, Some((creation, r.backend.clone()))))
    })
    .unwrap_or(Err(EngineError::Failed("the engine is not running on this thread".into())))?;
    let (id, now) = asked;
    if let Some((creation, backend)) = now
        && !backend.create(&creation)
    {
        registry::with(|r| r.pending.remove(&id));
        registry::check_teardown();
        return Err(EngineError::Failed("CEF refused to create the browser".into()));
    }
    Ok(id)
}

/// `Engine::create_tab`.
pub fn create_tab(spec: TabSpec) -> Result<(), EngineError> {
    let (url, html) = match spec.page {
        InitialPage::Url(url) => (url.to_string(), None),
        InitialPage::Html { html, shown_url } => ("about:blank".to_owned(), Some((html, shown_url))),
        InitialPage::Blank => ("about:blank".to_owned(), None),
    };
    let (style, auto_resize) = match spec.kind {
        TabKind::Page => (Style::Chrome, None),
        TabKind::ExtensionPopup { min, max } => (Style::Alloy, Some((min, max))),
    };
    request(Request {
        kind: BrowserKind::Tab(spec.tab),
        browsing: spec.browsing,
        style,
        parent: spec.parent,
        bounds: spec.bounds,
        scale: spec.scale_factor,
        url,
        then: AfterCreate { hidden: !spec.visible, html, auto_resize, ..AfterCreate::default() },
    })
    .map(|_| ())
}

/// Asks a browser to close. `force` skips `beforeunload`, marks the browser closing (no prompt is
/// admitted any more) and ends its prompts. A tab's shutdown self-test arm can delay or pin the close.
pub fn close_browser(id: BrowserId, force: bool) {
    let found = registry::with(|r| {
        let entry = r.entries.get(&id)?;
        let arm = entry.kind.tab().and_then(|t| r.arms.get(&t).copied());
        Some((entry.page.clone(), entry.atoms.clone(), arm, r.backend.clone()))
    })
    .flatten();
    let Some((page, atoms, arm, backend)) = found else { return };
    if force {
        atoms.closing.store(true, std::sync::atomic::Ordering::SeqCst);
        prompts::end_browser(id, PromptEnd::TabClosed);
    }
    match arm {
        Some(Arm::PinOpen) => log::warn!("vsesvit-cef: browser {id} is pinned open (self-test)"),
        Some(Arm::DelayClose(delay)) => backend.post(
            delay,
            Box::new(move || {
                if let Some(page) = registry::browser(id) {
                    page.close(true);
                }
            }),
        ),
        _ => page.close(force),
    }
}

/// `Engine::close_tab`. A tab still being created closes as soon as it exists.
pub fn close_tab(tab: TabId, mode: CloseMode) {
    let browser = registry::with(|r| {
        if let Some(id) = r.tabs.get(&tab) {
            return Some(*id);
        }
        if let Some(p) = r.pending.values_mut().find(|p| p.kind == BrowserKind::Tab(tab)) {
            p.then.close = true;
        }
        None
    })
    .flatten();
    if let Some(id) = browser {
        close_browser(id, mode == CloseMode::Force);
    }
}

/// `Engine::detach_tab`: the tab ends; its browser stays, parked, until `close_parked`.
pub fn detach_tab(tab: TabId) -> Result<ParkedId, EngineError> {
    let (id, parked, sink) = registry::with(|r| {
        if !r.teardown.admits() {
            return Err(EngineError::ShuttingDown);
        }
        let id = *r.tabs.get(&tab).ok_or(EngineError::UnknownTab(tab))?;
        r.tabs.remove(&tab);
        r.focus.remove(&tab);
        let parked = r.next_parked();
        if let Some(entry) = r.entries.get_mut(&id) {
            entry.kind = BrowserKind::Parked(parked);
        }
        Ok((id, parked, r.sink.clone()))
    })
    .unwrap_or(Err(EngineError::UnknownTab(tab)))?;
    prompts::end_browser(id, PromptEnd::TabClosed);
    sink.tab(tab, TabEvent::Detached);
    super::download::on_parked(parked, id);
    Ok(parked)
}

/// Closes a parked browser once its downloads ended (W3.3 calls it).
pub fn close_parked(parked: ParkedId) {
    let id = registry::with(|r| r.entries.iter().find(|(_, e)| e.kind == BrowserKind::Parked(parked)).map(|(id, _)| *id)).flatten();
    if let Some(id) = id {
        close_browser(id, true);
    }
}

fn ids_where(keep: impl Fn(&Entry) -> bool) -> Vec<BrowserId> {
    registry::with(|r| r.entries.iter().filter(|(_, e)| keep(e)).map(|(id, _)| *id).collect()).unwrap_or_default()
}

/// `Engine::begin_shutdown` (section 3.6 step 3).
pub fn begin_shutdown() {
    if registry::with(|r| r.teardown.begin_shutdown()) != Some(true) {
        return;
    }
    prompts::shut();
    let held = registry::with(|r| (std::mem::take(&mut r.held), r.backend.clone()));
    if let Some((held, backend)) = held {
        for creation in held {
            if !backend.create(&creation) {
                registry::with(|r| r.pending.remove(&creation.id));
            }
        }
    }
    for id in ids_where(|e| e.kind.user()) {
        close_browser(id, true);
    }
    registry::check_teardown();
}

/// `Engine::close_internal` (section 3.6 step 5). Begins shutdown first if needed.
pub fn close_internal() {
    begin_shutdown();
    if registry::with(|r| r.teardown.close_internal()) == Some(true) {
        for id in ids_where(|e| !e.kind.user()) {
            close_browser(id, true);
        }
    }
    registry::check_teardown();
}

/// `Engine::test_shutdown`: arms a tab, before it is created, with a shutdown self-test case.
#[cfg(feature = "self-test")]
pub fn test_shutdown(test: vsesvit_engine::TestShutdown) -> Result<(), EngineError> {
    use vsesvit_engine::TestShutdown;
    let (tab, arm) = match test {
        TestShutdown::DelayClose { tab, delay } => (tab, Arm::DelayClose(delay)),
        TestShutdown::PinOpen { tab } => (tab, Arm::PinOpen),
        TestShutdown::HoldCreation { tab } => (tab, Arm::HoldCreation),
    };
    registry::arm(tab, arm);
    Ok(())
}

/// The logic of `on_after_created`. `creation` is the client's; `None` is a browser Chromium created
/// itself (`default_client`).
pub fn after_created(creation: Option<CreationId>, page: Rc<dyn BrowserOps>, id: BrowserId) {
    let atoms = prompts::register_scope(id);
    let native = page.native();
    let known = registry::with(|r| {
        let pending = creation.and_then(|c| r.pending.remove(&c));
        let (kind, browsing, style, then) = match pending {
            Some(p) => (p.kind, p.browsing, p.style, p.then),
            None => (BrowserKind::Foreign, Browsing::Normal, Style::Chrome, AfterCreate::default()),
        };
        r.entries.insert(
            id,
            Entry {
                page: page.clone(),
                kind,
                browsing,
                style,
                atoms,
                since: std::time::Instant::now(),
                last_seen: "on_after_created",
                // The first load of a tab the shell created is the shell's; a popup's is the page's.
                shell_navigation: matches!(kind, BrowserKind::Tab(_)) && then.popup.is_none(),
            },
        );
        if let BrowserKind::Tab(tab) = kind {
            r.tabs.insert(tab, id);
        }
        (kind, then, r.teardown.admits(), r.sink.clone(), r.hooks.clone())
    });
    let Some((kind, then, running, sink, hooks)) = known else { return };
    if let BrowserKind::Foreign = kind {
        let neutralized = match &hooks {
            Some(hooks) => hooks.neutralize_foreign(native).map_err(|e| e.reason),
            None => Err("no hooks".to_owned()),
        };
        match neutralized {
            Ok(()) if running => {
                log::info!("vsesvit-cef: Chromium opened browser {id} on its own; neutralized");
                return;
            }
            Ok(()) => {}
            Err(reason) => log::warn!("vsesvit-cef: closing browser {id} Chromium opened on its own: {reason}"),
        }
        close_browser(id, true);
        return;
    }
    if !running || then.close {
        close_browser(id, true);
        return;
    }
    if let BrowserKind::Tab(tab) = kind {
        sink.tab(tab, TabEvent::Created { native });
        if let Some((_, _, opener, url, disposition)) = then.popup {
            sink.send(BrowserEvent::PopupOpened { opener, tab, url, disposition });
        }
        if let Some((html, shown_url)) = then.html {
            super::super::schemes::load_html(tab, html, shown_url.as_ref());
        }
    }
    if then.hidden {
        page.was_hidden(true);
    }
    if let Some((min, max)) = then.auto_resize {
        page.set_auto_resize(min, max);
    }
}

/// The logic of `on_before_close`.
pub fn before_close(id: BrowserId) {
    prompts::end_browser(id, PromptEnd::TabClosed);
    let removed = registry::with(|r| {
        let entry = r.entries.remove(&id)?;
        entry.atoms.closing.store(true, std::sync::atomic::Ordering::SeqCst);
        if let BrowserKind::Tab(tab) = entry.kind {
            r.tabs.remove(&tab);
            r.focus.remove(&tab);
            r.arms.remove(&tab);
        }
        Some((entry, r.sink.clone()))
    })
    .flatten();
    prompts::forget_scope(id);
    let Some((entry, sink)) = removed else { return };
    if let BrowserKind::Tab(tab) = entry.kind {
        sink.tab(tab, TabEvent::Closed);
    }
    drop(entry);
    contexts::drop_private_if_done();
    registry::check_teardown();
}

/// The logic of `do_close`: true when the glue closes the window itself. Only Alloy browsers get
/// here (G16). On Windows CEF's default posts `WM_CLOSE` to the root window, which would close the
/// GPUI window (or the hidden host), so the glue destroys the browser's own window instead
/// (spike-ext-win decision 4). Elsewhere CEF's default closes the browser's own window.
pub fn do_close(id: BrowserId) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let found = registry::with(|r| r.entries.get(&id).filter(|e| e.style == Style::Alloy).map(|e| (e.page.native(), r.backend.clone())));
    let Some(Some((native, backend))) = found else { return false };
    #[cfg(windows)]
    {
        let raw = native.raw();
        backend.post(std::time::Duration::ZERO, Box::new(move || crate::native::destroy_window(NativeHandle::from_raw(raw))));
    }
    #[cfg(not(windows))]
    let _ = (native, backend);
    true
}

/// Where a page wants a link opened, from CEF's raw disposition.
pub fn disposition_of(raw: i64) -> OpenDisposition {
    let is = |d: WindowOpenDisposition| raw == i64::from(d.get_raw());
    if is(WindowOpenDisposition::NEW_FOREGROUND_TAB) {
        OpenDisposition::NewForegroundTab
    } else if is(WindowOpenDisposition::NEW_BACKGROUND_TAB) {
        OpenDisposition::NewBackgroundTab
    } else if is(WindowOpenDisposition::NEW_POPUP) {
        OpenDisposition::NewPopup
    } else if is(WindowOpenDisposition::NEW_WINDOW) {
        OpenDisposition::NewWindow
    } else if is(WindowOpenDisposition::OFF_THE_RECORD) {
        OpenDisposition::OffTheRecord
    } else if is(WindowOpenDisposition::SAVE_TO_DISK) {
        OpenDisposition::SaveToDisk
    } else {
        OpenDisposition::CurrentTab
    }
}

/// Where an allowed popup goes.
pub struct PopupPlace {
    /// Its creation, whose client the popup gets.
    pub creation: CreationId,
    /// Its window's content host.
    pub parent: NativeHandle,
    /// Where its page sits.
    pub bounds: PhysRect,
    /// The parent's scale.
    pub scale: f32,
}

/// The logic of `on_before_popup`: `None` blocks it. Blocked after Running, and when the shell says so.
pub fn before_popup(opener: BrowserId, popup_id: i32, url: &str, disposition: OpenDisposition, user_gesture: bool) -> Option<PopupPlace> {
    let (opener_tab, hooks) = registry::with(|r| {
        if !r.teardown.admits() {
            return None;
        }
        Some((r.tab_of(opener)?, r.hooks.clone()?))
    })
    .flatten()?;
    let url = Url::parse(url).ok()?;
    let decision = hooks.popup_policy(&PopupRequest { opener: opener_tab, url: url.clone(), disposition, user_gesture });
    let PopupDecision::AllowAsTab { tab, parent, bounds, scale_factor } = decision else { return None };
    let creation = registry::with(|r| {
        if !r.teardown.admits() {
            return None;
        }
        let id = r.next_creation();
        r.focus.entry(tab).or_insert(PageFocus::NEW_BROWSER);
        r.pending.insert(
            id,
            Pending {
                kind: BrowserKind::Tab(tab),
                browsing: r.entries.get(&opener).map(|e| e.browsing).unwrap_or_default(),
                style: Style::Chrome,
                counted: true,
                requested: std::time::Instant::now(),
                then: AfterCreate { popup: Some((opener, popup_id, opener_tab, url, disposition)), ..AfterCreate::default() },
            },
        );
        Some(id)
    })
    .flatten()?;
    Some(PopupPlace { creation, parent, bounds, scale: scale_factor })
}

/// The logic of `on_before_popup_aborted`: the allowed popup will never exist.
pub fn popup_aborted(opener: BrowserId, popup_id: i32) {
    registry::with(|r| {
        r.pending.retain(|_, p| !matches!(p.then.popup, Some((o, id, ..)) if o == opener && id == popup_id));
    });
    registry::check_teardown();
}

/// A DevTools window Chromium opens for a tab (Inspect): give it our client so its life span reaches
/// the registry. Counted from `on_after_created`.
pub fn devtools_creation(of: BrowserId) -> Option<CreationId> {
    registry::with(|r| {
        let tab = r.tab_of(of)?;
        let id = r.next_creation();
        r.pending.insert(
            id,
            Pending {
                kind: BrowserKind::DevTools(tab),
                browsing: Browsing::Normal,
                style: Style::Chrome,
                counted: false,
                requested: std::time::Instant::now(),
                then: AfterCreate::default(),
            },
        );
        Some(id)
    })
    .flatten()
}

wrap_life_span_handler! {
    pub struct LifeSpan {
        ctx: Arc<HandlerCtx>,
    }

    impl LifeSpanHandler {
        fn on_before_popup(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, popup_id: i32, target_url: Option<&CefString>,
                           _target_frame_name: Option<&CefString>, target_disposition: WindowOpenDisposition, user_gesture: i32,
                           _popup_features: Option<&PopupFeatures>, window_info: Option<&mut WindowInfo>, client: Option<&mut Option<Client>>,
                           _settings: Option<&mut BrowserSettings>, _extra_info: Option<&mut Option<DictionaryValue>>,
                           _no_javascript_access: Option<&mut i32>) -> i32 {
            let Some(browser) = browser else { return 1 };
            let disposition = disposition_of(i64::from(target_disposition.get_raw()));
            let place = before_popup(browser.identifier(), popup_id, &strings::text(target_url), disposition, user_gesture != 0);
            let (Some(place), Some(info), Some(client)) = (place, window_info, client) else { return 1 };
            crate::native::fix_popup(info, place.parent, place.bounds, place.scale);
            *client = Some(client::build(Some(place.creation), self.ctx.sink.clone()));
            0
        }

        fn on_before_popup_aborted(&self, browser: Option<&mut Browser>, popup_id: i32) {
            if let Some(browser) = browser {
                popup_aborted(browser.identifier(), popup_id);
            }
        }

        fn on_before_dev_tools_popup(&self, browser: Option<&mut Browser>, _window_info: Option<&mut WindowInfo>, client: Option<&mut Option<Client>>,
                                     _settings: Option<&mut BrowserSettings>, _extra_info: Option<&mut Option<DictionaryValue>>,
                                     _use_default_window: Option<&mut i32>) {
            let (Some(browser), Some(client)) = (browser, client) else { return };
            if let Some(creation) = devtools_creation(browser.identifier()) {
                *client = Some(client::build(Some(creation), self.ctx.sink.clone()));
            }
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else { return };
            let id = browser.identifier();
            let page: Rc<dyn BrowserOps> = Rc::new(CefBrowser::new(browser.clone(), self.ctx.sink.clone()));
            after_created(self.ctx.creation, page, id);
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> i32 {
            browser.map(|b| do_close(b.identifier()) as i32).unwrap_or(0)
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                before_close(browser.identifier());
            }
        }
    }
}

impl LifeSpan {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> LifeSpanHandler {
        LifeSpan::new(ctx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispositions_map() {
        let raw = |d: WindowOpenDisposition| i64::from(d.get_raw());
        assert_eq!(disposition_of(raw(WindowOpenDisposition::NEW_FOREGROUND_TAB)), OpenDisposition::NewForegroundTab);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::NEW_BACKGROUND_TAB)), OpenDisposition::NewBackgroundTab);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::NEW_POPUP)), OpenDisposition::NewPopup);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::NEW_WINDOW)), OpenDisposition::NewWindow);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::OFF_THE_RECORD)), OpenDisposition::OffTheRecord);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::SAVE_TO_DISK)), OpenDisposition::SaveToDisk);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::CURRENT_TAB)), OpenDisposition::CurrentTab);
        assert_eq!(disposition_of(raw(WindowOpenDisposition::SWITCH_TO_TAB)), OpenDisposition::CurrentTab);
    }
}
