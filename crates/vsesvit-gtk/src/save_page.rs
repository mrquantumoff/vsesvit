//! Save Page As: an HTML page becomes one MHTML file, and a document the tab shows directly
//! (an image, a PDF, plain text) is saved as that file, through the downloads list.

use std::path::Path;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::downloads::sanitize;
use webkit::prelude::*;

use crate::browser::Browser;
use crate::downloads::file_name;
use crate::window::BrowserWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Mhtml,
    /// The main resource as served.
    File,
}

/// Beyond this many bytes a page title is cut short, at a character, in the suggested file
/// name, which keeps the name under the 255 bytes most file systems allow.
const TITLE_BYTES: usize = 200;

/// No response yet (a page still loading, the new tab page) reads as HTML.
fn format_of(mime: Option<&str>) -> Format {
    match mime {
        None | Some("text/html" | "application/xhtml+xml") => Format::Mhtml,
        Some(_) => Format::File,
    }
}

fn format_of_view(view: &webkit::WebView) -> Format {
    let mime = view.main_resource().and_then(|r| r.response()).and_then(|r| r.mime_type());
    format_of(mime.as_deref())
}

fn mhtml_name(title: &str) -> String {
    let title = title.replace(['/', '\\'], "-");
    let title = title[..title.floor_char_boundary(TITLE_BYTES)].trim();
    format!("{}.mhtml", sanitize(if title.is_empty() { "page" } else { title }))
}

/// What the save dialog offers: the page title for a page, the file's own name for a file.
fn suggested_name(view: &webkit::WebView, format: Format) -> String {
    match format {
        Format::Mhtml => mhtml_name(&view.title().unwrap_or_default()),
        Format::File => {
            let served = view.main_resource().and_then(|r| r.response()).and_then(|r| r.suggested_filename());
            let from_uri = || {
                let uri = view.uri()?;
                let url = url::Url::parse(&uri).ok()?;
                let last = url.path_segments()?.next_back()?.to_owned();
                Some(glib::uri_unescape_string(last.as_str(), None::<&str>).map_or(last, String::from))
            };
            sanitize(&served.map(String::from).or_else(from_uri).unwrap_or_default())
        }
    }
}

/// Saves what `view` shows to `path`. A file goes through the downloads list, which reports
/// how it ends; this returns once it has started.
pub(crate) async fn save(browser: &Browser, view: &webkit::WebView, path: &Path) -> Result<(), String> {
    save_as(browser, view, format_of_view(view), view.uri(), path).await
}

/// [`save`] as `format`, a file from `uri`: what `view` showed when the user chose to save,
/// so a page that moves on behind the dialog cannot change what lands under the name it offered.
async fn save_as(browser: &Browser, view: &webkit::WebView, format: Format, uri: Option<glib::GString>, path: &Path) -> Result<(), String> {
    match format {
        Format::Mhtml => view.save_to_file_future(&gio::File::for_path(path), webkit::SaveMode::Mhtml).await.map_err(|e| e.to_string()),
        Format::File => {
            let uri = uri.ok_or("the tab shows nothing")?;
            browser.downloads().download_to(view, &uri, path);
            Ok(())
        }
    }
}

/// `win.save-page`: asks where, then saves the selected tab.
pub(crate) fn present(window: &BrowserWindow) {
    let Some(tab) = window.selected_tab() else { return };
    let view = tab.web_view().clone();
    let (format, uri) = (format_of_view(&view), view.uri());
    let dialog = gtk::FileDialog::builder()
        .title("Save Page As")
        .initial_folder(&gio::File::for_path(window.browser().downloads().directory()))
        .initial_name(suggested_name(&view, format))
        .modal(true)
        .build();
    let window = window.clone();
    glib::spawn_future_local(async move {
        let Some(path) = dialog.save_future(Some(&window)).await.ok().and_then(|file| file.path()) else { return };
        match save_as(window.browser(), &view, format, uri, &path).await {
            Ok(()) if format == Format::Mhtml => window.toast(adw::Toast::new(&format!("Saved “{}”", file_name(&path)))),
            Ok(()) => {}
            Err(e) => window.toast(adw::Toast::new(&format!("Cannot save the page: {e}"))),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Reply, Server, browser, scratch_dir, wait_until};
    use crate::window::Focus;

    #[test]
    fn html_saves_as_mhtml_and_anything_else_as_itself() {
        assert_eq!(format_of(Some("text/html")), Format::Mhtml);
        assert_eq!(format_of(Some("application/xhtml+xml")), Format::Mhtml);
        assert_eq!(format_of(None), Format::Mhtml);
        assert_eq!(format_of(Some("application/pdf")), Format::File);
        assert_eq!(format_of(Some("image/png")), Format::File);
        assert_eq!(format_of(Some("text/plain")), Format::File);
    }

    #[test]
    fn a_page_is_named_by_its_title() {
        assert_eq!(mhtml_name("Vsesvit fixture"), "Vsesvit fixture.mhtml");
        assert_eq!(mhtml_name("A/B testing"), "A-B testing.mhtml");
        assert_eq!(mhtml_name("  "), "page.mhtml");
        assert_eq!(mhtml_name(&"x".repeat(400)), format!("{}.mhtml", "x".repeat(TITLE_BYTES)));
    }

    #[test]
    fn a_long_title_in_any_script_fits_a_file_name() {
        for title in ["日".repeat(120), "😀".repeat(100), format!("x{}", "é".repeat(200))] {
            let name = mhtml_name(&title);
            assert!(name.len() <= 255, "{} bytes", name.len());
            assert!(name.ends_with(".mhtml"));
        }
    }

    #[gtk::test]
    fn a_page_that_moves_on_behind_the_dialog_still_saves_what_it_offered() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/notes.txt" => Reply::Body("text/plain", b"plain words".to_vec()),
            _ => Reply::Page("Moved on"),
        });
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let (notes, page) = (server.url("/notes.txt"), server.url("/page"));
        let tab = window.open_tab(Some(&notes), None, Focus::Foreground);
        let view = tab.web_view();
        let shown = |url: &str| tab.committed_uri().as_deref() == Some(url) && !view.is_loading();
        wait_until("the file", || shown(&notes));
        let (format, uri) = (format_of_view(view), view.uri());
        tab.load(&page);
        wait_until("the page", || shown(&page));
        let path = scratch_dir("save-page").join("notes.txt");
        let saved = glib::MainContext::default().block_on(save_as(&browser, view, format, uri, &path));
        wait_until("the saved file", || std::fs::metadata(&path).is_ok_and(|m| m.len() > 0));
        let bytes = std::fs::read(&path).unwrap();
        window.destroy();
        assert_eq!(saved, Ok(()));
        assert_eq!(String::from_utf8_lossy(&bytes), "plain words");
    }
}
