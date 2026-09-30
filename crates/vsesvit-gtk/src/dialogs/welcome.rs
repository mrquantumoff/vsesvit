//! The first-run welcome (`vsesvit_core::onboarding`): a few pages stepped through with
//! Back and Next, to choose the search engine, import bookmarks, add the recommended
//! extensions, make Vsesvit the default browser and sign in to sync. Every choice takes effect
//! when it is made and none is required, so Next only moves on. Closing the dialog at any page
//! ends the first run.
//!
//! Like the library dialogs, it holds the window weakly, so an open welcome does not keep
//! the profile open.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::onboarding::{self, RECOMMENDED_EXTENSIONS, Recommended};
use vsesvit_core::search::SearchEngine;
use vsesvit_sync::status::{Action, State};

use super::bookmarks::{Import, pick_bookmarks_file};
use crate::window::BrowserWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Welcome,
    Search,
    Import,
    Extensions,
    DefaultBrowser,
    Sync,
    Done,
}

pub(crate) const STEPS: [Step; 7] = [
    Step::Welcome,
    Step::Search,
    Step::Import,
    Step::Extensions,
    Step::DefaultBrowser,
    Step::Sync,
    Step::Done,
];

impl Step {
    /// For file names.
    #[cfg(feature = "self-test")]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Step::Welcome => "welcome",
            Step::Search => "search",
            Step::Import => "import",
            Step::Extensions => "extensions",
            Step::DefaultBrowser => "default-browser",
            Step::Sync => "sync",
            Step::Done => "done",
        }
    }

    pub(crate) fn next_label(self) -> &'static str {
        match self {
            Step::Welcome => "Get Started",
            Step::Sync => "Skip",
            Step::Done => "Start Browsing",
            _ => "Next",
        }
    }

    fn page(self, window: &BrowserWindow) -> gtk::Widget {
        match self {
            Step::Welcome => welcome_page(),
            Step::Search => search_page(window),
            Step::Import => import_page(window),
            Step::Extensions => extensions_page(window),
            Step::DefaultBrowser => default_browser_page(),
            Step::Sync => sync_page(window),
            Step::Done => status_page(
                "object-select-symbolic",
                "You’re All Set",
                "Settings has these choices and more. The main menu’s Welcome brings this tour back.",
                None,
            ),
        }
        .upcast()
    }
}

/// Opens the welcome on `window` and returns the dialog.
pub(crate) fn present(window: &BrowserWindow) -> adw::Dialog {
    let carousel = adw::Carousel::builder()
        .interactive(false)
        .allow_scroll_wheel(false)
        .allow_mouse_drag(false)
        .vexpand(true)
        .build();
    for step in STEPS {
        carousel.append(&step.page(window));
    }
    let dots = adw::CarouselIndicatorDots::builder().carousel(&carousel).build();
    let back = gtk::Button::builder().label("Back").build();
    let next = gtk::Button::builder()
        .label(STEPS[0].next_label())
        .css_classes(["suggested-action"])
        .build();
    let bottom = gtk::CenterBox::builder()
        .start_widget(&back)
        .center_widget(&dots)
        .end_widget(&next)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    toolbar.set_content(Some(&carousel));
    toolbar.add_bottom_bar(&bottom);
    let dialog = adw::Dialog::builder()
        .title("Welcome to Vsesvit")
        .content_width(560)
        .content_height(640)
        .child(&toolbar)
        .default_widget(&next)
        .build();

    // The step being shown or scrolled to, so a click during the scroll counts from there.
    let current = Rc::new(Cell::new(0_usize));
    let go = Rc::new(glib::clone!(
        #[weak]
        carousel,
        #[weak]
        back,
        #[weak]
        next,
        #[strong]
        current,
        move |index: usize| {
            current.set(index);
            back.set_visible(index > 0);
            next.set_label(STEPS[index].next_label());
            carousel.scroll_to(&carousel.nth_page(index as u32), true);
        }
    ));
    back.set_visible(false);
    back.connect_clicked(glib::clone!(
        #[strong]
        go,
        #[strong]
        current,
        move |_| go(current.get().saturating_sub(1))
    ));
    next.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        #[strong]
        current,
        move |_| match current.get() + 1 {
            index if index < STEPS.len() => go(index),
            _ => {
                dialog.close();
            }
        }
    ));
    let weak = window.downgrade();
    dialog.connect_closed(move |_| {
        if let Some(window) = weak.upgrade()
            && let Err(e) = onboarding::finish(&mut window.browser().core().borrow_mut())
        {
            log::warn!("onboarding: {e}");
        }
    });
    dialog.present(Some(window));
    dialog
}

fn status_page(icon: &str, title: &str, description: &str, child: Option<&gtk::Widget>) -> adw::StatusPage {
    let page = adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(description)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["compact"])
        .build();
    if let Some(child) = child {
        let clamp = adw::Clamp::builder().maximum_size(440).child(child).build();
        page.set_child(Some(&clamp));
    }
    page
}

fn welcome_page() -> adw::StatusPage {
    // A build run from the source tree has no installed icon.
    let has_icon = gdk::Display::default()
        .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(crate::APP_ID));
    let page = status_page(
        if has_icon { crate::APP_ID } else { "web-browser-symbolic" },
        "Welcome to Vsesvit",
        "A web browser that runs extensions from the Chrome Web Store. A few choices and you are ready to go.",
        None,
    );
    page.remove_css_class("compact");
    page
}

fn search_page(window: &BrowserWindow) -> adw::StatusPage {
    let (engines, default): (Vec<SearchEngine>, _) = {
        let mut profile = window.browser().core().borrow_mut();
        let mut engines = profile.search_engines();
        (engines.list().unwrap_or_default(), engines.default_engine().ok().map(|e| e.id))
    };
    let group = adw::PreferencesGroup::new();
    let mut first: Option<gtk::CheckButton> = None;
    for engine in engines {
        let check = gtk::CheckButton::builder()
            .active(default.as_ref() == Some(&engine.id))
            .valign(gtk::Align::Center)
            .build();
        match &first {
            Some(first) => check.set_group(Some(first)),
            None => first = Some(check.clone()),
        }
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&engine.name))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        let weak = window.downgrade();
        check.connect_toggled(move |check| {
            if check.is_active()
                && let Some(window) = weak.upgrade()
            {
                let set = window.browser().core().borrow_mut().search_engines().set_default(&engine.id);
                if let Err(e) = set {
                    log::warn!("search engines: {e}");
                }
            }
        });
        group.add(&row);
    }
    status_page(
        "system-search-symbolic",
        "Search Engine",
        "What the address bar searches when you type words instead of an address.",
        Some(group.upcast_ref()),
    )
}

/// A row whose suffix does one thing: a button, then a spinner while it runs, then the
/// outcome. A failure goes in the subtitle, and the button comes back to try again.
struct TaskRow {
    row: adw::ActionRow,
    button: gtk::Button,
    stack: gtk::Stack,
    outcome: gtk::Label,
}

/// What a task's run came to: `Ok(None)` when the user backed out of it.
type Outcome = Result<Option<String>, String>;

impl TaskRow {
    fn new(title: &str, subtitle: &str, action: &str) -> Self {
        let button = gtk::Button::builder().label(action).valign(gtk::Align::Center).build();
        let outcome = gtk::Label::builder().css_classes(["dim-label"]).build();
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&button, Some("ready"));
        stack.add_named(&adw::Spinner::new(), Some("running"));
        stack.add_named(&outcome, Some("done"));
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(title))
            .subtitle(glib::markup_escape_text(subtitle))
            .build();
        row.add_suffix(&stack);
        TaskRow { row, button, stack, outcome }
    }

    fn done(&self, outcome: &str) {
        self.outcome.set_label(outcome);
        self.stack.set_visible_child_name("done");
    }

    /// Runs `work` on each click of the button and shows what it came to.
    fn on_click<F, Fut>(&self, work: F)
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Outcome> + 'static,
    {
        let subtitle = self.row.subtitle().unwrap_or_default();
        let work = Rc::new(work);
        self.button.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            self.row,
            #[weak(rename_to = stack)]
            self.stack,
            #[weak(rename_to = outcome)]
            self.outcome,
            move |button| {
                let task = TaskRow { row, button: button.clone(), stack, outcome };
                let (work, subtitle) = (work.clone(), subtitle.clone());
                task.stack.set_visible_child_name("running");
                glib::spawn_future_local(async move {
                    let outcome = work().await;
                    task.row.remove_css_class("error");
                    task.row.set_subtitle(&subtitle);
                    match outcome {
                        Ok(Some(outcome)) => task.done(&outcome),
                        Ok(None) => task.stack.set_visible_child_name("ready"),
                        Err(error) => {
                            task.row.set_subtitle(&glib::markup_escape_text(&error));
                            task.row.add_css_class("error");
                            task.button.set_label("Try Again");
                            task.stack.set_visible_child_name("ready");
                        }
                    }
                });
            }
        ));
    }
}

fn imported(added: Result<usize, String>) -> Outcome {
    match added? {
        0 => Ok(Some("Nothing to import".to_owned())),
        1 => Ok(Some("1 item imported".to_owned())),
        n => Ok(Some(format!("{n} items imported"))),
    }
}

fn import_page(window: &BrowserWindow) -> adw::StatusPage {
    let browsers = adw::PreferencesGroup::new();
    let looking = adw::ActionRow::builder().title("Looking for other browsers…").build();
    looking.add_suffix(&adw::Spinner::new());
    browsers.add(&looking);
    let weak = window.downgrade();
    glib::spawn_future_local(glib::clone!(
        #[weak]
        browsers,
        async move {
            let found = gio::spawn_blocking(vsesvit_core::import::installed_browsers).await.unwrap_or_default();
            browsers.remove(&looking);
            if found.is_empty() {
                browsers.add(&adw::ActionRow::builder().title("No other browsers found").build());
            }
            for found in found {
                let import = Import::from(found);
                let task = TaskRow::new(&import.from, &format!("Into “{}” on the bookmarks bar", import.folder), "Import");
                browsers.add(&task.row);
                let weak = weak.clone();
                task.on_click(move || {
                    let (weak, import) = (weak.clone(), import.clone());
                    async move {
                        let window = weak.upgrade().ok_or("The window is closed")?;
                        imported(import.run(window.browser().clone()).await)
                    }
                });
            }
        }
    ));

    let file = TaskRow::new("Bookmarks File", "An HTML export, or a Chromium Bookmarks file", "Choose File…");
    let weak = window.downgrade();
    file.on_click(move || {
        let weak = weak.clone();
        async move {
            let Some(path) = pick_bookmarks_file(weak.upgrade().as_ref()).await else {
                return Ok(None);
            };
            let window = weak.upgrade().ok_or("The window is closed")?;
            imported(Import::file(path).run(window.browser().clone()).await)
        }
    });
    let files = adw::PreferencesGroup::new();
    files.add(&file.row);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.append(&browsers);
    content.append(&files);
    status_page(
        "user-bookmarks-symbolic",
        "Import Bookmarks",
        "Bring your bookmarks from another browser. Each import goes into a folder of its own on the bookmarks bar.",
        Some(content.upcast_ref()),
    )
}

fn extensions_page(window: &BrowserWindow) -> adw::StatusPage {
    let group = adw::PreferencesGroup::new();
    for recommended in RECOMMENDED_EXTENSIONS {
        let task = TaskRow::new(recommended.name, recommended.blurb, "Install");
        let installed = window.browser().core().borrow_mut().extensions().get(&recommended.id());
        if matches!(installed, Ok(Some(_))) {
            task.done("Installed");
        }
        let weak = window.downgrade();
        task.on_click(move || install(weak.clone(), *recommended));
        group.add(&task.row);
    }
    status_page(
        "application-x-addon-symbolic",
        "Extensions",
        "A few to start with, from the Chrome Web Store. The Extensions page adds more.",
        Some(group.upcast_ref()),
    )
}

async fn install(window: glib::WeakRef<BrowserWindow>, recommended: Recommended) -> Outcome {
    let browser = window.upgrade().ok_or("The window is closed")?.browser().clone();
    match browser.install(recommended.install_source(), |_| {}).await {
        Ok(Some(_)) => Ok(Some("Installed".to_owned())),
        Ok(None) => Err("Removed on another device while it downloaded".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

const WEB_TYPES: [&str; 3] = ["x-scheme-handler/http", "x-scheme-handler/https", "text/html"];

/// Whether web links from other apps open here, and whether this copy can make them.
enum DefaultBrowser {
    Vsesvit,
    Other { app: gio::AppInfo, current: Option<String> },
    /// Why this copy cannot register itself, for the user.
    Unavailable(&'static str),
}

impl DefaultBrowser {
    fn now() -> Self {
        let id = format!("{}.desktop", crate::APP_ID);
        let current = gio::AppInfo::default_for_type(WEB_TYPES[1], false);
        if current.as_ref().and_then(|app| app.id()).as_deref() == Some(id.as_str()) {
            return DefaultBrowser::Vsesvit;
        }
        // A sandbox cannot write the user's `mimeapps.list`, and no portal sets a default.
        if Path::new("/.flatpak-info").exists() {
            return DefaultBrowser::Unavailable(
                "Flatpak apps cannot change the default browser themselves. Choose Vsesvit as the web browser under Default Apps in your system settings.",
            );
        }
        let installed = gio::AppInfo::all_for_type(WEB_TYPES[1]).into_iter().find(|app| app.id().as_deref() == Some(id.as_str()));
        match installed {
            Some(app) => DefaultBrowser::Other { app, current: current.map(|app| app.display_name().to_string()) },
            None => DefaultBrowser::Unavailable(
                "This copy of Vsesvit has no desktop entry installed, so the system cannot open links with it.",
            ),
        }
    }

    fn status(&self) -> String {
        match self {
            DefaultBrowser::Vsesvit => "Vsesvit is your default browser.".to_owned(),
            DefaultBrowser::Other { current: Some(name), .. } => format!("Links from other apps open in {name}."),
            DefaultBrowser::Other { current: None, .. } => "No default browser is set.".to_owned(),
            DefaultBrowser::Unavailable(why) => (*why).to_owned(),
        }
    }
}

/// Sign In starts the usual sign-in, whose page opens in a tab, and moves on without waiting for
/// it. Once signed in or signing in, the page says so instead.
fn sync_page(window: &BrowserWindow) -> adw::StatusPage {
    let browser = window.browser();
    let syncer = browser.sync().clone();
    let server = adw::PreferencesGroup::new();
    server.add(&super::settings::sync_server_row(browser, &server));
    let sign_in = gtk::Button::builder()
        .label(Action::SignIn.label())
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    let status = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
    sign_in.connect_clicked(glib::clone!(
        #[strong]
        syncer,
        move |button| {
            syncer.act(Action::SignIn);
            if let Some(next) = button.ancestor(adw::Dialog::static_type()).and_downcast::<adw::Dialog>().and_then(|d| d.default_widget()) {
                next.activate();
            }
        }
    ));
    syncer.watch(glib::clone!(
        #[weak]
        sign_in,
        #[weak]
        status,
        #[upgrade_or]
        false,
        move |state: &State| {
            let offered = state.status(0).actions.contains(&Action::SignIn);
            sign_in.set_visible(offered);
            status.set_visible(!offered);
            status.set_label(&state.status(vsesvit_sync::now_secs()).title);
            true
        }
    ));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.append(&server);
    content.append(&sign_in);
    content.append(&status);
    status_page(
        super::settings::SYNC_ICON,
        "Sync your browser",
        "Have your bookmarks, history, open tabs, extensions and settings on all your devices.",
        Some(content.upcast_ref()),
    )
}

fn default_browser_page() -> adw::StatusPage {
    let status = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
    let error = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["error"])
        .visible(false)
        .build();
    let button = gtk::Button::builder()
        .label("Make Vsesvit the Default Browser")
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    let refresh = glib::clone!(
        #[weak]
        status,
        #[weak]
        button,
        move || {
            let now = DefaultBrowser::now();
            status.set_label(&now.status());
            button.set_visible(matches!(now, DefaultBrowser::Other { .. }));
        }
    );
    refresh();
    button.connect_clicked(glib::clone!(
        #[weak]
        error,
        move |_| {
            let result = match DefaultBrowser::now() {
                DefaultBrowser::Other { app, .. } => {
                    WEB_TYPES.iter().try_for_each(|kind| app.set_as_default_for_type(kind))
                }
                _ => Ok(()),
            };
            error.set_visible(result.is_err());
            if let Err(e) = result {
                error.set_label(&format!("Could not make Vsesvit the default: {e}"));
            }
            refresh();
        }
    ));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.append(&status);
    content.append(&button);
    content.append(&error);
    status_page(
        "web-browser-symbolic",
        "Default Browser",
        "Open links from other apps in Vsesvit.",
        Some(content.upcast_ref()),
    )
}
