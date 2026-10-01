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
    match format_of_view(view) {
        Format::Mhtml => view.save_to_file_future(&gio::File::for_path(path), webkit::SaveMode::Mhtml).await.map_err(|e| e.to_string()),
        Format::File => {
            let uri = view.uri().ok_or("the tab shows nothing")?;
            browser.downloads().download_to(view, &uri, path);
            Ok(())
        }
    }
}

/// `win.save-page`: asks where, then saves the selected tab.
pub(crate) fn present(window: &BrowserWindow) {
    let Some(tab) = window.selected_tab() else { return };
    let view = tab.web_view().clone();
    let format = format_of_view(&view);
    let dialog = gtk::FileDialog::builder()
        .title("Save Page As")
        .initial_folder(&gio::File::for_path(window.browser().downloads().directory()))
        .initial_name(suggested_name(&view, format))
        .modal(true)
        .build();
    let window = window.clone();
    glib::spawn_future_local(async move {
        let Some(path) = dialog.save_future(Some(&window)).await.ok().and_then(|file| file.path()) else { return };
        match save(window.browser(), &view, &path).await {
            Ok(()) if format == Format::Mhtml => window.toast(adw::Toast::new(&format!("Saved “{}”", file_name(&path)))),
            Ok(()) => {}
            Err(e) => window.toast(adw::Toast::new(&format!("Cannot save the page: {e}"))),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
