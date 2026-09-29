//! The GApplication: one instance per profile, command-line handling, application actions and
//! the keyboard shortcuts of every window.
//!
//! The profile is opened before the application registers on the session bus. When core
//! reports the profile as locked, another Vsesvit process owns it: this process still runs
//! the application, and GApplication then hands its command line (the URLs to open) to that
//! running instance and exits. Only if no instance is reachable does it report the lock.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::gio::ActionEntry;
use gtk::{gdk, gio, glib};
use vsesvit_core::{OpenError, OpenOptions, Profile};

use crate::browser::Browser;
use crate::cli::{self, Command};
use crate::profile::ProfileLocation;
use crate::{dialogs, location};

/// Accelerators for actions; the shortcuts dialog reads them back from the actions.
pub(crate) const ACCELS: &[(&str, &[&str])] = &[
    ("win.new-tab", &["<Control>t"]),
    ("win.close-tab", &["<Control>w", "<Control>F4"]),
    ("win.reopen-closed-tab", &["<Control><Shift>t"]),
    ("win.focus-location", &["<Control>l", "<Alt>d", "F6"]),
    ("win.reload", &["<Control>r", "F5"]),
    (
        "win.reload-bypass-cache",
        &["<Control><Shift>r", "<Shift>F5"],
    ),
    ("win.back", &["<Alt>Left"]),
    ("win.forward", &["<Alt>Right"]),
    ("win.bookmark-page", &["<Control>d"]),
    ("win.find", &["<Control>f"]),
    ("win.find-next", &["<Control>g"]),
    ("win.find-previous", &["<Control><Shift>g"]),
    (
        "win.zoom-in",
        &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
    ),
    ("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]),
    ("win.zoom-reset", &["<Control>0", "<Control>KP_0"]),
    ("win.fullscreen", &["F11"]),
    ("win.toggle-tab-sidebar", &["<Control>s", "F9"]),
    ("win.show-bookmarks-bar", &["<Control><Shift>b"]),
    ("win.show-bookmarks", &["<Control><Shift>o"]),
    ("win.show-history", &["<Control>h"]),
    ("win.show-downloads", &["<Control>j"]),
    ("win.show-settings", &["<Control>comma"]),
    ("app.new-window", &["<Control>n"]),
    ("app.shortcuts", &["<Control>question"]),
    ("app.quit", &["<Control>q"]),
];

pub(crate) const CSS: &str = "
.link-preview {
  padding: 3px 8px;
  border-top-right-radius: 6px;
  background-color: @view_bg_color;
  color: @view_fg_color;
  box-shadow: 0 0 0 1px alpha(currentColor, 0.15);
  font-size: 0.9em;
}
.bookmarks-bar { min-height: 28px; padding: 2px 6px; }
.bookmarks-bar button { padding: 2px 6px; min-height: 24px; }
.bookmarks-bar .drop-before { box-shadow: inset 2px 0 @accent_bg_color; }
.bookmarks-bar .drop-after { box-shadow: inset -2px 0 @accent_bg_color; }
.bookmark-row.drop-before { box-shadow: inset 0 2px @accent_bg_color; }
.bookmark-row.drop-after { box-shadow: inset 0 -2px @accent_bg_color; }
.drop-into { background-color: alpha(@accent_bg_color, 0.25); border-radius: 6px; }
.bookmarks-bar .bookmarks-chevron { padding: 2px 8px; font-weight: bold; }
popover.bookmark-menu > contents { padding: 4px; }
.bookmark-menu-row { padding: 4px 8px; min-height: 24px; font-weight: normal; }
entry.address-entry > progress { margin: 0 10px 1px; }
entry.address-entry > progress > trough > progress {
  min-height: 2px;
  border: none;
  border-radius: 1px;
  box-shadow: none;
  background-color: transparent;
  background-image: linear-gradient(to right, alpha(@window_fg_color, 0), @window_fg_color);
}
.address-zoom { min-height: 20px; min-width: 0; padding: 0 6px; font-size: 0.85em; border-radius: 10px; }
.tab-sidebar { background-color: @sidebar_bg_color; }
.tab-sidebar listview { background: transparent; }
.tab-sidebar row { padding: 0; }
.tab-row { padding: 4px 6px 4px 10px; min-height: 30px; }
.tab-row .tab-close { min-width: 22px; min-height: 22px; padding: 0; opacity: 0.6; }
.tab-row .tab-close:hover { opacity: 1; }
.extension-badge {
  font-size: 0.65em;
  font-weight: bold;
  padding: 0 3px;
  min-width: 10px;
  min-height: 12px;
  border-radius: 6px;
  background-color: @accent_bg_color;
  color: @accent_fg_color;
}
";

/// The browser lives here only between startup and the end of `run`, so that nothing the
/// application's closures hold keeps it alive past shutdown.
pub(crate) type Slot = Rc<RefCell<Option<Browser>>>;

pub(crate) fn run(profile_dir: Option<PathBuf>, args: &[OsString]) -> ExitCode {
    let location = match ProfileLocation::resolve(profile_dir) {
        Ok(location) => location,
        Err(e) => {
            eprintln!("vsesvit: cannot use the profile directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let profile = match Profile::open(&location.root, OpenOptions::default()) {
        Ok(profile) => Some(profile),
        // Another process owns the profile. The application below finds it on the bus
        // and forwards this command line to it.
        Err(OpenError::Locked) => None,
        Err(e) => {
            eprintln!(
                "vsesvit: cannot open the profile at {}: {e}",
                location.root.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let app = adw::Application::builder()
        .application_id(&location.app_id)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let pending = Rc::new(RefCell::new(profile));
    let slot: Slot = Rc::default();

    app.connect_startup(glib::clone!(
        #[strong]
        slot,
        move |app| {
            if let Some(profile) = pending.take() {
                startup(app, &slot, profile);
            }
        }
    ));
    app.connect_command_line(glib::clone!(
        #[strong]
        slot,
        move |_, command_line| match slot.borrow().as_ref() {
            Some(browser) => open_from_command_line(browser, command_line),
            None => {
                command_line.printerr_literal(
                    "vsesvit: the profile is in use by another Vsesvit process that could not be reached\n",
                );
                glib::ExitCode::FAILURE
            }
        }
    ));
    app.connect_activate(glib::clone!(
        #[strong]
        slot,
        move |_| {
            if let Some(browser) = slot.borrow().as_ref() {
                browser.present();
            }
        }
    ));
    app.connect_shutdown(glib::clone!(
        #[strong]
        slot,
        move |_| {
            if let Some(browser) = slot.borrow().as_ref() {
                browser.shutdown();
            }
        }
    ));

    let status = app.run_with_args_os(args);
    // After `run`, so the new process becomes the primary instance instead of handing its
    // command line to this one.
    let restart = slot.borrow().as_ref().and_then(Browser::restart_program);
    if let Some(program) = restart {
        release_profile(&app, &slot);
        match std::process::Command::new(&program)
            .args(location.relaunch_args())
            .spawn()
        {
            Ok(_) => log::info!("restarted into {}", program.display()),
            Err(e) => log::warn!("could not start {}: {e}", program.display()),
        }
    }
    status.into()
}

/// Closes the profile before a restart starts the next process, which opens it straight
/// away and gives up if this one still holds its lock. The windows hold the browser, and
/// GTK keeps them after `run` returns, so they are destroyed first; the session was already
/// saved at shutdown.
fn release_profile(app: &adw::Application, slot: &Slot) {
    let Some(browser) = slot.take() else { return };
    let profile = Rc::downgrade(browser.core());
    drop(browser);
    for window in app.windows() {
        if let Some(window) = window.downcast_ref::<adw::ApplicationWindow>()
            && let Some(dialog) = window.visible_dialog()
        {
            dialog.force_close();
        }
        window.destroy();
    }
    // What the teardown queued (finalizing widgets, answering force-closed dialogs), bounded
    // in case a source keeps rescheduling itself.
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && context.iteration(false) {}
    if profile.strong_count() > 0 {
        log::warn!("the profile is still open; the restarted browser may find it locked");
    }
}

/// Everything a window needs before the first one exists. Shared with the self-test, which
/// builds its own application around a fresh profile.
pub(crate) fn setup(app: &adw::Application, slot: &Slot) {
    glib::set_application_name("Vsesvit");
    gtk::Window::set_default_icon_name(crate::APP_ID);
    load_css();
    for (action, accels) in ACCELS {
        app.set_accels_for_action(action, accels);
    }
    install_actions(app, slot);
}

fn startup(app: &adw::Application, slot: &Slot, profile: Profile) {
    setup(app, slot);
    let browser = Browser::new(app, profile);
    browser.start();
    slot.replace(Some(browser));
}

/// Runs in the primary instance for its own command line and for each later `vsesvit`
/// invocation on the same profile, which GApplication forwards here and then exits.
fn open_from_command_line(
    browser: &Browser,
    command_line: &gio::ApplicationCommandLine,
) -> glib::ExitCode {
    let targets = match cli::parse(command_line.arguments().into_iter().skip(1)) {
        Ok(Command::Browse { targets, .. }) => targets,
        Ok(_) => Vec::new(),
        Err(e) => {
            command_line.printerr_literal(&format!("vsesvit: {e}\n"));
            return glib::ExitCode::FAILURE;
        }
    };
    let cwd = command_line.cwd().unwrap_or_else(|| PathBuf::from("/"));
    let mut urls = Vec::with_capacity(targets.len());
    for target in &targets {
        match location::resolve_cli_target(target, &cwd, Path::is_file) {
            Some(url) => urls.push(url),
            None => command_line.printerr_literal(&format!(
                "vsesvit: cannot open {}\n",
                target.to_string_lossy()
            )),
        }
    }
    if browser.windows().is_empty() {
        browser.open_startup_windows(&urls);
    } else if urls.is_empty() {
        browser.present();
    } else {
        browser.open_window(&urls);
    }
    glib::ExitCode::SUCCESS
}

fn install_actions(app: &adw::Application, slot: &Slot) {
    app.add_action_entries([
        ActionEntry::builder("new-window")
            .activate(glib::clone!(
                #[strong]
                slot,
                move |_: &adw::Application, _, _| {
                    if let Some(browser) = slot.borrow().as_ref() {
                        browser.open_window(&[]);
                    }
                }
            ))
            .build(),
        ActionEntry::builder("about")
            .activate(|app: &adw::Application, _, _| {
                if let Some(window) = app.active_window() {
                    dialogs::about::present(&window);
                }
            })
            .build(),
        ActionEntry::builder("shortcuts")
            .activate(|app: &adw::Application, _, _| {
                if let Some(window) = app.active_window() {
                    dialogs::shortcuts::present(&window);
                }
            })
            .build(),
        ActionEntry::builder("quit")
            .activate(glib::clone!(
                #[strong]
                slot,
                move |app: &adw::Application, _, _| {
                    // The windows are gone by the time `shutdown` runs, so the session is
                    // saved while they still exist.
                    if let Some(browser) = slot.borrow().as_ref() {
                        browser.save_session_now();
                    }
                    app.quit();
                }
            ))
            .build(),
    ]);
    #[cfg(debug_assertions)]
    install_debug_actions(app, slot);
}

/// Development builds only, driven over D-Bus (`org.gtk.Actions`):
///
/// - `app.debug-screenshot(path)` captures the active window with the in-app renderer, so
///   scripts can check the running browser without grabbing the desktop.
/// - `app.debug-apply-sync(path)` applies a JSON file of wire records
///   (`[{"kind": "prefs", "id": "theme", "body": {...}}, ...]`) through core's sync store
///   and refreshes the UI the way a sync engine would.
/// - `app.debug-set-tabs-position(left|right|top)` is what the Settings dialog does.
/// - `app.debug-close-dialog()` closes the active window's dialog, if one is open.
#[cfg(debug_assertions)]
fn install_debug_actions(app: &adw::Application, slot: &Slot) {
    app.add_action_entries([
        ActionEntry::builder("debug-screenshot")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|app: &adw::Application, _, parameter| {
                let Some(path) = parameter.and_then(|p| p.get::<String>()) else {
                    return;
                };
                let Some(window) = app.active_window() else {
                    log::warn!("debug-screenshot: no window");
                    return;
                };
                glib::spawn_future_local(async move {
                    let saved = crate::screenshot::save_png(&window, Path::new(&path));
                    match glib::future_with_timeout(std::time::Duration::from_secs(5), saved).await {
                        Ok(Ok(())) => log::info!("debug-screenshot: wrote {path}"),
                        Ok(Err(e)) => log::warn!("debug-screenshot: {e}"),
                        Err(_) => log::warn!("debug-screenshot: the window drew no frame in 5 s"),
                    }
                });
            })
            .build(),
        ActionEntry::builder("debug-apply-sync")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(glib::clone!(
                #[strong]
                slot,
                move |_: &adw::Application, _, parameter| {
                    let Some(path) = parameter.and_then(|p| p.get::<String>()) else {
                        return;
                    };
                    let browser = slot.borrow().clone();
                    let Some(browser) = browser else {
                        log::warn!("debug-apply-sync: no profile");
                        return;
                    };
                    match apply_sync_file(&browser, Path::new(&path)) {
                        Ok(summary) => log::info!("debug-apply-sync: {summary}"),
                        Err(e) => log::warn!("debug-apply-sync: {e}"),
                    }
                }
            ))
            .build(),
        ActionEntry::builder("debug-set-tabs-position")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(glib::clone!(
                #[strong]
                slot,
                move |_: &adw::Application, _, parameter| {
                    use vsesvit_core::prefs::TabsPosition;
                    let position = match parameter.and_then(|p| p.get::<String>()).as_deref() {
                        Some("left") => TabsPosition::Left,
                        Some("right") => TabsPosition::Right,
                        Some("top") => TabsPosition::Top,
                        other => {
                            log::warn!("debug-set-tabs-position: unknown position {other:?}");
                            return;
                        }
                    };
                    let browser = slot.borrow().clone();
                    if let Some(browser) = browser {
                        browser.set_tabs_position(position);
                    }
                }
            ))
            .build(),
        ActionEntry::builder("debug-close-dialog")
            .activate(|app: &adw::Application, _, _| {
                let dialog = app
                    .active_window()
                    .and_downcast::<adw::ApplicationWindow>()
                    .and_then(|window| window.visible_dialog());
                match dialog {
                    Some(dialog) => {
                        dialog.close();
                    }
                    None => log::warn!("debug-close-dialog: no dialog is open"),
                }
            })
            .build(),
    ]);
}

/// Reads wire records from `path` and applies them, then lets the browser refresh.
#[cfg(debug_assertions)]
fn apply_sync_file(browser: &Browser, path: &Path) -> Result<String, String> {
    use vsesvit_core::sync::{Kind, WireRecord};
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut records = Vec::with_capacity(entries.len());
    for entry in &entries {
        let kind = match entry.get("kind").and_then(|k| k.as_str()).unwrap_or_default() {
            "bookmarks" => Kind::Bookmarks,
            "history_pages" => Kind::HistoryPages,
            "history_deletions" => Kind::HistoryDeletions,
            "sessions" => Kind::Sessions,
            "extensions" => Kind::Extensions,
            "ext_storage_sync" => Kind::ExtStorageSync,
            "prefs" => Kind::Prefs,
            "search_engines" => Kind::SearchEngines,
            "site_permissions" => Kind::SitePermissions,
            other => return Err(format!("unknown record kind {other:?}")),
        };
        let id = entry.get("id").and_then(|i| i.as_str()).ok_or("a record has no id")?.to_owned();
        let body = entry.get("body").ok_or("a record has no body")?;
        let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        records.push(WireRecord { kind, id, body });
    }
    let report = browser.core().borrow_mut().sync().apply(records).map_err(|e| e.to_string())?;
    browser.sync_applied(&report.changed);
    Ok(format!(
        "merged {}, unchanged {}, rejected {}: {:?}",
        report.merged,
        report.unchanged,
        report.rejected.len(),
        report.changed
    ))
}

fn load_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{registered_app, scratch_dir};
    use crate::window::BrowserWindow;

    const CHILD: &str = "app::tests::releasing_the_profile_frees_its_lock";

    #[test]
    fn a_restart_frees_the_profile_before_the_new_process_starts() {
        // The check tears down a browser of its own, and the extension runtime allows one
        // per process, so it runs in a child process.
        let exe = std::env::current_exe().expect("the test binary");
        let output = std::process::Command::new(exe)
            .args([CHILD, "--exact", "--include-ignored", "--test-threads=1", "--nocapture"])
            .output()
            .expect("the test binary runs");
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{log}");
        assert!(log.contains("1 passed"), "{log}");
    }

    #[gtk::test]
    #[ignore = "run in a child process by a_restart_frees_the_profile_before_the_new_process_starts"]
    fn releasing_the_profile_frees_its_lock() {
        let root = scratch_dir("restart");
        let profile = Profile::open(&root, OpenOptions::default()).expect("a scratch profile");
        let app = registered_app();
        let browser = Browser::new(&app, profile);
        browser.start();
        let window = BrowserWindow::new(&browser);
        window.new_tab();
        // The library dialogs, one of them still open, as they can be when the user restarts.
        let close_dialog = |window: &BrowserWindow| {
            if let Some(dialog) = window.visible_dialog() {
                dialog.force_close();
            }
        };
        dialogs::history::present(&window);
        close_dialog(&window);
        dialogs::bookmarks::present(&window);
        close_dialog(&window);
        dialogs::downloads::present(&window);
        close_dialog(&window);
        dialogs::extensions::present(&window);
        let slot: Slot = Rc::new(RefCell::new(Some(browser)));
        drop(window);

        release_profile(&app, &slot);
        let reopened = Profile::open(&root, OpenOptions::default());
        assert!(reopened.is_ok(), "the profile is still held: {:?}", reopened.err());
    }
}
