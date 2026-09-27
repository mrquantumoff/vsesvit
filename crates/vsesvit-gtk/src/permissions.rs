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
    let site = tab
        .web_view()
        .uri()
        .map_or_else(|| "This page".to_owned(), |uri| describe_site(&uri));

    let dialog = adw::AlertDialog::new(Some(prompt.heading()), Some(&prompt.body(&site)));
    dialog.add_responses(&[("deny", "_Deny"), ("allow", "_Allow")]);
    dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("deny");
    let request = request.clone();
    dialog.choose(Some(&window), None::<&gio::Cancellable>, move |response| {
        if response == "allow" {
            request.allow();
        } else {
            request.deny();
        }
    });
    true
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
}
