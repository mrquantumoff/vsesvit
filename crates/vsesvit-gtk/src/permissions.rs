//! Site permission requests. Location, notifications, camera, microphone and screen sharing
//! ask the user; everything else gets a fixed answer.

use adw::prelude::*;
use gtk::gio;
use url::Url;
use webkit::prelude::*;

use crate::tab::Tab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prompt {
    Location,
    Notifications,
    Camera,
    Microphone,
    CameraAndMicrophone,
    Screen,
}

enum Decision {
    Ask(Prompt),
    Allow,
    Deny,
}

impl Prompt {
    fn heading(self) -> &'static str {
        match self {
            Prompt::Location => "Share Your Location?",
            Prompt::Notifications => "Allow Notifications?",
            Prompt::Camera => "Allow Camera Access?",
            Prompt::Microphone => "Allow Microphone Access?",
            Prompt::CameraAndMicrophone => "Allow Camera and Microphone Access?",
            Prompt::Screen => "Share Your Screen?",
        }
    }

    fn body(self, site: &str) -> String {
        let what = match self {
            Prompt::Location => "wants to know your location",
            Prompt::Notifications => "wants to show desktop notifications",
            Prompt::Camera => "wants to use your camera",
            Prompt::Microphone => "wants to use your microphone",
            Prompt::CameraAndMicrophone => "wants to use your camera and microphone",
            Prompt::Screen => "wants to see your screen",
        };
        format!("{site} {what}.")
    }
}

/// Handles `permission-request`. Always returns true: every request gets an answer.
pub(crate) fn handle(tab: &Tab, request: &webkit::PermissionRequest) -> bool {
    let prompt = match decide(request) {
        Decision::Allow => {
            request.allow();
            return true;
        }
        Decision::Deny => {
            request.deny();
            return true;
        }
        Decision::Ask(prompt) => prompt,
    };
    let Some(window) = tab.window() else {
        request.deny();
        return true;
    };
    let asked_by = requesting_document(tab);
    let site = asked_by
        .as_deref()
        .map_or_else(|| "This page".to_owned(), describe_site);

    let dialog = adw::AlertDialog::new(Some(prompt.heading()), Some(&prompt.body(&site)));
    dialog.add_responses(&[("deny", "_Deny"), ("allow", "_Allow")]);
    dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("deny");
    let request = request.clone();
    let tab = tab.downgrade();
    dialog.choose(Some(&window), None::<&gio::Cancellable>, move |response| {
        let on_screen = tab.upgrade().and_then(|tab| tab.committed_uri());
        if grants(&response, asked_by.as_deref(), on_screen.as_deref()) {
            request.allow();
        } else {
            request.deny();
        }
    });
    true
}

/// The page asking is the one on screen. During a provisional load the web view's URI is
/// already the one being requested, which has not run anything yet.
fn requesting_document(tab: &Tab) -> Option<String> {
    tab.committed_uri()
}

/// An Allow counts only while the site the prompt named is still the one on screen.
fn grants(response: &str, asked_by: Option<&str>, on_screen: Option<&str>) -> bool {
    response == "allow" && asked_by.is_some() && site_of(asked_by) == site_of(on_screen)
}

fn site_of(uri: Option<&str>) -> Option<(String, Option<String>, Option<u16>)> {
    let url = Url::parse(uri?).ok()?;
    Some((
        url.scheme().to_owned(),
        url.host_str().map(str::to_owned),
        url.port_or_known_default(),
    ))
}

fn decide(request: &webkit::PermissionRequest) -> Decision {
    if request.is::<webkit::GeolocationPermissionRequest>() {
        return Decision::Ask(Prompt::Location);
    }
    if request.is::<webkit::NotificationPermissionRequest>() {
        return Decision::Ask(Prompt::Notifications);
    }
    if let Some(media) = request.downcast_ref::<webkit::UserMediaPermissionRequest>() {
        if webkit::functions::user_media_permission_is_for_display_device(media) {
            return Decision::Ask(Prompt::Screen);
        }
        return Decision::Ask(
            match (media.is_for_video_device(), media.is_for_audio_device()) {
                (true, true) => Prompt::CameraAndMicrophone,
                (true, false) => Prompt::Camera,
                _ => Prompt::Microphone,
            },
        );
    }
    if request.is::<webkit::PointerLockPermissionRequest>() {
        // Pointer lock is granted without asking, as in other browsers; Esc releases it.
        return Decision::Allow;
    }
    Decision::Deny
}

fn describe_site(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(url) if url.scheme() == "file" => "This file".to_owned(),
        Ok(url) => url
            .host_str()
            .map_or_else(|| "This page".to_owned(), |host| format!("“{host}”")),
        Err(_) => "This page".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_named_by_host() {
        assert_eq!(
            describe_site("https://maps.example.com/here"),
            "“maps.example.com”"
        );
        assert_eq!(describe_site("file:///tmp/x.html"), "This file");
        assert_eq!(describe_site("about:blank"), "This page");
    }

    #[test]
    fn prompts_read_as_sentences() {
        assert_eq!(
            Prompt::Location.body("“a.example”"),
            "“a.example” wants to know your location."
        );
    }

    #[test]
    fn an_answer_holds_only_for_the_site_it_was_asked_for() {
        assert!(grants("allow", Some("https://a.example/x"), Some("https://a.example/y#z")));
        assert!(grants("allow", Some("file:///tmp/a.html"), Some("file:///tmp/a.html")));
        assert!(!grants("allow", Some("https://a.example/"), Some("https://b.example/")));
        assert!(!grants("allow", Some("https://a.example/"), Some("http://a.example/")));
        assert!(!grants("allow", Some("https://a.example/"), None));
        assert!(!grants("deny", Some("https://a.example/"), Some("https://a.example/")));
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
        wait_until("the next load to start", || {
            tab.web_view().uri().as_deref() == Some(pending.as_str())
        });
        let site = requesting_document(&tab).as_deref().map(describe_site);
        window.destroy();
        assert_eq!(site.as_deref(), Some("“127.0.0.1”"));
    }
}
