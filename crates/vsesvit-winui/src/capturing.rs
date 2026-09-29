//! What a page captures (camera, microphone, screen), for the in-use indicators and their Stop
//! buttons. WebView2 reports no capture state, so a main-world script (`js/capture.js`) follows
//! the tracks the page's `getUserMedia` and `getDisplayMedia` calls return, and the shell polls
//! it through `ExecuteScript` while a capture may be live.
//!
//! This is the page's word: tracks from frames, from the legacy callback APIs or from a page that
//! tampers with the built-ins before the script's snapshot are not seen, so the indicators can
//! only under-report. Windows' own camera and microphone privacy indicator stays authoritative.

use serde::Deserialize;
use vsesvit_core::permissions::{Capturing, Permission};

pub(crate) const MAIN_WORLD_SCRIPT: &str = include_str!("js/capture.js");

pub(crate) const STATE_SCRIPT: &str =
    "window.__vsesvitCapture ? window.__vsesvitCapture.state() : null";

/// The capture a permission governs, as the script names it.
fn source(permission: Permission) -> Option<&'static str> {
    match permission {
        Permission::Camera => Some("camera"),
        Permission::Microphone => Some("microphone"),
        Permission::ScreenShare => Some("screen"),
        _ => None,
    }
}

/// Ends the page's tracks of `permission`'s capture; `None` for a permission with none.
pub(crate) fn stop_script(permission: Permission) -> Option<String> {
    source(permission)
        .map(|s| format!("window.__vsesvitCapture && window.__vsesvitCapture.stop({s:?})"))
}

/// Whether `capturing` includes `permission`'s capture.
pub(crate) fn uses(capturing: Capturing, permission: Permission) -> bool {
    match permission {
        Permission::Camera => capturing.camera,
        Permission::Microphone => capturing.microphone,
        Permission::ScreenShare => capturing.screen,
        _ => false,
    }
}

#[derive(Deserialize)]
struct State {
    camera: bool,
    microphone: bool,
    screen: bool,
}

/// `ExecuteScript`'s JSON result of [`STATE_SCRIPT`]; nothing for a page without the script.
pub(crate) fn parse_state(json: &str) -> Capturing {
    match serde_json::from_str::<Option<State>>(json) {
        Ok(Some(s)) => Capturing {
            camera: s.camera,
            microphone: s.microphone,
            screen: s.screen,
        },
        _ => Capturing::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_parses_or_is_nothing() {
        let state = parse_state(r#"{"camera":true,"microphone":false,"screen":true}"#);
        assert_eq!(
            state,
            Capturing {
                camera: true,
                microphone: false,
                screen: true
            }
        );
        assert_eq!(parse_state("null"), Capturing::default());
        assert_eq!(parse_state(r#"{"camera":"yes"}"#), Capturing::default());
    }

    #[test]
    fn stop_names_the_capture_of_its_permission() {
        assert_eq!(
            stop_script(Permission::ScreenShare).as_deref(),
            Some(r#"window.__vsesvitCapture && window.__vsesvitCapture.stop("screen")"#)
        );
        assert_eq!(stop_script(Permission::Location), None);
        assert!(uses(
            Capturing {
                microphone: true,
                ..Capturing::default()
            },
            Permission::Microphone
        ));
        assert!(!uses(
            Capturing {
                camera: true,
                ..Capturing::default()
            },
            Permission::Microphone
        ));
    }
}
