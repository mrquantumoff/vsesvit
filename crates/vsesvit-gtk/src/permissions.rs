//! Site permissions: WebKit's requests and `navigator.permissions` queries, answered from
//! core's stored settings and each tab's one-time grants, and asked through a prompt bubble
//! on the address bar's site-info icon when neither decides. Also the capture WebKit reports
//! per tab, and the Permissions section of the site-info popover.
//!
//! WebKit's requests carry no origin, so a request is taken to come from the document on
//! screen: the tab's committed URI. `navigator.permissions.query` is answered by the same
//! rule, so a frame never reads a state its requests would not meet.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;
use url::Url;
use vsesvit_core::permissions::{self as core, Answer, Capturing, Decision, Origin, Permission, Prompt, Setting, SiteChoice, TabGrants};
use webkit::prelude::*;

use crate::browser::Browser;
use crate::tab::Tab;

/// What WebKit asks about, as far as the answer depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Geolocation,
    Notification,
    Clipboard,
    UserMedia { video: bool, audio: bool, display: bool },
    /// Device labels for `enumerateDevices`.
    DeviceInfo,
    PointerLock,
    Other,
}

/// How a request of a [`Kind`] is decided.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ask {
    /// From settings and grants, else by the user. One request, one prompt.
    Permissions(Vec<Permission>),
    /// Allowed when the site may use the camera or the microphone; never prompted for.
    DeviceInfo,
    Allow,
    Deny,
}

fn kind_of(request: &webkit::PermissionRequest) -> Kind {
    if request.is::<webkit::GeolocationPermissionRequest>() {
        return Kind::Geolocation;
    }
    if request.is::<webkit::NotificationPermissionRequest>() {
        return Kind::Notification;
    }
    if request.is::<webkit::ClipboardPermissionRequest>() {
        return Kind::Clipboard;
    }
    if let Some(media) = request.downcast_ref::<webkit::UserMediaPermissionRequest>() {
        return Kind::UserMedia {
            video: media.is_for_video_device(),
            audio: media.is_for_audio_device(),
            display: webkit::functions::user_media_permission_is_for_display_device(media),
        };
    }
    if request.is::<webkit::DeviceInfoPermissionRequest>() {
        return Kind::DeviceInfo;
    }
    if request.is::<webkit::PointerLockPermissionRequest>() {
        return Kind::PointerLock;
    }
    Kind::Other
}

fn ask_for(kind: Kind) -> Ask {
    match kind {
        Kind::Geolocation => Ask::Permissions(vec![Permission::Location]),
        Kind::Notification => Ask::Permissions(vec![Permission::Notifications]),
        Kind::Clipboard => Ask::Permissions(vec![Permission::ClipboardRead]),
        Kind::UserMedia { display: true, .. } => Ask::Permissions(vec![Permission::ScreenShare]),
        Kind::UserMedia { video, audio, .. } => {
            let devices = [(video, Permission::Camera), (audio, Permission::Microphone)];
            let asked: Vec<Permission> = devices.into_iter().filter(|(wanted, _)| *wanted).map(|(_, p)| p).collect();
            if asked.is_empty() { Ask::Deny } else { Ask::Permissions(asked) }
        }
        Kind::DeviceInfo => Ask::DeviceInfo,
        // Pointer lock is granted without asking, as in other browsers; Esc releases it.
        Kind::PointerLock => Ask::Allow,
        Kind::Other => Ask::Deny,
    }
}

/// The `navigator.permissions.query` names answered from settings; others read "prompt".
fn queried(name: &str) -> Option<Permission> {
    match name {
        "geolocation" => Some(Permission::Location),
        "notifications" => Some(Permission::Notifications),
        "camera" => Some(Permission::Camera),
        "microphone" => Some(Permission::Microphone),
        "clipboard-read" => Some(Permission::ClipboardRead),
        _ => None,
    }
}

fn state_of(decision: &Decision) -> webkit::PermissionState {
    match decision {
        Decision::Allow => webkit::PermissionState::Granted,
        Decision::Block => webkit::PermissionState::Denied,
        Decision::Ask(_) => webkit::PermissionState::Prompt,
    }
}

/// Requests a tab keeps waiting for the user at most; a page that asks for more is denied.
const MAX_WAITING: usize = 8;

/// A tab's side of site permissions: its one-time grants, the requests waiting for the user,
/// oldest first, and what the user turned down in the document on screen, which it is not
/// asked for again.
#[derive(Default)]
pub(crate) struct TabPermissions {
    grants: TabGrants,
    pending: VecDeque<Pending>,
    dismissed: Vec<Permission>,
}

struct Pending {
    request: webkit::PermissionRequest,
    permissions: Vec<Permission>,
    /// The page on screen when the request came; an answer holds only while its site is.
    asked_by: Option<String>,
}

/// The prompt a tab's oldest waiting request needs, and what it asks for (the permissions
/// of the request that nothing decides yet).
pub(crate) struct NextPrompt {
    pub(crate) request: webkit::PermissionRequest,
    pub(crate) asked: Vec<Permission>,
    pub(crate) prompt: Prompt,
}

/// Handles `permission-request`. Always returns true: every request gets an answer, now or
/// once the user picks one.
pub(crate) fn handle(tab: &Tab, request: &webkit::PermissionRequest) -> bool {
    let Some(window) = tab.window() else {
        return settle(request, false);
    };
    let browser = window.browser();
    let asked_by = requesting_document(tab);
    let origin = origin_of(asked_by.as_deref());
    let permissions = match ask_for(kind_of(request)) {
        Ask::Allow => return settle(request, true),
        Ask::Deny => return settle(request, false),
        Ask::DeviceInfo => {
            let devices = [Permission::Camera, Permission::Microphone];
            let may = devices.iter().any(|&p| decide(browser, tab, origin.as_ref(), &[p]) == Decision::Allow);
            return settle(request, may);
        }
        Ask::Permissions(permissions) => permissions,
    };
    match decide_request(browser, tab, origin.as_ref(), &permissions) {
        Decision::Allow => settle(request, true),
        Decision::Block => settle(request, false),
        Decision::Ask(_) if tab.permissions().borrow().pending.len() >= MAX_WAITING => settle(request, false),
        Decision::Ask(_) => {
            tab.permissions().borrow_mut().pending.push_back(Pending { request: request.clone(), permissions, asked_by });
            window.sync_permission_prompt();
            true
        }
    }
}

fn settle(request: &webkit::PermissionRequest, allow: bool) -> bool {
    if allow {
        request.allow();
    } else {
        request.deny();
    }
    true
}

/// Handles `query-permission-state`, from the same settings and grants a request would meet.
pub(crate) fn query(tab: &Tab, query: &webkit::PermissionStateQuery) -> bool {
    let permission = query.name().as_deref().and_then(queried);
    let state = match (permission, tab.window()) {
        (Some(permission), Some(window)) => {
            let origin = origin_of(requesting_document(tab).as_deref());
            state_of(&decide_request(window.browser(), tab, origin.as_ref(), &[permission]))
        }
        _ => webkit::PermissionState::Prompt,
    };
    query.finish(state);
    true
}

/// Whether a notification from `tab`'s page may show. A web process keeps the notification
/// permissions it was started with (see [`seed_notifications`]), so a later block or reset
/// is enforced here.
pub(crate) fn notification_allowed(tab: &Tab) -> bool {
    let Some(window) = tab.window() else { return false };
    let origin = origin_of(tab.committed_uri().as_deref());
    decide(window.browser(), tab, origin.as_ref(), &[Permission::Notifications]) == Decision::Allow
}

/// Hands WebKit the stored notification settings, which every web process it starts from
/// now on reads for `Notification.permission`. One already running keeps what it had, even
/// across a reload. Done at startup and after each change rather than from WebKit's
/// `initialize-notification-permissions`, which fires inside WebKit calls the shell may make
/// while it holds the profile.
pub(crate) fn seed_notifications(browser: &Browser) {
    let settings = browser.core().borrow_mut().site_permissions().all();
    let origins = |wanted: Setting| -> Vec<webkit::SecurityOrigin> {
        settings
            .iter()
            .filter(|s| s.permission == Permission::Notifications && s.setting == wanted)
            .map(|s| webkit::SecurityOrigin::for_uri(s.origin.as_str()))
            .collect()
    };
    let (allowed, blocked) = (origins(Setting::Allow), origins(Setting::Block));
    let allowed: Vec<&webkit::SecurityOrigin> = allowed.iter().collect();
    let blocked: Vec<&webkit::SecurityOrigin> = blocked.iter().collect();
    browser.runtime().web_context().initialize_notification_permissions(&allowed, &blocked);
}

/// The next prompt `tab` needs. Waiting requests that settings, grants or a dismissal now
/// decide (an earlier prompt's answer, a change in site info) are answered on the way.
pub(crate) fn next_prompt(browser: &Browser, tab: &Tab) -> Option<NextPrompt> {
    loop {
        let (request, permissions, origin) = {
            let state = tab.permissions().borrow();
            let head = state.pending.front()?;
            (head.request.clone(), head.permissions.clone(), origin_of(head.asked_by.as_deref()))
        };
        match decide_request(browser, tab, origin.as_ref(), &permissions) {
            Decision::Ask(asked) => return Some(NextPrompt { prompt: core::prompt(origin.as_ref(), &asked), request, asked }),
            decided => {
                tab.permissions().borrow_mut().pending.pop_front();
                settle(&request, decided == Decision::Allow);
            }
        }
    }
}

/// Applies the button pressed (Dismiss for a closed bubble) to `request`, if `tab` still
/// waits on it. `asked` is what the prompt asked for. The caller then runs [`enforce`], since
/// a stored answer can decide other tabs' requests.
pub(crate) fn answer(browser: &Browser, tab: &Tab, request: &webkit::PermissionRequest, asked: &[Permission], answer: Answer) {
    let pending = {
        let mut state = tab.permissions().borrow_mut();
        let at = state.pending.iter().position(|p| p.request == *request);
        at.and_then(|at| state.pending.remove(at))
    };
    let Some(pending) = pending else { return };
    let origin = origin_of(pending.asked_by.as_deref());
    let granted = {
        let mut state = tab.permissions().borrow_mut();
        if matches!(answer, Answer::Dismiss | Answer::NeverAllow) {
            state.dismissed.extend_from_slice(asked);
        }
        browser.core().borrow_mut().site_permissions().answer(origin.as_ref(), asked, answer, &mut state.grants)
    };
    let granted = granted.unwrap_or_else(|e| {
        log::warn!("site permissions: {e}");
        false
    });
    settle(&pending.request, holds(granted, pending.asked_by.as_deref(), tab.committed_uri().as_deref()));
}

/// Every committed navigation: grants end when the tab leaves their site, the requests of the
/// page it left are denied, and a new document may ask for what the last one was refused. A
/// load is in flight as a new document commits, and none for a same-document navigation.
pub(crate) fn committed(tab: &Tab) {
    let Some(uri) = tab.committed_uri() else { return };
    let left: VecDeque<Pending> = {
        let mut state = tab.permissions().borrow_mut();
        if let Ok(url) = Url::parse(&uri) {
            state.grants.committed(&url);
        }
        if tab.web_view().is_loading() {
            state.dismissed.clear();
        }
        let (stay, left) = state.pending.drain(..).partition(|p| site_of(p.asked_by.as_deref()) == site_of(Some(&uri)));
        state.pending = stay;
        left
    };
    for pending in left {
        pending.request.deny();
    }
}

/// A closed tab's waiting requests are denied.
pub(crate) fn closed(tab: &Tab) {
    let left: Vec<Pending> = tab.permissions().borrow_mut().pending.drain(..).collect();
    for pending in left {
        pending.request.deny();
    }
}

/// Brings the engine and every tab in line with the settings after a change: new web
/// processes get the notification settings, a capture that is no longer allowed stops, and
/// waiting prompts the change decides are answered.
pub(crate) fn enforce(browser: &Browser) {
    seed_notifications(browser);
    for window in browser.windows() {
        for tab in window.tabs() {
            let origin = origin_of(tab.committed_uri().as_deref());
            let now = capturing(tab.web_view());
            for permission in Permission::ALL.iter().copied().filter(|&p| now.uses(p)) {
                if ends(permission, &decide(browser, &tab, origin.as_ref(), &[permission])) {
                    stop(tab.web_view(), permission);
                }
            }
        }
        window.sync_permission_prompt();
    }
}

/// Whether a live capture under `permission` must end now. A screen share is allowed share
/// by share and never decides as Allow, so only a block ends it.
fn ends(permission: Permission, decision: &Decision) -> bool {
    match decision {
        Decision::Allow => false,
        Decision::Block => true,
        Decision::Ask(_) => permission.remembers_allow(),
    }
}

fn decide(browser: &Browser, tab: &Tab, origin: Option<&Origin>, permissions: &[Permission]) -> Decision {
    let state = tab.permissions().borrow();
    browser.core().borrow_mut().site_permissions().decide(origin, permissions, &state.grants)
}

/// [`decide`] for a request of the page on screen, which is refused what the user turned down
/// in this document rather than asked again.
fn decide_request(browser: &Browser, tab: &Tab, origin: Option<&Origin>, permissions: &[Permission]) -> Decision {
    match decide(browser, tab, origin, permissions) {
        Decision::Ask(asked) if asked.iter().all(|p| tab.permissions().borrow().dismissed.contains(p)) => Decision::Block,
        decision => decision,
    }
}

/// The page asking is the one on screen. During a provisional load the web view's URI is
/// already the one being requested, which has not run anything yet.
fn requesting_document(tab: &Tab) -> Option<String> {
    tab.committed_uri()
}

fn origin_of(uri: Option<&str>) -> Option<Origin> {
    Origin::parse(uri?)
}

/// An Allow counts only while the site the prompt named is still the one on screen.
fn holds(granted: bool, asked_by: Option<&str>, on_screen: Option<&str>) -> bool {
    granted && asked_by.is_some() && site_of(asked_by) == site_of(on_screen)
}

fn site_of(uri: Option<&str>) -> Option<(String, Option<String>, Option<u16>)> {
    let url = Url::parse(uri?).ok()?;
    Some((url.scheme().to_owned(), url.host_str().map(str::to_owned), url.port_or_known_default()))
}

// Capture.

pub(crate) fn capturing(web_view: &webkit::WebView) -> Capturing {
    let live = |state: webkit::MediaCaptureState| matches!(state, webkit::MediaCaptureState::Active | webkit::MediaCaptureState::Muted);
    Capturing {
        camera: live(web_view.camera_capture_state()),
        microphone: live(web_view.microphone_capture_state()),
        screen: live(web_view.display_capture_state()),
    }
}

pub(crate) fn stop(web_view: &webkit::WebView, permission: Permission) {
    let none = webkit::MediaCaptureState::None;
    match permission {
        Permission::Camera => web_view.set_camera_capture_state(none),
        Permission::Microphone => web_view.set_microphone_capture_state(none),
        Permission::ScreenShare => web_view.set_display_capture_state(none),
        _ => {}
    }
}

/// The in-use icon on a tab, and its tooltip.
pub(crate) fn indicator(capturing: Capturing) -> Option<(&'static str, String)> {
    let description = capturing.description()?;
    let icon = if capturing.screen {
        "screen-shared-symbolic"
    } else if capturing.camera {
        "camera-web-symbolic"
    } else {
        "audio-input-microphone-symbolic"
    };
    Some((icon, description))
}

/// The bar over a page that shares the screen.
pub(crate) fn sharing_title(uri: Option<&str>) -> String {
    let site = origin_of(uri).map_or_else(|| "this page".to_owned(), |o| o.host_for_display());
    format!("Sharing your screen with {site}")
}

// The prompt bubble.

/// How long after the prompt shows its buttons ignore activation, so that a click or a key
/// meant for the page cannot answer it (as Chrome guards its prompts).
pub(crate) const PROMPT_GUARD: Duration = Duration::from_millis(500);

/// The heading, the body and a button per answer in order, the first one suggested.
/// `on_answer` gets the button pressed.
///
/// The bubble takes no grab and no focus: the page or the address bar keeps the keyboard, so
/// typing goes on where it was, and only a deliberate click answers.
pub(crate) fn prompt_popover(prompt: &Prompt, on_answer: impl Fn(Answer) + 'static) -> gtk::Popover {
    let on_answer = Rc::new(on_answer);
    let shown_at: Rc<Cell<Option<Instant>>> = Rc::default();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .width_request(320)
        .build();
    content.append(&wrapped(&prompt.heading, &["heading"]));
    content.append(&wrapped(&prompt.body, &[]));
    let buttons = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for (i, &answer) in prompt.answers.iter().enumerate() {
        let button = gtk::Button::with_label(answer.label());
        if i == 0 {
            button.add_css_class("suggested-action");
        }
        let (on_answer, shown_at) = (on_answer.clone(), shown_at.clone());
        button.connect_clicked(move |_| {
            if shown_at.get().is_some_and(|at| at.elapsed() >= PROMPT_GUARD) {
                on_answer(answer);
            }
        });
        buttons.append(&button);
    }
    content.append(&buttons);
    let popover = gtk::Popover::builder().child(&content).autohide(false).css_classes(["permission-prompt"]).build();
    popover.connect_map(move |_| shown_at.set(Some(Instant::now())));
    popover
}

fn wrapped(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).max_width_chars(40).css_classes(classes.to_vec()).build()
}

// The site-info section.

/// The Permissions section for `tab`'s page: a row for every permission stored for its
/// site, granted this time or in use. `None` when there is none.
pub(crate) fn site_info_section(browser: &Browser, tab: &Tab) -> Option<gtk::Box> {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    fill_section(&section, browser, tab);
    section.first_child().is_some().then_some(section)
}

fn fill_section(section: &gtk::Box, browser: &Browser, tab: &Tab) {
    while let Some(child) = section.first_child() {
        section.remove(&child);
    }
    let origin = origin_of(tab.committed_uri().as_deref());
    let stored = origin.as_ref().map(|o| browser.core().borrow_mut().site_permissions().for_site(o)).unwrap_or_default();
    let granted: Vec<Permission> = {
        let state = tab.permissions().borrow();
        state.grants.granted().filter(|&p| state.grants.allows(origin.as_ref(), p)).collect()
    };
    let rows = core::site_rows(origin.is_some(), &stored, &granted, capturing(tab.web_view()));
    if rows.is_empty() {
        return;
    }
    section.append(&gtk::Label::builder().label("Permissions").xalign(0.0).css_classes(["heading"]).build());
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    for site_row in rows {
        let row = permission_row(section, browser, tab, origin.as_ref(), site_row.permission, &site_row.choices, site_row.current);
        if site_row.live {
            row.add_suffix(&stop_button(tab, site_row.permission));
        }
        list.append(&row);
    }
    section.append(&list);
    if let Some(origin) = origin.filter(|_| !stored.is_empty()) {
        let reset = gtk::Button::builder().label("Reset permissions").halign(gtk::Align::Start).build();
        reset.connect_clicked(glib::clone!(
            #[weak]
            section,
            #[strong]
            browser,
            #[weak]
            tab,
            move |_| {
                reset_site(&browser, &tab, &origin);
                changed(&section, &browser, &tab);
            }
        ));
        section.append(&reset);
    }
}

/// "Reset permissions": every setting of the site back to Ask, and this tab's grants ended, a
/// screen share allowed this time with them.
fn reset_site(browser: &Browser, tab: &Tab, origin: &Origin) {
    tab.permissions().borrow_mut().grants = TabGrants::default();
    if capturing(tab.web_view()).screen {
        stop(tab.web_view(), Permission::ScreenShare);
    }
    if let Err(e) = browser.core().borrow_mut().site_permissions().reset_site(origin) {
        log::warn!("site permissions: {e}");
    }
}

fn permission_row(
    section: &gtk::Box,
    browser: &Browser,
    tab: &Tab,
    origin: Option<&Origin>,
    permission: Permission,
    choices: &[SiteChoice],
    current: SiteChoice,
) -> adw::ComboRow {
    let labels: Vec<&str> = choices.iter().map(|c| c.label()).collect();
    let row = adw::ComboRow::builder()
        .title(permission.label())
        .model(&gtk::StringList::new(&labels))
        .selected(choices.iter().position(|c| *c == current).and_then(|i| u32::try_from(i).ok()).unwrap_or(0))
        .build();
    let choices = choices.to_vec();
    let origin = origin.cloned();
    row.connect_selected_notify(glib::clone!(
        #[weak]
        section,
        #[strong]
        browser,
        #[weak]
        tab,
        move |row| {
            let Some(&choice) = choices.get(row.selected() as usize) else { return };
            choose(&browser, &tab, origin.as_ref(), permission, choice);
            changed(&section, &browser, &tab);
        }
    ));
    row
}

fn stop_button(tab: &Tab, permission: Permission) -> gtk::Button {
    let button = gtk::Button::builder().label("Stop").valign(gtk::Align::Center).build();
    button.connect_clicked(glib::clone!(
        #[weak]
        tab,
        move |button| {
            stop(tab.web_view(), permission);
            button.set_visible(false);
        }
    ));
    button
}

fn choose(browser: &Browser, tab: &Tab, origin: Option<&Origin>, permission: Permission, choice: SiteChoice) {
    if choice == SiteChoice::AllowedThisTime {
        return;
    }
    let setting = choice.setting();
    if setting != Some(Setting::Allow) {
        tab.permissions().borrow_mut().grants.revoke(permission);
        // A screen share has no grant to end, so `enforce` would let it go on.
        if !permission.remembers_allow() && capturing(tab.web_view()).uses(permission) {
            stop(tab.web_view(), permission);
        }
    }
    if let Some(origin) = origin
        && let Err(e) = browser.core().borrow_mut().site_permissions().set(origin, permission, setting)
    {
        log::warn!("site permissions: {e}");
    }
}

/// After a change from the section: every tab follows it, and the section shows it once the
/// row that changed has finished emitting.
fn changed(section: &gtk::Box, browser: &Browser, tab: &Tab) {
    enforce(browser);
    glib::idle_add_local_once(glib::clone!(
        #[weak]
        section,
        #[strong]
        browser,
        #[weak]
        tab,
        move || fill_section(&section, &browser, &tab)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_ask_for_what_they_would_use() {
        let media = |video, audio, display| ask_for(Kind::UserMedia { video, audio, display });
        assert_eq!(ask_for(Kind::Geolocation), Ask::Permissions(vec![Permission::Location]));
        assert_eq!(ask_for(Kind::Notification), Ask::Permissions(vec![Permission::Notifications]));
        assert_eq!(ask_for(Kind::Clipboard), Ask::Permissions(vec![Permission::ClipboardRead]));
        assert_eq!(media(true, true, false), Ask::Permissions(vec![Permission::Camera, Permission::Microphone]), "one request, one prompt");
        assert_eq!(media(true, false, false), Ask::Permissions(vec![Permission::Camera]));
        assert_eq!(media(false, true, false), Ask::Permissions(vec![Permission::Microphone]));
        assert_eq!(media(true, true, true), Ask::Permissions(vec![Permission::ScreenShare]));
        assert_eq!(ask_for(Kind::DeviceInfo), Ask::DeviceInfo);
        assert_eq!(ask_for(Kind::PointerLock), Ask::Allow);
        assert_eq!(ask_for(Kind::Other), Ask::Deny);
    }

    #[test]
    fn queries_read_the_decision() {
        assert_eq!(queried("geolocation"), Some(Permission::Location));
        assert_eq!(queried("clipboard-read"), Some(Permission::ClipboardRead));
        assert_eq!(queried("midi"), None);
        assert_eq!(state_of(&Decision::Allow), webkit::PermissionState::Granted);
        assert_eq!(state_of(&Decision::Block), webkit::PermissionState::Denied);
        assert_eq!(state_of(&Decision::Ask(vec![Permission::Camera])), webkit::PermissionState::Prompt);
    }

    #[test]
    fn an_answer_holds_only_for_the_site_it_was_asked_for() {
        assert!(holds(true, Some("https://a.example/x"), Some("https://a.example/y#z")));
        assert!(holds(true, Some("file:///tmp/a.html"), Some("file:///tmp/a.html")));
        assert!(!holds(true, Some("https://a.example/"), Some("https://b.example/")));
        assert!(!holds(true, Some("https://a.example/"), Some("http://a.example/")));
        assert!(!holds(true, Some("https://a.example/"), None));
        assert!(!holds(false, Some("https://a.example/"), Some("https://a.example/")));
    }

    #[test]
    fn only_a_block_ends_a_live_screen_share() {
        let ask = |p| Decision::Ask(vec![p]);
        assert!(!ends(Permission::ScreenShare, &ask(Permission::ScreenShare)), "a share allowed this time goes on");
        assert!(ends(Permission::ScreenShare, &Decision::Block));
        assert!(ends(Permission::Camera, &ask(Permission::Camera)), "a camera grant that ended stops the camera");
        assert!(!ends(Permission::Camera, &Decision::Allow));
    }

    #[test]
    fn indicators_name_the_most_visible_capture() {
        let capturing = |camera, microphone, screen| Capturing { camera, microphone, screen };
        assert_eq!(indicator(capturing(false, false, false)), None);
        assert_eq!(indicator(capturing(true, true, false)), Some(("camera-web-symbolic", "Using your camera and microphone".to_owned())));
        assert_eq!(indicator(capturing(false, true, false)).map(|i| i.0), Some("audio-input-microphone-symbolic"));
        assert_eq!(indicator(capturing(true, false, true)).map(|i| i.0), Some("screen-shared-symbolic"));
        assert_eq!(sharing_title(Some("http://localhost:8080/call")), "Sharing your screen with localhost:8080");
        assert_eq!(sharing_title(Some("about:blank")), "Sharing your screen with this page");
    }

    mod queue {
        use std::cell::RefCell;
        use std::rc::Rc;

        use super::*;
        use crate::test_support::{Reply, Server, browser, settle, wait_until};
        use crate::window::{BrowserWindow, Focus};

        const LOCATION: &str = "navigator.geolocation.getCurrentPosition(() => document.title = 'location:ok', e => document.title = 'location:' + e.code); 'asked'";
        const MICROPHONE: &str = "navigator.mediaDevices.getUserMedia({audio: true}).then(s => { window.stream = s; document.title = 'microphone:ok'; }, e => document.title = 'microphone:' + e.name); 'asked'";

        /// Popovers behave as for a user: the site-info bubble hides on a click outside.
        fn setup() -> (Server, BrowserWindow) {
            crate::SCRIPTED.set(false);
            let browser = browser();
            browser.engine().settings().set_enable_mock_capture_devices(true);
            let server = Server::start("127.0.0.1", |_| Reply::Page("Asking"));
            let window = BrowserWindow::new(&browser);
            window.present();
            (server, window)
        }

        fn open(window: &BrowserWindow, url: &str, focus: Focus) -> Tab {
            let tab = window.open_tab(Some(url), None, focus);
            wait_until("the page to load", || tab.committed_uri().as_deref() == Some(url) && !tab.web_view().is_loading());
            tab
        }

        /// Runs `script` in the tab's page and returns its value as a string.
        fn run(tab: &Tab, script: &str) -> String {
            let result = Rc::new(RefCell::new(None));
            let slot = result.clone();
            tab.web_view().evaluate_javascript(script, None, None, None::<&gtk::gio::Cancellable>, move |value| {
                slot.replace(Some(value.map_or_else(|e| e.to_string(), |v| v.to_str().to_string())));
            });
            wait_until("the script to run", || result.borrow().is_some());
            result.take().unwrap_or_default()
        }

        fn waiting(tab: &Tab) -> usize {
            tab.permissions().borrow().pending.len()
        }

        fn title(tab: &Tab) -> String {
            tab.web_view().title().map(String::from).unwrap_or_default()
        }

        fn widgets<W: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<W> {
            let mut found: Vec<W> = root.downcast_ref::<W>().cloned().into_iter().collect();
            let mut child = root.first_child();
            while let Some(c) = child {
                found.extend(widgets(&c));
                child = c.next_sibling();
            }
            found
        }

        /// The heading of the permission prompt on screen, if one is.
        fn prompt(window: &BrowserWindow) -> Option<String> {
            let bubble = window.address_bar().prompt()?;
            widgets::<gtk::Label>(bubble.upcast_ref()).into_iter().find(|l| l.has_css_class("heading")).map(|l| l.label().into())
        }

        fn button(window: &BrowserWindow, answer: Answer) -> gtk::Button {
            let bubble = window.address_bar().prompt().expect("a prompt is shown");
            let button = widgets::<gtk::Button>(bubble.upcast_ref()).into_iter().find(|b| b.label().as_deref() == Some(answer.label()));
            button.expect("the prompt offers the answer")
        }

        /// Clicks `answer` once the prompt takes clicks.
        fn press(window: &BrowserWindow, answer: Answer) {
            settle(PROMPT_GUARD);
            button(window, answer).emit_clicked();
        }

        /// A click in the window, outside the prompt, as its click gesture sees it.
        fn click_in_window(window: &BrowserWindow) {
            let controllers = window.observe_controllers();
            let clicks = (0..controllers.n_items())
                .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
                .find(|g| g.propagation_phase() == gtk::PropagationPhase::Capture && g.button() == 0)
                .expect("the window watches clicks");
            clicks.emit_by_name::<()>("released", &[&1i32, &10.0f64, &10.0f64]);
        }

        #[gtk::test]
        fn prompts_show_one_at_a_time_for_the_selected_tab() {
            let (server, window) = setup();
            let front = open(&window, &server.url("/front"), Focus::Foreground);
            let back = open(&window, &server.url("/back"), Focus::Background);

            run(&front, LOCATION);
            wait_until("the prompt", || prompt(&window).is_some());
            let first = prompt(&window);

            window.select_tab(&back);
            let after_switching = (prompt(&window), waiting(&front));
            run(&back, MICROPHONE);
            wait_until("the other tab's prompt", || prompt(&window).is_some());
            let other = prompt(&window);
            press(&window, Answer::Dismiss);
            wait_until("the dismissed request to fail", || title(&back) == "microphone:NotAllowedError");
            let after_dismissing = prompt(&window);

            window.select_tab(&front);
            let back_again = prompt(&window);
            run(&front, MICROPHONE);
            wait_until("a second request in the tab", || waiting(&front) == 2);
            let while_one_shows = prompt(&window);
            press(&window, Answer::AllowThisTime);
            wait_until("the answered request to go", || waiting(&front) == 1);
            let next = prompt(&window);

            window.close_tab(&front);
            wait_until("the closed tab's request to go", || waiting(&front) == 0);
            let after_closing = prompt(&window);
            window.destroy();

            assert_eq!(first.as_deref(), Some("Know your location?"));
            assert_eq!(after_switching, (None, 1), "a tab's prompt waits while another tab is selected");
            assert_eq!(other.as_deref(), Some("Use your microphone?"));
            assert_eq!(after_dismissing, None);
            assert_eq!(back_again, first, "the prompt comes back with its tab");
            assert_eq!(while_one_shows, first, "one prompt at a time, oldest first");
            assert_eq!(next.as_deref(), Some("Use your microphone?"), "the next prompt follows an answer");
            assert_eq!(after_closing, None);
        }

        #[gtk::test]
        fn the_prompt_leaves_the_keyboard_where_it_was_and_ignores_early_clicks() {
            let (server, window) = setup();
            let tab = open(&window, &server.url("/"), Focus::Foreground);
            window.address_bar().focus_for_typing();
            wait_until("the address bar to take the focus", || {
                gtk::prelude::RootExt::focus(&window).is_some_and(|f| f.is_ancestor(window.address_bar()))
            });
            run(&tab, MICROPHONE);
            wait_until("the prompt", || prompt(&window).is_some());
            settle(std::time::Duration::from_millis(100));
            let focus_in_bar = gtk::prelude::RootExt::focus(&window).is_some_and(|f| f.is_ancestor(window.address_bar()));
            let autohide = window.address_bar().prompt().is_some_and(|p| p.is_autohide());
            button(&window, Answer::AllowWhileVisiting).emit_clicked();
            let after_early_click = (waiting(&tab), prompt(&window).is_some());
            press(&window, Answer::AllowThisTime);
            wait_until("the microphone to open", || title(&tab) == "microphone:ok");
            window.destroy();

            assert!(focus_in_bar, "typing goes on in the address bar");
            assert!(!autohide, "the prompt takes no grab");
            assert_eq!(after_early_click, (1, true), "a click right as the prompt shows does not answer it");
        }

        #[gtk::test]
        fn another_bubble_or_a_tab_switch_withdraws_the_prompt_and_a_click_on_the_page_dismisses_it() {
            let (server, window) = setup();
            let tab = open(&window, &server.url("/asking"), Focus::Foreground);
            let other = open(&window, &server.url("/other"), Focus::Background);
            run(&tab, MICROPHONE);
            wait_until("the prompt", || prompt(&window).is_some());

            window.show_site_info();
            let site_info = window.address_bar().bubble().expect("site info opens");
            let under_site_info = (prompt(&window), waiting(&tab));
            site_info.popdown();
            wait_until("the prompt to come back", || prompt(&window).is_some());

            click_in_window(&window);
            window.select_tab(&other);
            wait_until("the click to be handled", || !glib::MainContext::default().pending());
            let after_switching = (prompt(&window), waiting(&tab));
            window.select_tab(&tab);
            let back = prompt(&window);

            click_in_window(&window);
            wait_until("the dismissed request to fail", || title(&tab) == "microphone:NotAllowedError");
            let after_click = (prompt(&window), waiting(&tab));

            run(&tab, LOCATION);
            wait_until("the next prompt", || prompt(&window).is_some());
            window.address_bar().prompt().expect("a prompt is shown").popdown();
            wait_until("the closed prompt to count as Not now", || waiting(&tab) == 0);
            window.destroy();

            assert_eq!(under_site_info, (None, 1), "site info stays open while the request waits");
            assert_eq!(after_switching, (None, 1), "a click that switches tabs withdraws the prompt");
            assert_eq!(back.as_deref(), Some("Use your microphone?"));
            assert_eq!(after_click, (None, 0), "a click on the page is Not now");
        }

        #[gtk::test]
        fn a_dismissed_prompt_is_not_asked_again_by_the_same_document() {
            let (server, window) = setup();
            let tab = open(&window, &server.url("/"), Focus::Foreground);
            run(&tab, MICROPHONE);
            wait_until("the prompt", || prompt(&window).is_some());
            press(&window, Answer::Dismiss);
            wait_until("the dismissed request to fail", || title(&tab) == "microphone:NotAllowedError");
            run(&tab, "document.title = 'again'; history.pushState(null, '', '/moved'); 'reset'");
            run(&tab, MICROPHONE);
            wait_until("the repeat to fail", || title(&tab) == "microphone:NotAllowedError");
            let asked_again = (prompt(&window), waiting(&tab));
            tab.load(&server.url("/"));
            wait_until("the next document", || tab.committed_uri().is_some_and(|u| !u.ends_with("/moved")) && !tab.web_view().is_loading());
            run(&tab, MICROPHONE);
            wait_until("a new document to ask", || prompt(&window).is_some());
            window.destroy();
            assert_eq!(asked_again, (None, 0), "the same page is not asked again");
        }

        #[gtk::test]
        fn a_reset_ends_the_tabs_one_time_grants() {
            let (server, window) = setup();
            let tab = open(&window, &server.url("/"), Focus::Foreground);
            let origin = Origin::parse(&server.url("/")).expect("an http origin");
            let browser = window.browser().clone();
            {
                let mut profile = browser.core().borrow_mut();
                let mut site = profile.site_permissions();
                site.set(&origin, Permission::Location, Some(Setting::Block)).unwrap();
                site.answer(Some(&origin), &[Permission::Camera], Answer::AllowThisTime, &mut tab.permissions().borrow_mut().grants).unwrap();
            }
            reset_site(&browser, &tab, &origin);
            let camera = decide(&browser, &tab, Some(&origin), &[Permission::Camera]);
            let stored = browser.core().borrow_mut().site_permissions().for_site(&origin);
            window.destroy();
            assert_eq!(camera, Decision::Ask(vec![Permission::Camera]), "the camera allowed this time is asked for again");
            assert_eq!(stored, []);
        }

        #[gtk::test]
        fn a_kept_answer_settles_the_same_prompt_in_another_window() {
            let (server, window) = setup();
            let other_window = BrowserWindow::new(window.browser());
            other_window.present();
            let first = open(&window, &server.url("/one"), Focus::Foreground);
            let second = open(&other_window, &server.url("/two"), Focus::Foreground);
            run(&first, MICROPHONE);
            run(&second, MICROPHONE);
            wait_until("a prompt in each window", || prompt(&window).is_some() && prompt(&other_window).is_some());
            press(&other_window, Answer::AllowWhileVisiting);
            wait_until("both pages to get the microphone", || title(&first) == "microphone:ok" && title(&second) == "microphone:ok");
            let left = prompt(&window);
            let origin = Origin::parse(&server.url("/")).expect("an http origin");
            window.browser().core().borrow_mut().site_permissions().reset_site(&origin).unwrap();
            window.destroy();
            other_window.destroy();
            assert_eq!(left, None);
        }

        #[gtk::test]
        fn a_prompt_waits_out_fullscreen() {
            let (server, window) = setup();
            let tab = open(&window, &server.url("/"), Focus::Foreground);
            window.fullscreen();
            wait_until("fullscreen", || window.is_fullscreen());
            run(&tab, MICROPHONE);
            wait_until("the request", || waiting(&tab) == 1);
            let while_fullscreen = prompt(&window);
            window.unfullscreen();
            wait_until("the prompt after fullscreen", || prompt(&window).is_some());
            window.destroy();
            assert_eq!(while_fullscreen, None, "the address bar is hidden");
        }

        #[gtk::test]
        fn a_kept_answer_decides_the_next_request_and_leaving_the_site_denies() {
            let (server, window) = setup();
            let other = Server::start("127.0.0.2", |_| Reply::Page("Elsewhere"));
            let tab = open(&window, &server.url("/"), Focus::Foreground);
            let origin = Origin::parse(&server.url("/")).expect("an http origin");

            run(&tab, MICROPHONE);
            wait_until("the prompt", || prompt(&window).is_some());
            press(&window, Answer::AllowWhileVisiting);
            wait_until("the microphone to open", || title(&tab) == "microphone:ok");
            let stored = window.browser().core().borrow_mut().site_permissions().get(&origin, Permission::Microphone);
            run(&tab, "document.title = 'again'; 'reset'");
            run(&tab, MICROPHONE);
            wait_until("the second request", || title(&tab) == "microphone:ok");
            let asked_again = prompt(&window);
            wait_until("the capture indicator", || tab.capturing().microphone);

            run(&tab, LOCATION);
            wait_until("the location prompt", || prompt(&window).is_some());
            tab.load(&other.url("/"));
            wait_until("the other site", || tab.committed_uri().is_some_and(|u| u.starts_with("http://127.0.0.2")));
            let left = (waiting(&tab), prompt(&window));
            window.browser().core().borrow_mut().site_permissions().reset_site(&origin).unwrap();
            window.destroy();

            assert_eq!(stored, Some(Setting::Allow));
            assert_eq!(asked_again, None, "an allowed site is not asked again");
            assert_eq!(left, (0, None), "leaving the site answers its requests");
        }

        #[gtk::test]
        fn stored_notification_settings_reach_new_pages() {
            let (allowed, window) = setup();
            let blocked = Server::start("127.0.0.3", |_| Reply::Page("Blocked"));
            let origins = [(&allowed, Setting::Allow), (&blocked, Setting::Block)].map(|(server, setting)| {
                let origin = Origin::parse(&server.url("/")).expect("an http origin");
                let mut profile = window.browser().core().borrow_mut();
                profile.site_permissions().set(&origin, Permission::Notifications, Some(setting)).unwrap();
                origin
            });
            enforce(window.browser());
            let permission = |url: &str| run(&open(&window, url, Focus::Foreground), "Notification.permission");
            let seen = (permission(&allowed.url("/")), permission(&blocked.url("/")));
            for origin in &origins {
                window.browser().core().borrow_mut().site_permissions().reset_site(origin).unwrap();
            }
            window.destroy();
            // WebKit reads a seeded block as "default".
            assert_eq!(seen, ("granted".to_owned(), "default".to_owned()));
        }
    }

    #[gtk::test]
    fn a_request_during_a_provisional_load_is_from_the_page_on_screen() {
        use crate::test_support::{Reply, Server, browser, wait_until};
        use crate::window::{BrowserWindow, Focus};

        let shown = Server::start("127.0.0.1", |_| Reply::Page("Shown"));
        let requested = Server::start("127.0.0.2", |_| Reply::Hang);
        let window = BrowserWindow::new(&browser());
        let tab = window.open_tab(Some(&shown.url("/")), None, Focus::Foreground);
        wait_until("the first page to commit", || tab.committed_uri().is_some());
        let pending = requested.url("/slow");
        tab.load(&pending);
        wait_until("the next load to start", || tab.web_view().uri().as_deref() == Some(pending.as_str()));
        let origin = origin_of(requesting_document(&tab).as_deref());
        window.destroy();
        assert_eq!(origin, Origin::parse(&shown.url("/")));
    }
}
