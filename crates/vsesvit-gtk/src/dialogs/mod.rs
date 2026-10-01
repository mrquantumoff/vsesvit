//! What the primary menu opens, all on core data: Bookmarks, History and Downloads, each in a
//! window of its own, and the Settings, Extensions, About, keyboard shortcuts and first-run
//! welcome dialogs. Adwaita keeps preferences in a dialog over the window.

pub(crate) mod about;
pub(crate) mod bookmarks;
pub(crate) mod downloads;
pub(crate) mod extensions;
pub(crate) mod history;
pub(crate) mod settings;
pub(crate) mod shortcut_settings;
pub(crate) mod shortcuts;
pub(crate) mod welcome;

use adw::prelude::*;
use gtk::glib;

use crate::window::BrowserWindow;

/// What opens in a window of its own, one of each at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Windowed {
    Bookmarks,
    History,
    Downloads,
}

/// Brings `kind`'s window forward, or shows the one `build` makes. The window belongs to
/// `opener`, the browser window it acts on, and closes with it.
pub(crate) fn present_window(opener: &BrowserWindow, kind: Windowed, build: impl FnOnce() -> adw::Window) {
    let browser = opener.browser();
    if let Some(window) = browser.windowed(kind) {
        window.present();
        return;
    }
    let window = build();
    window.set_application(Some(browser.app()));
    // Destroying a window unrealizes it at once; its `destroy` waits for every reference to go.
    opener.connect_unrealize(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    browser.add_windowed(kind, &window);
    window.present();
}

/// A searchable list window: header bar with extra buttons, a search entry, toasts, and a
/// content slot the caller fills.
pub(crate) struct LibraryWindow {
    pub(crate) window: adw::Window,
    pub(crate) search: gtk::SearchEntry,
    pub(crate) content: adw::Bin,
    pub(crate) toasts: adw::ToastOverlay,
}

impl LibraryWindow {
    pub(crate) fn new(title: &str, search_placeholder: &str, header_end: &[&gtk::Widget]) -> Self {
        let header = adw::HeaderBar::new();
        for widget in header_end {
            header.pack_end(*widget);
        }
        let search = gtk::SearchEntry::builder()
            .placeholder_text(search_placeholder)
            .hexpand(true)
            .build();
        let search_row = adw::Clamp::builder()
            .maximum_size(560)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .child(&search)
            .build();
        let content = adw::Bin::new();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&content));
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&search_row);
        toolbar.set_content(Some(&toasts));
        let window = adw::Window::builder()
            .title(title)
            .default_width(640)
            .default_height(680)
            .content(&toolbar)
            .build();
        LibraryWindow { window, search, content, toasts }
    }

    pub(crate) fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }
}

/// Asks for a line of text. `None` when cancelled or left empty.
pub(crate) async fn prompt_text(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    initial: &str,
    accept: &str,
) -> Option<String> {
    let entry = gtk::Entry::builder()
        .text(initial)
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "_Cancel"), ("accept", accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");
    entry.grab_focus();
    let response = dialog.choose_future(Some(parent)).await;
    let text = entry.text().trim().to_owned();
    (response == "accept" && !text.is_empty()).then_some(text)
}

/// Asks the user to pick one of `options`. `None` when cancelled.
pub(crate) async fn prompt_choice(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    options: &[&str],
    accept: &str,
) -> Option<u32> {
    let chooser = gtk::DropDown::from_strings(options);
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.set_extra_child(Some(&chooser));
    dialog.add_responses(&[("cancel", "_Cancel"), ("accept", accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");
    let response = dialog.choose_future(Some(parent)).await;
    (response == "accept").then(|| chooser.selected())
}

/// Yes-or-no, with a destructive accept button.
pub(crate) async fn confirm(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    accept: &str,
) -> bool {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_responses(&[("cancel", "_Cancel"), ("accept", accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.choose_future(Some(parent)).await == "accept"
}

/// A visit time in the user's locale.
pub(crate) fn format_time(unix_ms: i64) -> String {
    glib::DateTime::from_unix_local(unix_ms.div_euclid(1000))
        .ok()
        .and_then(|t| t.format("%x %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{browser, wait_until};

    #[gtk::test]
    fn a_window_of_its_own_opens_once_and_closes_with_its_browser_window() {
        let browser = browser();
        let first = BrowserWindow::new(&browser);
        let second = BrowserWindow::new(&browser);
        first.present();
        second.present();
        history::present(&first);
        let history = browser.windowed(Windowed::History).expect("History has a window");
        history::present(&second);
        assert_eq!(browser.windowed(Windowed::History), Some(history.clone()), "asking again from any window shows the same one");
        assert_eq!(browser.windows().len(), 2, "it is no browser window");

        second.destroy();
        assert!(history.is_visible(), "it stays open while the window it was opened from does");
        first.destroy();
        wait_until("History closes with its browser window", || !history.is_visible());
        drop(history);
        assert_eq!(browser.windowed(Windowed::History), None);
    }
}
