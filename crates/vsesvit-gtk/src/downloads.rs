//! Downloads go straight to the downloads directory, never overwriting a file, with a toast
//! when they start and when they finish.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::downloads::unique_destination;

use crate::window::BrowserWindow;

pub(crate) fn watch(session: &webkit::NetworkSession, dir: PathBuf, app: &adw::Application) {
    let app = app.downgrade();
    session.connect_download_started(move |_, download| {
        let dir = dir.clone();
        download.connect_decide_destination(move |download, suggested| {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("cannot create {}: {e}", dir.display());
                download.cancel();
                return true;
            }
            let destination = unique_destination(&dir, suggested, |p| p.exists());
            match destination.to_str() {
                Some(path) => download.set_destination(path),
                None => download.cancel(),
            }
            true
        });

        download.connect_created_destination(glib::clone!(
            #[strong]
            app,
            move |download, destination| {
                let toast = adw::Toast::new(&format!("Downloading “{}”", file_name(destination)));
                if let Some(window) = window_for(&app, download) {
                    window.toast(toast);
                }
            }
        ));

        let failed = Rc::new(Cell::new(false));
        download.connect_failed(glib::clone!(
            #[strong]
            app,
            #[strong]
            failed,
            move |download, error| {
                failed.set(true);
                if error.matches(webkit::DownloadError::CancelledByUser) {
                    return;
                }
                let name = download
                    .destination()
                    .map_or_else(|| "file".to_owned(), |d| file_name(&d));
                log::warn!("download of {name} failed: {error}");
                if let Some(window) = window_for(&app, download) {
                    window.toast(adw::Toast::new(&format!("Download of “{name}” failed")));
                }
            }
        ));
        download.connect_finished(glib::clone!(
            #[strong]
            app,
            move |download| {
                let Some(destination) = download.destination().filter(|_| !failed.get()) else {
                    return;
                };
                log::info!("downloaded {destination}");
                let Some(window) = window_for(&app, download) else {
                    return;
                };
                let toast = adw::Toast::builder()
                    .title(format!("“{}” downloaded", file_name(&destination)))
                    .button_label("Open")
                    .build();
                let file = gio::File::for_path(&*destination);
                toast.connect_button_clicked(glib::clone!(
                    #[weak]
                    window,
                    move |_| open_file(&window, &file)
                ));
                window.toast(toast);
            }
        ));
    });
}

/// The window of the tab that started the download, or else the most recently used one.
fn window_for(
    app: &glib::WeakRef<adw::Application>,
    download: &webkit::Download,
) -> Option<BrowserWindow> {
    download
        .web_view()
        .and_then(|view| view.root())
        .and_downcast::<BrowserWindow>()
        .or_else(|| {
            app.upgrade()
                .and_then(|app| app.active_window())
                .and_downcast()
        })
}

fn open_file(window: &BrowserWindow, file: &gio::File) {
    gtk::FileLauncher::new(Some(file)).launch(Some(window), None::<&gio::Cancellable>, |result| {
        if let Err(e) = result {
            log::warn!("cannot open the download: {e}");
        }
    });
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}
