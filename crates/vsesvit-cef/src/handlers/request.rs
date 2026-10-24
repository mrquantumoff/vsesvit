//! Owned by W1.3.
//! `RequestHandler`: `on_before_browse` asks the shell's `navigation_policy`, `on_open_urlfrom_tab`
//! hands middle and Ctrl clicks to the shell, certificate errors and renderer ends become `TabEvent`s
//! (all UI thread). The IO-thread getters delegate: resource requests to `resource_request.rs` and
//! auth to `auth.rs`.
#![allow(clippy::unnecessary_cast, reason = "CEF's enum integers are i32 on Windows and u32 on Linux")]

use std::sync::Arc;
use std::time::Duration;

use cef::*;
use vsesvit_engine::{
    BrowserEvent, CertStatus, NavigationDecision, NavigationRequest, TabEvent, TerminationStatus, Url,
};

use crate::client::HandlerCtx;
use crate::handlers::display::to_tab;
use crate::handlers::life_span::disposition_of;
use crate::handlers::load::transition_of;
use crate::registry::{self, BrowserId};
use crate::strings;

/// What a navigation the page or the shell started looks like to `on_before_browse`.
pub struct Browse {
    /// Where to.
    pub url: String,
    /// The main frame.
    pub main_frame: bool,
    /// A server redirect.
    pub is_redirect: bool,
    /// A user gesture started it.
    pub user_gesture: bool,
    /// CEF's transition bits.
    pub transition: u32,
}

/// The logic of `on_before_browse`: true cancels the navigation. Only tabs ask the shell; a cancelled
/// navigation with a redirect target loads it from a task, outside this callback.
pub fn before_browse(browser: BrowserId, browse: &Browse) -> bool {
    let found = registry::with(|r| {
        let entry = r.entries.get(&browser)?;
        let tab = entry.kind.tab()?;
        Some((tab, entry.shell_navigation, r.hooks.clone()?, r.sink.clone(), r.backend.clone()))
    })
    .flatten();
    let Some((tab, browser_initiated, hooks, sink, backend)) = found else { return false };
    let Ok(url) = Url::parse(&browse.url) else { return false };
    let request = NavigationRequest {
        tab,
        url: url.clone(),
        is_main_frame: browse.main_frame,
        is_redirect: browse.is_redirect,
        user_gesture: browse.user_gesture,
        browser_initiated,
        transition: transition_of(browse.transition),
    };
    match hooks.navigation_policy(&request) {
        NavigationDecision::Allow => {
            if browse.main_frame {
                sink.tab(tab, TabEvent::NavigationStarted { url, is_redirect: browse.is_redirect });
            }
            false
        }
        NavigationDecision::Cancel => true,
        NavigationDecision::Redirect(to) => {
            let to = to.to_string();
            backend.post(
                Duration::ZERO,
                Box::new(move || {
                    if let Some(page) = registry::browser(browser) {
                        page.load(&to);
                    }
                }),
            );
            true
        }
    }
}

/// `Engine::load`.
pub fn load(tab: vsesvit_engine::TabId, url: &Url) {
    registry::navigate(tab, |page| page.load(url.as_str()));
}

/// `Engine::reload`.
pub fn reload(tab: vsesvit_engine::TabId, bypass_cache: bool) {
    registry::navigate(tab, |page| page.reload(bypass_cache));
}

/// `Engine::go_back`.
pub fn go_back(tab: vsesvit_engine::TabId) {
    registry::navigate(tab, |page| page.go_back());
}

/// `Engine::go_forward`.
pub fn go_forward(tab: vsesvit_engine::TabId) {
    registry::navigate(tab, |page| page.go_forward());
}

/// `Engine::stop`.
pub fn stop(tab: vsesvit_engine::TabId) {
    registry::on_tab(tab, |page| page.stop());
}

/// `Engine::start_download`.
pub fn start_download(tab: vsesvit_engine::TabId, url: &Url) {
    registry::on_tab(tab, |page| page.start_download(url.as_str()));
}

/// The logic of `on_open_urlfrom_tab`: the shell opens it. True cancels Chromium's own handling.
pub fn open_url_from_tab(browser: BrowserId, url: &str, disposition: i64, user_gesture: bool) -> bool {
    let Some(Some((opener, sink))) = registry::with(|r| Some((r.tab_of(browser)?, r.sink.clone()))) else { return false };
    let Ok(url) = Url::parse(url) else { return true };
    sink.send(BrowserEvent::OpenUrlRequested { opener, url, disposition: disposition_of(disposition), user_gesture });
    true
}

/// The contract's certificate status for CEF's `cef_cert_status_t` bits.
pub fn cert_status_of(bits: u32) -> CertStatus {
    use sys::cef_cert_status_t as C;
    let has = |flag: C| bits & flag as u32 != 0;
    let mut status = CertStatus::NONE;
    let table = [
        (C::CERT_STATUS_COMMON_NAME_INVALID, CertStatus::NAME_INVALID),
        (C::CERT_STATUS_NON_UNIQUE_NAME, CertStatus::NAME_INVALID),
        (C::CERT_STATUS_NAME_CONSTRAINT_VIOLATION, CertStatus::NAME_INVALID),
        (C::CERT_STATUS_DATE_INVALID, CertStatus::DATE_INVALID),
        (C::CERT_STATUS_VALIDITY_TOO_LONG, CertStatus::DATE_INVALID),
        (C::CERT_STATUS_AUTHORITY_INVALID, CertStatus::AUTHORITY_INVALID),
        (C::CERT_STATUS_REVOKED, CertStatus::REVOKED),
        (C::CERT_STATUS_WEAK_SIGNATURE_ALGORITHM, CertStatus::WEAK),
        (C::CERT_STATUS_WEAK_KEY, CertStatus::WEAK),
        (C::CERT_STATUS_INVALID, CertStatus::INVALID),
        (C::CERT_STATUS_PINNED_KEY_MISSING, CertStatus::OTHER),
        (C::CERT_STATUS_CT_COMPLIANCE_FAILED, CertStatus::OTHER),
    ];
    for (flag, ours) in table {
        if has(flag) {
            status = status | ours;
        }
    }
    status
}

/// Why a renderer ended, from CEF's status.
pub fn termination_of(raw: i64, error_code: i32) -> TerminationStatus {
    let is = |t: cef::TerminationStatus| raw == i64::from(t.get_raw());
    if is(cef::TerminationStatus::PROCESS_CRASHED) {
        TerminationStatus::Crashed
    } else if is(cef::TerminationStatus::PROCESS_WAS_KILLED) {
        TerminationStatus::Killed
    } else if is(cef::TerminationStatus::PROCESS_OOM) {
        TerminationStatus::OutOfMemory
    } else {
        TerminationStatus::Other(error_code)
    }
}

wrap_request_handler! {
    pub struct Request {
        ctx: Arc<HandlerCtx>,
    }

    impl RequestHandler {
        fn on_before_browse(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut cef::Request>,
                            user_gesture: i32, is_redirect: i32) -> i32 {
            let (Some(browser), Some(frame), Some(request)) = (browser, frame, request) else { return 0 };
            let browse = Browse {
                url: strings::userfree(&request.url()),
                main_frame: frame.is_main() != 0,
                is_redirect: is_redirect != 0,
                user_gesture: user_gesture != 0,
                transition: request.transition_type().get_raw() as u32,
            };
            before_browse(browser.identifier(), &browse) as i32
        }

        fn on_open_urlfrom_tab(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, target_url: Option<&CefString>,
                               target_disposition: WindowOpenDisposition, user_gesture: i32) -> i32 {
            let Some(browser) = browser else { return 0 };
            open_url_from_tab(browser.identifier(), &strings::text(target_url), i64::from(target_disposition.get_raw()), user_gesture != 0) as i32
        }

        fn resource_request_handler(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut cef::Request>,
                                    is_navigation: i32, is_download: i32, request_initiator: Option<&CefString>,
                                    disable_default_handling: Option<&mut i32>) -> Option<ResourceRequestHandler> {
            super::resource_request::handler(&self.ctx, browser, frame, request, is_navigation != 0, is_download != 0,
                request_initiator, disable_default_handling)
        }

        fn auth_credentials(&self, browser: Option<&mut Browser>, origin_url: Option<&CefString>, is_proxy: i32, host: Option<&CefString>,
                            port: i32, realm: Option<&CefString>, scheme: Option<&CefString>, callback: Option<&mut AuthCallback>) -> i32 {
            super::auth::get_auth_credentials(&self.ctx, browser, origin_url, is_proxy != 0, host, port, realm, scheme, callback) as i32
        }

        fn on_certificate_error(&self, browser: Option<&mut Browser>, _cert_error: Errorcode, request_url: Option<&CefString>,
                                ssl_info: Option<&mut Sslinfo>, _callback: Option<&mut Callback>) -> i32 {
            let Some(browser) = browser else { return 0 };
            let status = ssl_info.map(|s| cert_status_of(s.cert_status().get_raw() as u32)).unwrap_or(CertStatus::OTHER);
            if let Ok(url) = Url::parse(&strings::text(request_url)) {
                to_tab(browser.identifier(), TabEvent::CertificateError { url, status, der_chain: Vec::new() });
            }
            0
        }

        fn on_render_process_terminated(&self, browser: Option<&mut Browser>, status: cef::TerminationStatus, error_code: i32,
                                        _error_string: Option<&CefString>) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::RenderProcessGone(termination_of(i64::from(status.get_raw()), error_code)));
            }
        }
    }
}

impl Request {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> RequestHandler {
        Request::new(ctx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::cef_cert_status_t as C;

    #[test]
    fn certificate_bits_map() {
        assert_eq!(cert_status_of(0), CertStatus::NONE);
        let bits = C::CERT_STATUS_AUTHORITY_INVALID as u32 | C::CERT_STATUS_DATE_INVALID as u32;
        assert_eq!(cert_status_of(bits), CertStatus::AUTHORITY_INVALID | CertStatus::DATE_INVALID);
        assert_eq!(cert_status_of(C::CERT_STATUS_COMMON_NAME_INVALID as u32), CertStatus::NAME_INVALID);
        assert_eq!(cert_status_of(C::CERT_STATUS_REVOKED as u32), CertStatus::REVOKED);
        assert_eq!(cert_status_of(C::CERT_STATUS_WEAK_KEY as u32), CertStatus::WEAK);
        assert_eq!(cert_status_of(C::CERT_STATUS_IS_EV as u32), CertStatus::NONE, "EV is no error");
    }

    #[test]
    fn terminations_map() {
        let raw = |t: cef::TerminationStatus| i64::from(t.get_raw());
        assert_eq!(termination_of(raw(cef::TerminationStatus::PROCESS_CRASHED), 0), TerminationStatus::Crashed);
        assert_eq!(termination_of(raw(cef::TerminationStatus::PROCESS_WAS_KILLED), 0), TerminationStatus::Killed);
        assert_eq!(termination_of(raw(cef::TerminationStatus::PROCESS_OOM), 0), TerminationStatus::OutOfMemory);
        assert_eq!(termination_of(raw(cef::TerminationStatus::LAUNCH_FAILED), 7), TerminationStatus::Other(7));
    }
}
