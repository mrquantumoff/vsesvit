//! Self-updates (docs/design/packaging.md). The application owns one [`Lifecycle`]: a timer and
//! the banner's button feed it events, and checks, downloads and installs run on a worker
//! thread whose results come back to the UI thread as more events.

pub(crate) mod headless;
mod lifecycle;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use futures_channel::mpsc;
use futures_util::StreamExt;
use gtk::gio::ActionEntry;
use gtk::glib;
use semver::Version;
use vsesvit_core::prefs::UpdateChannel;
use vsesvit_update::{
    Config, DisabledReason, Downloaded, Error, Format, Installation, Updater,
    remove_stale_downloads,
};

use crate::window::BrowserWindow;
pub(crate) use lifecycle::Banner;
use lifecycle::{Effect, Event, Lifecycle, State};

const FIRST_CHECK_DELAY_SECS: u32 = 30;
const CHECK_INTERVAL_SECS: u32 = 24 * 60 * 60;

#[derive(Clone)]
pub(crate) struct Updates(Rc<Inner>);

struct Inner {
    app: adw::Application,
    updater: Arc<Updater>,
    installation: Installation,
    dir: PathBuf,
    /// Resolved before anything is installed: once a package manager replaces the executable,
    /// `current_exe()` names the deleted file.
    program: PathBuf,
    lifecycle: RefCell<Lifecycle<Downloaded>>,
    automatic: Cell<bool>,
    window_seen: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    restart: Cell<bool>,
}

impl Updates {
    /// `None` when this copy does not update itself (unpackaged, Flatpak, or a build without an
    /// updater key); that is logged once.
    pub(crate) fn new(app: &adw::Application, automatic: bool) -> Option<Updates> {
        let installation = Installation::detect();
        let started = if installation.self_updates() {
            Config::builtin()
                .and_then(|config| Updater::new(config, current_version(), installation.clone()))
        } else {
            Err(Error::Disabled(DisabledReason::NotSelfUpdating))
        };
        let program = match &installation {
            Installation::AppImage { image } => Ok(image.clone()),
            _ => std::env::current_exe().map_err(Error::Io),
        };
        let parts = started.and_then(|updater| program.map(|program| (updater, program)));
        let (updater, program) = match parts {
            Ok(parts) => parts,
            Err(e) => {
                log::info!("{e}");
                return None;
            }
        };
        let updates = Updates(Rc::new(Inner {
            app: app.clone(),
            updater: Arc::new(updater),
            lifecycle: RefCell::new(Lifecycle::new(
                installation.format() == Some(Format::AppImage),
            )),
            installation,
            dir: download_dir(),
            program,
            automatic: Cell::new(automatic),
            window_seen: Cell::new(false),
            timer: RefCell::default(),
            restart: Cell::new(false),
        }));
        if !automatic {
            log::info!("automatic updates are off");
        }
        updates.install_action();
        Some(updates)
    }

    pub(crate) fn automatic(&self) -> bool {
        self.0.automatic.get()
    }

    pub(crate) fn set_automatic(&self, automatic: bool) {
        self.0.automatic.set(automatic);
        if !automatic {
            self.cancel_timer();
        } else if self.0.window_seen.get() && self.0.timer.borrow().is_none() {
            self.schedule();
        }
    }

    /// Shows the banner the lifecycle calls for; the first window also starts the checks.
    pub(crate) fn window_opened(&self, window: &BrowserWindow) {
        window.set_update_banner(self.0.lifecycle.borrow().banner().as_ref());
        if !self.0.window_seen.replace(true) && self.automatic() {
            self.schedule();
        }
    }

    /// The program to start once the application has quit, when the user asked for a restart.
    pub(crate) fn restart_program(&self) -> Option<PathBuf> {
        self.0.restart.get().then(|| self.0.program.clone())
    }

    fn install_action(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0
            .app
            .add_action_entries([ActionEntry::builder("update")
                .activate(move |_: &adw::Application, _, _| {
                    if let Some(inner) = weak.upgrade() {
                        Updates(inner).dispatch(Event::Activate);
                    }
                })
                .build()]);
    }

    fn schedule(&self) {
        let weak = Rc::downgrade(&self.0);
        let first = glib::timeout_add_seconds_local_once(FIRST_CHECK_DELAY_SECS, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let weak = Rc::downgrade(&inner);
            let daily = glib::timeout_add_seconds_local(CHECK_INTERVAL_SECS, move || {
                match weak.upgrade() {
                    Some(inner) => {
                        Updates(inner).dispatch(Event::Check);
                        glib::ControlFlow::Continue
                    }
                    None => glib::ControlFlow::Break,
                }
            });
            // The one-shot source is already gone, so its id is dropped, not removed.
            *inner.timer.borrow_mut() = Some(daily);
            Updates(inner).dispatch(Event::Check);
        });
        self.0.timer.replace(Some(first));
    }

    fn cancel_timer(&self) {
        if let Some(timer) = self.0.timer.take() {
            timer.remove();
        }
    }

    fn dispatch(&self, event: Event<Downloaded>) {
        let effect = self.0.lifecycle.borrow_mut().step(event);
        let (banner, disabled) = {
            let lifecycle = self.0.lifecycle.borrow();
            let disabled = match lifecycle.state() {
                State::Disabled(reason) => Some(reason.clone()),
                _ => None,
            };
            (lifecycle.banner(), disabled)
        };
        for window in self.windows() {
            window.set_update_banner(banner.as_ref());
        }
        if let Some(reason) = disabled {
            log::info!("{}", Error::Disabled(reason));
            self.cancel_timer();
        }
        match effect {
            None => {}
            Some(Effect::Check) => {
                let (updater, dir) = (self.0.updater.clone(), self.0.dir.clone());
                self.spawn(move |send| {
                    if let Err(e) = check_and_download(&updater, &dir, send) {
                        send(Event::Failed(e));
                    }
                });
            }
            Some(Effect::Install(downloaded)) => {
                let installation = self.0.installation.clone();
                self.spawn(move |send| {
                    send(match downloaded.install(&installation, &[]) {
                        Ok(next) => Event::Installed(next),
                        Err(failed) => Event::InstallFailed {
                            error: failed.error,
                            downloaded: failed.downloaded,
                        },
                    });
                });
            }
            Some(Effect::Restart) => {
                self.0.restart.set(true);
                self.0.app.quit();
            }
            Some(Effect::Exit) => self.0.app.quit(),
            Some(Effect::Notify(mut message)) => {
                if let Some(first) = message.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                let window = self
                    .0
                    .app
                    .active_window()
                    .and_then(|w| w.downcast::<BrowserWindow>().ok());
                if let Some(window) = window.or_else(|| self.windows().into_iter().next()) {
                    window.toast(adw::Toast::new(&message));
                }
            }
        }
    }

    /// Runs `work` on a worker thread; each event it sends is dispatched here on the UI thread.
    fn spawn(&self, work: impl FnOnce(&dyn Fn(Event<Downloaded>)) + Send + 'static) {
        let (sender, mut receiver) = mpsc::unbounded();
        let spawned = std::thread::Builder::new()
            .name("vsesvit-update".into())
            .spawn(move || {
                work(&|event| {
                    let _ = sender.unbounded_send(event);
                });
            });
        if let Err(e) = spawned {
            self.dispatch(Event::Failed(Error::Io(e)));
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            while let Some(event) = receiver.next().await {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Updates(inner).dispatch(event);
            }
        });
    }

    fn windows(&self) -> Vec<BrowserWindow> {
        self.0
            .app
            .windows()
            .into_iter()
            .filter_map(|w| w.downcast().ok())
            .collect()
    }
}

fn check_and_download(
    updater: &Updater,
    dir: &Path,
    send: &dyn Fn(Event<Downloaded>),
) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let available = updater.check(UpdateChannel::of_build().name())?;
    remove_stale_downloads(dir, available.as_ref().map(|a| &a.release().version))?;
    let Some(available) = available else {
        send(Event::UpToDate);
        return Ok(());
    };
    let update = available.into_update()?;
    send(Event::Found(update.release.version.clone()));
    let downloaded = update.download(dir, |_, _| {})?;
    send(Event::Downloaded(downloaded));
    Ok(())
}

fn download_dir() -> PathBuf {
    glib::user_cache_dir().join("vsesvit").join("updates")
}

fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}
