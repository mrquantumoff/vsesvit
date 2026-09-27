//! WebViews for extension pages: the hidden background view and action popups. Each
//! gets its own `UserContentManager` with the page shim and a message handler in the
//! default world, the manifest's CSP, CORS for host permissions, and navigation pinned
//! to the extension origin (anything else opens as a tab).

use std::rc::Rc;

use serde_json::json;
use webkit::prelude::*;

use crate::bridge::{self, Origin};
use crate::extension::{ExtView, Extension, ViewId, ViewKind};
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
    if !allowlist.is_empty() {
        view.set_cors_allowlist(&allowlist);
    }
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(std::env::var_os("VSESVIT_WEBEXT_CONSOLE").is_some());
    }

    let weak_inner = Rc::downgrade(inner);
    let base = ext.base_url.clone();
    view.connect_decide_policy(move |_, decision, decision_type| {
        if !matches!(decision_type, webkit::PolicyDecisionType::NavigationAction | webkit::PolicyDecisionType::NewWindowAction) {
            return false;
        }
        let Some(nav) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else { return false };
        let uri = nav.navigation_action().and_then(|a| a.request()).and_then(|r| r.uri()).map(String::from).unwrap_or_default();
        let inside = uri.starts_with(&base) || uri == base.trim_end_matches('/') || uri.starts_with("about:") || uri.starts_with("blob:") || uri.starts_with("data:");
        if inside && decision_type == webkit::PolicyDecisionType::NavigationAction {
            return false;
        }
        decision.ignore();
        if (uri.starts_with("http://") || uri.starts_with("https://") || inside)
            && let Some(inner) = weak_inner.upgrade()
        {
            inner.host.create_tab(&uri, true);
        }
        true
    });

    let ext_id = ext.id.as_str().to_owned();
    view.connect_web_process_terminated(move |_, reason| log::warn!("{ext_id}: extension page web process terminated: {reason:?}"));

    ext.views.borrow_mut().push(ExtView { id: view_id, kind, view: view.downgrade() });
    view
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
        let (event_name, detail) = match &install {
            InstallEvent::Installed => ("runtime.onInstalled", json!({ "reason": "install" })),
            InstallEvent::Updated { previous } => ("runtime.onInstalled", json!({ "reason": "update", "previousVersion": previous })),
            InstallEvent::Startup => ("runtime.onStartup", json!({})),
        };
        let args = if event_name == "runtime.onStartup" { vec![] } else { vec![detail] };
        bridge::emit(view, None, event_name, &args);
        log::debug!("{}: background ready, fired {event_name}", ext_for_load.id.as_str());
    });
    *ext.background.borrow_mut() = Some(view.clone());
    view.load_uri(&url);
}

#[derive(Clone, Debug)]
pub(crate) enum InstallEvent {
    Installed,
    Updated { previous: String },
    Startup,
}

impl Inner {
    pub(crate) fn next_view_id(&self) -> ViewId {
        let id = self.next_view.get();
        self.next_view.set(id + 1);
        ViewId(id)
    }
}
