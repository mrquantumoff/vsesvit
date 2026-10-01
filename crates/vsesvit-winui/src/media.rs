//! The page side of the sidebar player: a main-world script (`js/media.js`) that keeps the
//! page's media session handlers and presents its video for picture-in-picture, and the calls
//! the shell makes into it through `ExecuteScript`. What the script reports is the page's word,
//! so it only decides what the player shows for that page: the shell itself loads the artwork,
//! so `parse_state` keeps only an http(s) address, whatever the script let through.

use serde::Deserialize;
use vsesvit_core::Url;

pub(crate) const MAIN_WORLD_SCRIPT: &str = include_str!("js/media.js");

/// The player's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaAction {
    PlayPause,
    Previous,
    Next,
}

impl MediaAction {
    fn name(self) -> &'static str {
        match self {
            MediaAction::PlayPause => "playpause",
            MediaAction::Previous => "previoustrack",
            MediaAction::Next => "nexttrack",
        }
    }
}

/// What a page is playing, from its media elements and media session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Playback {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    /// A video with frames: picture-in-picture can show it.
    pub video: bool,
    /// The page handles "previous track" / "next track".
    pub previous: bool,
    pub next: bool,
    /// The media session's largest artwork, an http(s) address; empty without one.
    #[serde(default)]
    pub artwork: String,
}

pub(crate) const STATE_SCRIPT: &str =
    "window.__vsesvitMedia ? window.__vsesvitMedia.state() : null";

pub(crate) fn act_script(action: MediaAction) -> String {
    format!(
        "window.__vsesvitMedia && window.__vsesvitMedia.act({:?})",
        action.name()
    )
}

pub(crate) fn pip_script(on: bool) -> String {
    format!("!!(window.__vsesvitMedia && window.__vsesvitMedia.pip({on}))")
}

/// `ExecuteScript`'s JSON result of [`STATE_SCRIPT`]; `None` for a page without the script.
pub(crate) fn parse_state(json: &str) -> Option<Playback> {
    let mut playback = serde_json::from_str::<Option<Playback>>(json).ok().flatten()?;
    playback.artwork = web_address(&playback.artwork);
    Some(playback)
}

/// `raw` if it is an http(s) address with a host, else nothing: never a file, a share or an
/// app resource, which the shell's own image loader would open.
fn web_address(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(url)
            if matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some_and(|h| !h.is_empty()) =>
        {
            url.into()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_parses_or_is_none() {
        let json = r#"{"playing":true,"title":"T","artist":"A","video":true,"previous":false,"next":true,"artwork":"https://a.test/art.png"}"#;
        assert_eq!(
            parse_state(json),
            Some(Playback {
                playing: true,
                title: "T".into(),
                artist: "A".into(),
                video: true,
                previous: false,
                next: true,
                artwork: "https://a.test/art.png".into(),
            })
        );
        assert_eq!(parse_state("null"), None);
        assert_eq!(parse_state(r#"{"playing":"yes"}"#), None);
    }

    #[test]
    fn artwork_must_be_web() {
        let artwork = |url: &str| {
            let json = serde_json::json!({
                "playing": true, "title": "", "artist": "", "video": false,
                "previous": false, "next": false, "artwork": url,
            });
            parse_state(&json.to_string()).unwrap().artwork
        };
        for url in [
            "file://attacker.example/share/a.png",
            "file:///C:/Windows/win.ini",
            r"\\attacker\share\a.png",
            "ms-appx:///x.png",
            "javascript:alert(1)",
            "https://",
            "/relative.png",
            "data:image/png;base64,AAAA",
        ] {
            assert_eq!(artwork(url), "", "{url}");
        }
        assert_eq!(artwork("https://a.test/art.png"), "https://a.test/art.png");
        assert_eq!(artwork("http://a.test/art.png"), "http://a.test/art.png");
    }

    #[test]
    fn scripts_name_their_action() {
        assert!(act_script(MediaAction::Next).contains(r#"act("nexttrack")"#));
        assert!(pip_script(false).contains("pip(false)"));
    }
}
