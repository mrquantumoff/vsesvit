//! WebViews for extension pages: the hidden background view and action popups. Each
//! gets its own `UserContentManager` with the page shim and a message handler in the
//! default world, the manifest's CSP, CORS for host permissions, and its main frame
//! pinned to the extension origin (anything else opens as a tab).

use std::rc::Rc;

use gtk::prelude::*;
use serde_json::json;
use webkit::{gio, glib};
use webkit::prelude::*;

use crate::bridge::{self, Origin};
use crate::extension::{ExtView, Extension, OpeningPopup, ViewId, ViewKind};
use crate::lifecycle::InstallEvent;
use crate::{patterns, protocol};
use crate::runtime::Inner;

pub(crate) fn build(inner: &Rc<Inner>, ext: &Rc<Extension>, kind: ViewKind) -> webkit::WebView {
    let view_id = inner.next_view_id();
    let ucm = webkit::UserContentManager::new();
    ucm.add_script(&ext.page_script);
    bridge::register(inner, &ucm, ext, Origin::Page { view: view_id }, None);

    let builder = webkit::WebView::builder()
        .user_content_manager(&ucm)
        .web_extension_mode(ext.web_extension_mode())
        .default_content_security_policy(&ext.csp);
    // A related view shares the background's web process and inherits its context and
    // session; WebKit refuses those properties alongside `related-view`.
    let related = (kind != ViewKind::Background).then(|| ext.background.borrow().clone()).flatten();
    let view = match related {
        Some(bg) => builder.related_view(&bg).build(),
        None => builder.network_session(&inner.session).web_context(&inner.context).build(),
    };

    let allowlist: Vec<&str> = ext.host_permissions.iter().map(String::as_str).collect();
    let cors_bypass = !allowlist.is_empty();
    if cors_bypass {
        view.set_cors_allowlist(&allowlist);
    }
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(std::env::var_os("VSESVIT_WEBEXT_CONSOLE").is_some());
    }

    let opens_popups = kind == ViewKind::Background && ext.background_is_page();
    if opens_popups {
        adopt_popups(inner, ext, &view);
    }

    let weak_inner = Rc::downgrade(inner);
    let weak_ext = Rc::downgrade(ext);
    let base = ext.base_url.clone();
    let ext_id = ext.id.as_str().to_owned();
    view.connect_decide_policy(move |_, decision, decision_type| {
        let inside = |uri: &str| patterns::under_base(&base, uri) || uri.starts_with("about:") || uri.starts_with("blob:") || uri.starts_with("data:");
        let leave = |uri: &str, inside: bool| {
            if (uri.starts_with("http://") || uri.starts_with("https://") || inside)
                && let Some(inner) = weak_inner.upgrade()
            {
                inner.open_tab(uri);
            }
        };
        match decision_type {
            webkit::PolicyDecisionType::NavigationAction | webkit::PolicyDecisionType::NewWindowAction => {
                let Some(nav) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else { return false };
                let action = nav.navigation_action();
                let uri = action.as_ref().and_then(|a| a.request()).and_then(|r| r.uri()).map(String::from).unwrap_or_default();
                let inside = inside(&uri);
                if opens_popups && decision_type == webkit::PolicyDecisionType::NewWindowAction {
                    // The only window a background opens is the popup `open_popup` asked for.
                    if weak_ext.upgrade().is_some_and(|ext| ext.opening_popup.borrow().as_ref().is_some_and(|p| p.url == uri)) {
                        return false;
                    }
                    decision.ignore();
                    return true;
                }
                if decision_type == webkit::PolicyDecisionType::NavigationAction {
                    if inside {
                        return false;
                    }
                    // WebKitGTK raises this for subframe loads too and does not say which
                    // frame. Only what the user did in the page (a link, a form) is taken
                    // as leaving it here; anything else is judged at the response, where
                    // the frame is known.
                    let user_driven = action.is_some_and(|a| {
                        matches!(a.navigation_type(), webkit::NavigationType::LinkClicked | webkit::NavigationType::FormSubmitted | webkit::NavigationType::FormResubmitted)
                    });
                    if !user_driven {
                        return false;
                    }
                }
                decision.ignore();
                leave(&uri, inside);
                true
            }
            webkit::PolicyDecisionType::Response => {
                let Some(response) = decision.downcast_ref::<webkit::ResponsePolicyDecision>() else { return false };
                let uri = response.response().and_then(|r| r.uri()).map(String::from).unwrap_or_default();
                if inside(&uri) {
                    return false;
                }
                if response.is_main_frame_main_resource() {
                    decision.ignore();
                    leave(&uri, false);
                    return true;
                }
                // A subframe outside the extension origin. The CORS allowlist applies to
                // every frame of the view (WebKit checks the page, not the frame), so a
                // third-party frame would get the extension's host-permission fetches,
                // which Chrome gives only to the extension's own frames.
                if cors_bypass {
                    log::debug!("{ext_id}: refused subframe {uri} in an extension view with host permissions");
                    decision.ignore();
                    return true;
                }
                false
            }
            _ => false,
        }
    });

    // A closed popup or a crashed page takes its ports with it. Not while the view is
    // being torn down, which may happen inside a runtime call.
    let close_ports = {
        let weak_inner = Rc::downgrade(inner);
        move || {
            let weak_inner = weak_inner.clone();
            glib::idle_add_local_once(move || {
                if let Some(inner) = weak_inner.upgrade() {
                    inner.close_ports(|c| c.origin == Origin::Page { view: view_id });
                }
            });
        }
    };
    let ext_id = ext.id.as_str().to_owned();
    view.connect_web_process_terminated({
        let close_ports = close_ports.clone();
        move |_, reason| {
            log::warn!("{ext_id}: extension page web process terminated: {reason:?}");
            close_ports();
        }
    });
    view.connect_destroy(move |_| close_ports());

    ext.views.borrow_mut().push(ExtView { id: view_id, kind, view: view.downgrade() });
    view
}

/// An action popup at `url`, handed to `show` once it exists (already loading). A page
/// background opens it with `window.open`, so the popup's `opener` is the background page:
/// WebKit lets a page reach another view's window only through that relationship, and
/// `runtime.getBackgroundPage` needs it. Any other popup is a plain view.
pub(crate) fn open_popup(inner: &Rc<Inner>, ext: &Rc<Extension>, url: String, show: Box<dyn FnOnce(webkit::WebView)>) {
    let background = ext.background.borrow().clone().filter(|_| ext.background_is_page());
    let Some(background) = background else {
        let view = build(inner, ext, ViewKind::Popup);
        view.load_uri(&url);
        return show(view);
    };
    let source = format!("window.open({}, \"_blank\"); undefined", protocol::js_string(&url));
    *ext.opening_popup.borrow_mut() = Some(OpeningPopup { url, show });
    let (weak_inner, weak_ext) = (Rc::downgrade(inner), Rc::downgrade(ext));
    ext.when_background_loaded(move || {
        background.evaluate_javascript(&source, None, None, None::<&gio::Cancellable>, move |result| {
            if let Err(e) = result {
                log::debug!("window.open in the background: {e}");
            }
            // The background did not open it (its page is gone, or WebKit refused): the
            // popup opens on its own, without an opener.
            let (Some(inner), Some(ext)) = (weak_inner.upgrade(), weak_ext.upgrade()) else { return };
            let Some(pending) = ext.opening_popup.borrow_mut().take() else { return };
            if inner.extension(&ext.id).is_some_and(|loaded| Rc::ptr_eq(&loaded, &ext)) {
                let view = build(&inner, &ext, ViewKind::Popup);
                view.load_uri(&pending.url);
                (pending.show)(view);
            }
        });
    });
}

/// The popup [`open_popup`] asked the background for is built when WebKit creates it, as
/// WebKit requires, and shown once WebKit says it may be. WebKit gives it the background's
/// configuration, message handler included (see [`Extension::caller_view`]).
fn adopt_popups(inner: &Rc<Inner>, ext: &Rc<Extension>, background: &webkit::WebView) {
    if let Some(settings) = WebViewExt::settings(background) {
        settings.set_javascript_can_open_windows_automatically(true);
    }
    let (weak_inner, weak_ext) = (Rc::downgrade(inner), Rc::downgrade(ext));
    connect_create(background, move |_, action| {
        let (inner, ext) = (weak_inner.upgrade()?, weak_ext.upgrade()?);
        let uri = action.request().and_then(|r| r.uri()).map(String::from).unwrap_or_default();
        let pending = ext.opening_popup.borrow_mut().take_if(|p| p.url == uri)?;
        let popup = build(&inner, &ext, ViewKind::Popup);
        ext.adopted_popup.set(Some(&popup));
        // The popup holds itself until it is shown, and then whoever it is shown to does.
        let show = std::cell::Cell::new(Some((pending.show, popup.clone())));
        popup.connect_ready_to_show(move |_| {
            if let Some((show, popup)) = show.take() {
                show(popup);
            }
        });
        Some(popup)
    });
}

/// Connects `create` to `view`'s `create` signal, which asks for the view of a `window.open`
/// or a link to a new window. Whoever `create` gives that view to must hold it: the binding
/// hands WebKit a strong reference where a C handler returns a floating one for the view's
/// owner to sink, and WebKit never releases it, so it is released here once the signal is over.
pub fn connect_create(
    view: &webkit::WebView,
    create: impl Fn(&webkit::WebView, &webkit::NavigationAction) -> Option<webkit::WebView> + 'static,
) -> glib::SignalHandlerId {
    view.connect_create(move |view, action| {
        let created = create(view, action)?;
        let handed = created.clone();
        glib::idle_add_local_once(move || {
            // SAFETY: the reference WebKit got from the binding, which nothing else
            // releases; `handed` keeps the view alive through the call.
            unsafe { glib::gobject_ffi::g_object_unref(handed.as_ptr().cast()) };
        });
        Some(created.upcast())
    })
}

/// Create and start the background context, if the manifest declares one. Fires
/// `runtime.onInstalled` / `runtime.onStartup` once the page has loaded, since that is
/// when its top-level listeners exist.
pub(crate) fn start_background(inner: &Rc<Inner>, ext: &Rc<Extension>, install: InstallEvent) {
    let Some(url) = ext.background_url() else { return };
    let view = build(inner, ext, ViewKind::Background);
    let ext_for_load = ext.clone();
    let fired = std::cell::Cell::new(false);
    view.connect_load_changed(move |view, event| {
        if event != webkit::LoadEvent::Finished || fired.replace(true) {
            return;
        }
        let (event_name, args) = match &install {
            InstallEvent::Installed => ("runtime.onInstalled", vec![json!({ "reason": "install" })]),
            InstallEvent::Updated { previous } => ("runtime.onInstalled", vec![json!({ "reason": "update", "previousVersion": previous })]),
            InstallEvent::Startup => ("runtime.onStartup", vec![]),
            InstallEvent::Nothing => {
                log::debug!("{}: background ready (re-enabled, no lifecycle event)", ext_for_load.id.as_str());
                ext_for_load.background_loaded();
                return;
            }
        };
        bridge::emit(view, None, event_name, &args);
        log::debug!("{}: background ready, fired {event_name}", ext_for_load.id.as_str());
        ext_for_load.background_loaded();
    });
    *ext.background.borrow_mut() = Some(view.clone());
    ext.background_waiting.borrow_mut().get_or_insert_with(Vec::new);
    view.load_uri(&url);
}

impl Inner {
    pub(crate) fn next_view_id(&self) -> ViewId {
        let id = self.next_view.get();
        self.next_view.set(id + 1);
        ViewId(id)
    }
}
