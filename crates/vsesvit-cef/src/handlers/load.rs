//! Owned by W1.3.
//! `LoadHandler` (UI thread): loading state, commits, load ends and failures. `on_load_start` is CEF's
//! commit signal for a new document: it raises the browser's generation and ends the old document's
//! prompts (`Navigated`, design section 1.5), then reports `TabEvent::Committed`.

use std::sync::Arc;

use cef::*;
use vsesvit_engine::{CertStatus, CommitKind, EngineTransition, NetError, SslInfo, TabEvent, Url};

use crate::client::HandlerCtx;
use crate::handlers::display::to_tab;
use crate::handlers::request::cert_status_of;
use crate::registry::{self, BrowserId};
use crate::{prompts, strings};

/// How a navigation started, from CEF's transition bits.
pub fn transition_of(raw: u32) -> EngineTransition {
    let forward_back = TransitionType::FORWARD_BACK_FLAG.get_raw() as u32;
    if raw & forward_back != 0 {
        return EngineTransition::BackForward;
    }
    let source = raw & TransitionType::SOURCE_MASK.get_raw() as u32;
    let is = |t: TransitionType| source == t.get_raw() as u32;
    if is(TransitionType::LINK) {
        EngineTransition::Link
    } else if is(TransitionType::EXPLICIT) {
        EngineTransition::Typed
    } else if is(TransitionType::AUTO_BOOKMARK) {
        EngineTransition::AutoBookmark
    } else if is(TransitionType::RELOAD) {
        EngineTransition::Reload
    } else if is(TransitionType::FORM_SUBMIT) {
        EngineTransition::FormSubmit
    } else if is(TransitionType::GENERATED) || is(TransitionType::KEYWORD) || is(TransitionType::KEYWORD_GENERATED) {
        EngineTransition::Generated
    } else {
        EngineTransition::Other
    }
}

/// The main frame committed a new document.
pub fn committed(browser: BrowserId, url: &str, transition: EngineTransition) {
    prompts::new_document(browser);
    registry::with(|r| {
        if let Some(e) = r.entries.get_mut(&browser) {
            e.shell_navigation = false;
            e.last_seen = "on_load_start";
        }
    });
    if let Ok(url) = Url::parse(url) {
        to_tab(browser, TabEvent::Committed { url, kind: CommitKind::Document, transition, mime: None });
    }
}

/// The main frame failed to load. Aborted loads (`ERR_ABORTED`, -3) are not failures.
pub fn failed(browser: BrowserId, code: i32, text: &str, url: &str) {
    if code == -3 {
        return;
    }
    let Ok(url) = Url::parse(url) else { return };
    let name = text.trim_start_matches("net::").to_owned();
    to_tab(browser, TabEvent::LoadFailed { url, error: NetError { code, name, description: text.to_owned() } });
}

fn ssl_of(browser: &Browser) -> Option<SslInfo> {
    let status = browser.host()?.visible_navigation_entry()?.sslstatus()?;
    let content = status.content_status().get_raw() as u32;
    let mixed = content
        & (sys::cef_ssl_content_status_t::SSL_CONTENT_DISPLAYED_INSECURE_CONTENT as u32
            | sys::cef_ssl_content_status_t::SSL_CONTENT_RAN_INSECURE_CONTENT as u32)
        != 0;
    let cert = cert_status_of(status.cert_status().get_raw() as u32);
    Some(SslInfo { secure: status.is_secure_connection() != 0 && cert == CertStatus::NONE, status: cert, der_chain: Vec::new(), mixed_content: mixed })
}

wrap_load_handler! {
    pub struct Load {
        ctx: Arc<HandlerCtx>,
    }

    impl LoadHandler {
        fn on_loading_state_change(&self, browser: Option<&mut Browser>, is_loading: i32, can_go_back: i32, can_go_forward: i32) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::LoadingStateChanged {
                    loading: is_loading != 0,
                    can_go_back: can_go_back != 0,
                    can_go_forward: can_go_forward != 0,
                });
            }
        }

        fn on_load_start(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, transition_type: TransitionType) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            if frame.is_main() != 0 {
                committed(browser.identifier(), &strings::userfree(&frame.url()), transition_of(transition_type.get_raw() as u32));
            }
        }

        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status_code: i32) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            if frame.is_main() != 0 {
                registry::seen(browser.identifier(), "on_load_end");
                to_tab(browser.identifier(), TabEvent::SecurityChanged(ssl_of(browser)));
            }
        }

        fn on_load_error(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode,
                         error_text: Option<&CefString>, failed_url: Option<&CefString>) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            if frame.is_main() != 0 {
                failed(browser.identifier(), error_code.get_raw(), &strings::text(error_text), &strings::text(failed_url));
            }
        }
    }
}

impl Load {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> LoadHandler {
        Load::new(ctx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_map() {
        let raw = |t: TransitionType| t.get_raw() as u32;
        assert_eq!(transition_of(raw(TransitionType::LINK)), EngineTransition::Link);
        assert_eq!(transition_of(raw(TransitionType::EXPLICIT)), EngineTransition::Typed);
        assert_eq!(transition_of(raw(TransitionType::AUTO_BOOKMARK)), EngineTransition::AutoBookmark);
        assert_eq!(transition_of(raw(TransitionType::RELOAD)), EngineTransition::Reload);
        assert_eq!(transition_of(raw(TransitionType::FORM_SUBMIT)), EngineTransition::FormSubmit);
        assert_eq!(transition_of(raw(TransitionType::GENERATED)), EngineTransition::Generated);
        assert_eq!(transition_of(raw(TransitionType::AUTO_TOPLEVEL)), EngineTransition::Other);
        let back = raw(TransitionType::LINK) | raw(TransitionType::FORWARD_BACK_FLAG);
        assert_eq!(transition_of(back), EngineTransition::BackForward);
        let redirect = raw(TransitionType::EXPLICIT) | raw(TransitionType::SERVER_REDIRECT_FLAG);
        assert_eq!(transition_of(redirect), EngineTransition::Typed, "qualifiers do not change the source");
    }
}
