//! Site permissions: WebKit's requests and `navigator.permissions` queries, answered from
//! core's stored settings and each tab's one-time grants, and asked through a prompt bubble
//! on the address bar's site-info icon when neither decides. Also the capture WebKit reports
//! per tab, and the Permissions section of the site-info popover.
//!
//! WebKit's requests carry no origin, so a request is taken to come from the document on
//! screen: the tab's committed URI.

use std::collections::VecDeque;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use url::Url;
use vsesvit_core::permissions::{self as core, Answer, Capturing, Decision, Origin, Permission, Prompt, Setting, TabGrants};
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

/// A tab's side of site permissions: its one-time grants, and the requests waiting for the
/// user, oldest first.
#[derive(Default)]
pub(crate) struct TabPermissions {
    grants: TabGrants,
    pending: VecDeque<Pending>,
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
    match decide(browser, tab, origin.as_ref(), &permissions) {
        Decision::Allow => settle(request, true),
        Decision::Block => settle(request, false),
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
            let origin = query.security_origin().and_then(|o| Origin::parse(&o.to_str()));
            state_of(&decide(window.browser(), tab, origin.as_ref(), &[permission]))
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
/// now on reads for `Notification.permission`; one already running keeps what it had. Done
/// at startup and after each change rather than from WebKit's
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

/// The next prompt `tab` needs. Waiting requests that settings or grants now decide (an
/// earlier prompt's answer, a change in site info) are answered on the way.
pub(crate) fn next_prompt(browser: &Browser, tab: &Tab) -> Option<NextPrompt> {
    loop {
        let (request, permissions, origin) = {
            let state = tab.permissions().borrow();
            let head = state.pending.front()?;
            (head.request.clone(), head.permissions.clone(), origin_of(head.asked_by.as_deref()))
        };
        match decide(browser, tab, origin.as_ref(), &permissions) {
            Decision::Ask(asked) => return Some(NextPrompt { prompt: core::prompt(origin.as_ref(), &asked), request, asked }),
            decided => {
                tab.permissions().borrow_mut().pending.pop_front();
                settle(&request, decided == Decision::Allow);
            }
        }
    }
}

/// Applies the button pressed (Dismiss for a closed bubble) to `request`, if `tab` still
/// waits on it. `asked` is what the prompt asked for.
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
        browser.core().borrow_mut().site_permissions().answer(origin.as_ref(), asked, answer, &mut state.grants)
    };
    let granted = granted.unwrap_or_else(|e| {
        log::warn!("site permissions: {e}");
        false
    });
    seed_notifications(browser);
    settle(&pending.request, holds(granted, pending.asked_by.as_deref(), tab.committed_uri().as_deref()));
}

/// Every committed navigation: grants end when the tab leaves their site, and the requests
/// of the page it left are denied.
pub(crate) fn committed(tab: &Tab) {
    let Some(uri) = tab.committed_uri() else { return };
    let left: VecDeque<Pending> = {
        let mut state = tab.permissions().borrow_mut();
        if let Ok(url) = Url::parse(&uri) {
            state.grants.committed(&url);
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
            for permission in live(capturing(tab.web_view())) {
                if decide(browser, &tab, origin.as_ref(), &[permission]) != Decision::Allow {
                    stop(tab.web_view(), permission);
                }
            }
        }
        window.sync_permission_prompt();
    }
}

fn decide(browser: &Browser, tab: &Tab, origin: Option<&Origin>, permissions: &[Permission]) -> Decision {
    let state = tab.permissions().borrow();
    browser.core().borrow_mut().site_permissions().decide(origin, permissions, &state.grants)
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

/// The permission each live capture runs under.
fn live(capturing: Capturing) -> Vec<Permission> {
    [(capturing.camera, Permission::Camera), (capturing.microphone, Permission::Microphone), (capturing.screen, Permission::ScreenShare)]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, p)| p)
        .collect()
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

/// The heading, the body and a button per answer in order, the first one suggested.
/// `on_answer` gets the button pressed.
pub(crate) fn prompt_popover(prompt: &Prompt, on_answer: impl Fn(Answer) + 'static) -> gtk::Popover {
    let on_answer = Rc::new(on_answer);
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
        let on_answer = on_answer.clone();
        button.connect_clicked(move |_| on_answer(answer));
        buttons.append(&button);
    }
    content.append(&buttons);
    gtk::Popover::builder().child(&content).css_classes(["permission-prompt"]).build()
}

fn wrapped(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).max_width_chars(40).css_classes(classes.to_vec()).build()
}

// The site-info section.

/// A row's choice in the site-info popover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    /// A one-time grant of this tab, with nothing stored.
    ThisTime,
    Ask,
    Allow,
    Block,
}

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::ThisTime => "Allowed this time",
            Choice::Ask => "Ask",
            Choice::Allow => "Allow",
            Choice::Block => "Block",
        }
    }
}

/// A row's choices and the current one. Nothing is stored without an origin, and Allow is
/// never stored for what is asked every time.
fn choices(origin: Option<&Origin>, permission: Permission, stored: Option<Setting>, granted: bool) -> (Vec<Choice>, Choice) {
    let this_time = granted && stored.is_none();
    let mut choices = Vec::new();
    if this_time {
        choices.push(Choice::ThisTime);
    }
    choices.push(Choice::Ask);
    if origin.is_some() {
        if permission.remembers_allow() {
            choices.push(Choice::Allow);
        }
        choices.push(Choice::Block);
    }
    let current = match stored {
        Some(Setting::Allow) => Choice::Allow,
        Some(Setting::Block) => Choice::Block,
        None if this_time => Choice::ThisTime,
        None => Choice::Ask,
    };
    (choices, current)
}

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
    let in_use = live(capturing(tab.web_view()));
    let shown: Vec<Permission> = Permission::ALL
        .iter()
        .copied()
        .filter(|p| stored.iter().any(|(q, _)| q == p) || granted.contains(p) || in_use.contains(p))
        .collect();
    if shown.is_empty() {
        return;
    }
    section.append(&gtk::Label::builder().label("Permissions").xalign(0.0).css_classes(["heading"]).build());
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    for permission in shown {
        let setting = stored.iter().find(|(p, _)| *p == permission).map(|(_, s)| *s);
        let (choices, current) = choices(origin.as_ref(), permission, setting, granted.contains(&permission));
        let row = permission_row(section, browser, tab, origin.as_ref(), permission, &choices, current);
        if in_use.contains(&permission) {
            row.add_suffix(&stop_button(tab, permission));
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
                if let Err(e) = browser.core().borrow_mut().site_permissions().reset_site(&origin) {
                    log::warn!("site permissions: {e}");
                }
                changed(&section, &browser, &tab);
            }
        ));
        section.append(&reset);
    }
}

fn permission_row(
    section: &gtk::Box,
    browser: &Browser,
    tab: &Tab,
    origin: Option<&Origin>,
    permission: Permission,
    choices: &[Choice],
    current: Choice,
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

fn choose(browser: &Browser, tab: &Tab, origin: Option<&Origin>, permission: Permission, choice: Choice) {
    let setting = match choice {
        Choice::ThisTime => return,
        Choice::Ask => None,
        Choice::Allow => Some(Setting::Allow),
        Choice::Block => Some(Setting::Block),
    };
    if setting != Some(Setting::Allow) {
        tab.permissions().borrow_mut().grants.revoke(permission);
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
    fn site_info_offers_what_can_be_kept() {
        use Choice::*;
        let site = Origin::parse("https://meet.example");
        let site = site.as_ref();
        assert_eq!(choices(site, Permission::Camera, None, false), (vec![Ask, Allow, Block], Ask));
        assert_eq!(choices(site, Permission::Camera, Some(Setting::Block), false), (vec![Ask, Allow, Block], Block));
        assert_eq!(choices(site, Permission::Camera, None, true), (vec![ThisTime, Ask, Allow, Block], ThisTime));
        assert_eq!(choices(site, Permission::Camera, Some(Setting::Allow), true), (vec![Ask, Allow, Block], Allow));
        assert_eq!(choices(site, Permission::ScreenShare, None, true), (vec![ThisTime, Ask, Block], ThisTime), "screen sharing is never kept");
        assert_eq!(choices(None, Permission::Camera, None, true), (vec![ThisTime, Ask], ThisTime), "nothing is stored for an opaque origin");
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
        use crate::test_support::{Reply, Server, browser, wait_until};
        use crate::window::{BrowserWindow, Focus};

        const LOCATION: &str = "navigator.geolocation.getCurrentPosition(() => document.title = 'location:ok', e => document.title = 'location:' + e.code); 'asked'";
        const MICROPHONE: &str = "navigator.mediaDevices.getUserMedia({audio: true}).then(s => { window.stream = s; document.title = 'microphone:ok'; }, e => document.title = 'microphone:' + e.name); 'asked'";

        fn setup() -> (Server, BrowserWindow) {
            crate::SCRIPTED.set(true);
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

        fn run(tab: &Tab, script: &str) {
            let done = Rc::new(RefCell::new(false));
            let flag = done.clone();
            tab.web_view().evaluate_javascript(script, None, None, None::<&gtk::gio::Cancellable>, move |_| {
                flag.replace(true);
            });
            wait_until("the script to run", || *done.borrow());
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
            let bubble = window.address_bar().bubble().filter(|b| b.has_css_class("permission-prompt") && b.is_visible())?;
            widgets::<gtk::Label>(bubble.upcast_ref()).into_iter().find(|l| l.has_css_class("heading")).map(|l| l.label().into())
        }

        fn press(window: &BrowserWindow, answer: Answer) {
            let bubble = window.address_bar().bubble().expect("a prompt is shown");
            let button = widgets::<gtk::Button>(bubble.upcast_ref()).into_iter().find(|b| b.label().as_deref() == Some(answer.label()));
            button.expect("the prompt offers the answer").emit_clicked();
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
            let permission = |url: &str| {
                let tab = open(&window, url, Focus::Foreground);
                run(&tab, "document.title = Notification.permission");
                title(&tab)
            };
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
