//! The Downloads window: core's list, newest first, following the downloads in progress
//! while it is open, their speed and time left at least every second. A finished file can be
//! opened or shown in its folder, and a file that can run code kept or discarded; an entry can
//! leave the list without its file.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::downloads::{Download, DownloadId, Progress, State as DownloadState, status_line};
use vsesvit_core::private::Browsing;

use super::Windowed;
use crate::downloads::{self, Change, Downloads};
use crate::window::BrowserWindow;

/// How often the rows in progress refresh while no bytes arrive, so a stalled download's speed
/// falls.
const TICK: Duration = Duration::from_secs(1);

/// Holds no [`crate::browser::Browser`] or profile of its own: the widgets' handlers keep
/// this state alive for as long as the window's widgets exist, which must not keep the
/// profile open.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    /// The window's kind, whose downloads the list shows.
    browsing: Browsing,
    downloads: Weak<Downloads>,
    list: gtk::ListBox,
    stack: gtk::Stack,
    rows: RefCell<Vec<Row>>,
}

struct Row {
    download: Download,
    widget: adw::ActionRow,
    /// Only for a download in progress.
    bar: Option<gtk::ProgressBar>,
}

pub(crate) fn present(window: &BrowserWindow) {
    super::present_window(window, Windowed::Downloads(window.browsing()), || build(window));
}

fn build(window: &BrowserWindow) -> adw::Window {
    let open_folder = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Open Download Folder")
        .build();
    let clear = gtk::Button::builder()
        .label("_Clear List")
        .use_underline(true)
        .tooltip_text("Remove every finished download from the list")
        .build();
    let header = adw::HeaderBar::new();
    header.pack_start(&open_folder);
    header.pack_end(&clear);

    let list = super::boxed_list();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .vexpand(true)
        .build();
    let empty = adw::StatusPage::builder()
        .icon_name("folder-download-symbolic")
        .title("No Downloads")
        .description("Files you download appear here")
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&scroller, Some("list"));
    stack.add_named(&empty, Some("empty"));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    let downloads_window = adw::Window::builder()
        .title("Downloads")
        .default_width(560)
        .default_height(560)
        .content(&toolbar)
        .build();

    let downloads = window.browser().downloads();
    let state = Rc::new(State {
        window: window.downgrade(),
        browsing: window.browsing(),
        downloads: Rc::downgrade(downloads),
        list,
        stack,
        rows: RefCell::new(Vec::new()),
    });
    state.refresh();

    let weak_state = Rc::downgrade(&state);
    let subscription = downloads.subscribe(move |change| {
        if let Some(state) = weak_state.upgrade() {
            match change {
                Change::List => state.refresh(),
                Change::Progress(id) => state.update_progress(id),
            }
        }
    });
    let weak_state = Rc::downgrade(&state);
    let tick = glib::timeout_add_local(TICK, move || {
        if let Some(state) = weak_state.upgrade() {
            state.update_running();
        }
        glib::ControlFlow::Continue
    });
    let tick = Cell::new(Some(tick));
    let weak_downloads = Rc::downgrade(downloads);
    downloads_window.connect_destroy(move |_| {
        if let Some(downloads) = weak_downloads.upgrade() {
            downloads.unsubscribe(subscription);
        }
        if let Some(tick) = tick.take() {
            tick.remove();
        }
    });

    open_folder.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.open_folder()
    ));
    clear.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| {
            if let Some(downloads) = state.downloads.upgrade() {
                downloads.clear(state.browsing);
            }
        }
    ));
    downloads_window
}

impl State {
    fn refresh(self: &Rc<Self>) {
        let Some(downloads) = self.downloads.upgrade() else { return };
        self.list.remove_all();
        let rows: Vec<Row> = downloads
            .list(self.browsing)
            .into_iter()
            .map(|download| self.row(&downloads, download))
            .collect();
        for row in &rows {
            self.list.append(&row.widget);
        }
        let page = if rows.is_empty() { "empty" } else { "list" };
        self.rows.replace(rows);
        self.stack.set_visible_child_name(page);
    }

    fn update_progress(&self, id: DownloadId) {
        let Some(downloads) = self.downloads.upgrade() else { return };
        let live = downloads.progress(id);
        let rows = self.rows.borrow();
        let Some(row) = rows.iter().find(|row| row.download.id == id) else { return };
        row.widget.set_subtitle(&status_line(&row.download, live, true));
        if let Some(bar) = &row.bar {
            show_progress(bar, live);
        }
    }

    /// Rewords the rows in progress; their bars move only as bytes arrive.
    fn update_running(&self) {
        let Some(downloads) = self.downloads.upgrade() else { return };
        for row in self.rows.borrow().iter().filter(|row| row.download.state.is_live()) {
            row.widget.set_subtitle(&status_line(&row.download, downloads.progress(row.download.id), true));
        }
    }

    fn row(self: &Rc<Self>, downloads: &Downloads, download: Download) -> Row {
        let path = download.path.clone();
        let exists = path.exists();
        let live = downloads.progress(download.id);
        let widget = adw::ActionRow::builder()
            .title(downloads::file_name(&path))
            .subtitle(status_line(&download, live, exists))
            .use_markup(false)
            .build();
        let (content_type, _) = gio::content_type_guess(Some(&path), None);
        widget.add_prefix(&gtk::Image::from_gicon(&gio::content_type_get_symbolic_icon(&content_type)));

        let in_progress = download.state.is_live();
        let bar = in_progress.then(|| {
            let bar = gtk::ProgressBar::builder().valign(gtk::Align::Center).width_request(120).build();
            show_progress(&bar, live);
            widget.add_suffix(&bar);
            bar
        });
        let id = download.id;
        if in_progress {
            widget.add_suffix(&self.button("process-stop-symbolic", "Cancel", move |state| {
                if let Some(downloads) = state.downloads.upgrade() {
                    downloads.cancel(id);
                }
            }));
        }
        if download.state == DownloadState::Unconfirmed {
            let keep = gtk::Button::builder()
                .label("_Keep")
                .use_underline(true)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let discard = gtk::Button::builder()
                .label("_Discard")
                .use_underline(true)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            for (button, kept) in [(&keep, true), (&discard, false)] {
                button.connect_clicked(glib::clone!(
                    #[strong(rename_to = state)]
                    self,
                    move |_| match state.downloads.upgrade() {
                        Some(downloads) if kept => downloads.keep(id),
                        Some(downloads) => downloads.discard(id),
                        None => {}
                    }
                ));
                widget.add_suffix(button);
            }
        }
        if download.state == DownloadState::Completed && exists {
            let open = gtk::Button::builder()
                .label("_Open")
                .use_underline(true)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            open.connect_clicked(glib::clone!(
                #[strong(rename_to = state)]
                self,
                #[strong]
                path,
                move |_| state.with_window(|window| downloads::open(window, &path))
            ));
            widget.add_suffix(&open);
            widget.set_activatable_widget(Some(&open));
        }
        if exists && download.state != DownloadState::Unconfirmed {
            widget.add_suffix(&self.button("folder-open-symbolic", "Show in Folder", {
                let path = path.clone();
                move |state| state.with_window(|window| downloads::show_in_folder(window, &path))
            }));
        }
        if download.state.is_final() {
            widget.add_suffix(&self.button("window-close-symbolic", "Remove from List", move |state| {
                if let Some(downloads) = state.downloads.upgrade() {
                    downloads.remove(id);
                }
            }));
        }
        Row { download, widget, bar }
    }

    fn button(self: &Rc<Self>, icon: &str, tooltip: &str, action: impl Fn(&Rc<Self>) + 'static) -> gtk::Button {
        let button = super::row_button(icon, tooltip);
        button.connect_clicked(glib::clone!(
            #[strong(rename_to = state)]
            self,
            move |_| action(&state)
        ));
        button
    }

    fn with_window(&self, f: impl FnOnce(&BrowserWindow)) {
        if let Some(window) = self.window.upgrade() {
            f(&window);
        }
    }

    fn open_folder(&self) {
        let Some(downloads) = self.downloads.upgrade() else { return };
        let dir = downloads.directory();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("cannot create {}: {e}", dir.display());
        }
        self.with_window(|window| downloads::open(window, &dir));
    }
}

/// A fraction when the size is known, else a pulse per update.
fn show_progress(bar: &gtk::ProgressBar, live: Option<Progress>) {
    match live {
        Some(Progress { received, total: Some(total), .. }) if total > 0 => bar.set_fraction(received as f64 / total as f64),
        Some(_) => bar.pulse(),
        None => bar.set_fraction(0.0),
    }
}
