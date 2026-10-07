//! `view-source:` pages. WebKitGTK has no view-source of its own, so the shell serves the
//! scheme, the way GNOME Web serves its `ephy-source:`: the page's main resource as received,
//! from a tab showing exactly that page, else loaded by a hidden view with scripts off, shown
//! with numbered lines by core's `source_page`. The hidden view loads as the asking tab would:
//! in its session, with tracking protection and the cookie rules, and over https where
//! HTTPS-only would upgrade the page.
//!
//! The scheme is local, so web pages can neither open nor embed it, and the tab's gate keeps
//! local pages out too (`vsesvit_webext::Gate`); the browser's own loads (the address bar,
//! Ctrl+U) still can.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Once;

use futures_channel::oneshot;
use gtk::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Url;
use vsesvit_core::private::Browsing;
use vsesvit_core::view_source::{source_page, viewed_url};
use webkit::prelude::*;

use crate::browser::Browser;
use crate::tab::Tab;

const SCHEME: &str = "view-source";

/// Serves the scheme for every view. WebKit takes a scheme once per process.
pub(crate) fn register() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        let context = webkit::WebContext::default().expect("WebKit default web context");
        context.register_uri_scheme(SCHEME, |request| {
            let request = request.clone();
            glib::spawn_future_local(async move { serve(&request).await });
        });
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_local(SCHEME);
        }
    });
}

async fn serve(request: &webkit::URISchemeRequest) {
    let uri = request.uri().unwrap_or_default();
    let Some(page) = viewed_url(&uri) else {
        request.finish_error(&mut failed(&format!("{uri} shows no page's source")));
        return;
    };
    match source(request.web_view(), page).await {
        Ok(bytes) => {
            let html = glib::Bytes::from_owned(source_page(page, &String::from_utf8_lossy(&bytes)).into_bytes());
            let length = i64::try_from(html.len()).unwrap_or(-1);
            request.finish(&gio::MemoryInputStream::from_bytes(&html), length, Some("text/html"));
        }
        Err(mut e) => request.finish_error(&mut e),
    }
}

/// The bytes `page` arrived as: from a tab other than the asking one showing it, else loaded
/// again for the asking tab.
async fn source(requester: Option<webkit::WebView>, page: &str) -> Result<Vec<u8>, glib::Error> {
    let requester = requester.ok_or_else(|| failed("no view asked for the source"))?;
    let asking = requester.ancestor(Tab::static_type()).and_downcast::<Tab>().ok_or_else(|| failed("no tab asked for the source"))?;
    let browser = asking.window().ok_or_else(|| failed("the tab has no window"))?.browser().clone();
    if let Some(shown) = shown_elsewhere(&browser, &asking, page) {
        return shown.data_future().await;
    }
    load_hidden(&browser, asking.browsing(), page).await
}

/// The main resource of a tab other than `asking`, and of its kind, that has finished loading
/// `page`: a private tab's page never shows in a normal one, nor the other way round.
fn shown_elsewhere(browser: &Browser, asking: &Tab, page: &str) -> Option<webkit::WebResource> {
    let tabs = browser.windows().into_iter().flat_map(|window| window.tabs());
    tabs.filter(|tab| tab.browsing() == asking.browsing()).find_map(|tab| {
        let view = tab.web_view();
        let resource = view.main_resource()?;
        (tab != *asking && !view.is_loading() && resource.uri().as_deref() == Some(page)).then_some(resource)
    })
}

/// Loads `page` in a view no one sees, with scripts and images off and every response shown
/// rather than downloaded, and takes its main resource. Where HTTPS-only would upgrade the
/// page it loads over https, and it refuses a redirect back to http, since there is no tab to
/// show the warning in.
async fn load_hidden(browser: &Browser, browsing: Browsing, page: &str) -> Result<Vec<u8>, glib::Error> {
    let url = Url::parse(page).map_err(|e| failed(&format!("{page}: {e}")))?;
    let upgraded = browser.https_upgrade(browsing, &url);
    let settings = webkit::Settings::new();
    settings.set_enable_javascript(false);
    settings.set_auto_load_images(false);
    let content = webkit::UserContentManager::new();
    browser.trackers().attach(&content);
    browser.cookies().attach(&content);
    let view = webkit::WebView::builder()
        .network_session(&browser.network_session(browsing))
        .user_content_manager(&content)
        .settings(&settings)
        .build();
    let (sender, loaded) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let send = move |outcome: Result<(), glib::Error>| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(outcome);
        }
    };
    let send_failure = send.clone();
    let insecure = upgraded.is_some().then(|| url.clone());
    view.connect_load_failed(move |_, _, _, error| {
        let error = insecure.as_ref().map_or_else(|| error.clone(), |url| failed(&format!("{url} has no secure connection")));
        send_failure(Err(error));
        true
    });
    // Navigations before the page commits are its main frame's: the load and its redirects.
    let committed = Rc::new(Cell::new(false));
    let send_refusal = send.clone();
    view.connect_load_changed({
        let committed = committed.clone();
        move |_, event| match event {
            webkit::LoadEvent::Committed => committed.set(true),
            webkit::LoadEvent::Finished => send(Ok(())),
            _ => {}
        }
    });
    let asking = browser.clone();
    view.connect_decide_policy(move |_, decision, kind| match kind {
        webkit::PolicyDecisionType::NavigationAction if !committed.get() => {
            let Some(nav) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else { return false };
            let target = nav.navigation_action().and_then(|a| a.request()).and_then(|r| r.uri()).and_then(|uri| Url::parse(&uri).ok());
            let Some(http) = target.filter(|url| asking.https_upgrade(browsing, url).is_some()) else { return false };
            decision.ignore();
            send_refusal(Err(failed(&format!("{http} has no secure connection"))));
            true
        }
        webkit::PolicyDecisionType::Response => {
            decision.use_();
            true
        }
        _ => false,
    });
    view.load_uri(upgraded.as_ref().map_or(page, Url::as_str));
    loaded.await.unwrap_or_else(|_| Err(failed("the load went away")))?;
    let resource = view.main_resource().ok_or_else(|| failed("the page has no main resource"))?;
    resource.data_future().await
}

fn failed(message: &str) -> glib::Error {
    glib::Error::new(webkit::NetworkError::Failed, message)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::test_support::{Reply, Server, browser, scratch_dir, settle, wait_until};
    use crate::window::{BrowserWindow, Focus};
    use vsesvit_core::https_only::Reach;
    use vsesvit_core::permissions::{Origin, Permission, Setting};
    use vsesvit_core::prefs::keys;

    /// A page that embeds and then goes to the source of `page`, a script expression.
    fn lure_to(page: &str) -> String {
        format!(
            "<!doctype html><title>Lure</title><script>\
             const frame = document.createElement('iframe');\
             frame.src = 'view-source:' + {page};\
             document.documentElement.append(frame);\
             setTimeout(() => location.href = 'view-source:' + {page}, 300);\
             </script>"
        )
    }

    #[gtk::test]
    fn the_browser_shows_sources_that_pages_cannot_reach() {
        let fetched = Arc::new(AtomicUsize::new(0));
        let server = Server::start("127.0.0.1", {
            let fetched = fetched.clone();
            move |path| match path {
                "/page" => {
                    fetched.fetch_add(1, Ordering::SeqCst);
                    Reply::Page("Page")
                }
                "/lure" => Reply::Body("text/html", lure_to("location.origin + '/page'").into()),
                "/file.bin" => Reply::Body("application/octet-stream", b"<raw>".to_vec()),
                "/drop" => Reply::Drop,
                _ => Reply::NotFound,
            }
        });
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let shows = |tab: &Tab, page: &str| tab.web_view().title().as_deref() == Some(format!("view-source:{page}").as_str());

        let lure = server.url("/lure");
        let tab = window.open_tab(Some(&lure), None, Focus::Foreground);
        wait_until("the lure", || tab.committed_uri().as_deref() == Some(lure.as_str()));
        settle(Duration::from_millis(1500));
        let lured = (tab.committed_uri(), fetched.load(Ordering::SeqCst));

        let page = server.url("/page");
        // A local page may load the local scheme, but only the browser opens a source.
        let local = scratch_dir("view-source").join("lure.html");
        std::fs::write(&local, lure_to(&format!("'{page}'"))).expect("the local lure written");
        let local = gio::File::for_path(&local).uri().to_string();
        tab.load(&local);
        wait_until("the local lure", || tab.committed_uri().as_deref() == Some(local.as_str()));
        settle(Duration::from_millis(1500));
        let lured_locally = (tab.committed_uri(), fetched.load(Ordering::SeqCst));

        tab.load(&format!("view-source:{page}"));
        wait_until("the typed source", || shows(&tab, &page));
        let typed = fetched.load(Ordering::SeqCst);

        let downloads = browser.downloads().list(Browsing::Normal).len();
        let file = server.url("/file.bin");
        tab.load(&format!("view-source:{file}"));
        wait_until("the file's source", || shows(&tab, &file));
        settle(Duration::from_millis(500));
        let downloaded = browser.downloads().list(Browsing::Normal).len() - downloads;

        tab.load(&format!("view-source:{}", server.url("/drop")));
        wait_until("the error page", || tab.shows_error_page());

        window.destroy();
        assert_eq!(lured, (Some(lure), 0), "the page neither went to nor embedded its source");
        assert_eq!(lured_locally, (Some(local), 0), "nor did a local page");
        assert_eq!(typed, 1, "the address bar's view-source loaded the page once");
        assert_eq!(downloaded, 0, "a file's source is shown, not downloaded");
    }

    #[gtk::test]
    fn a_source_loaded_again_keeps_to_https_only() {
        let fetched = Arc::new(AtomicUsize::new(0));
        let server = Server::start("127.0.0.1", {
            let fetched = fetched.clone();
            move |path| match path {
                "/page" => {
                    fetched.fetch_add(1, Ordering::SeqCst);
                    Reply::Page("Page")
                }
                _ => Reply::NotFound,
            }
        });
        let browser = browser();
        browser.set_pref(&keys::HTTPS_ONLY, &true);
        browser.set_https_reach(Reach::Everywhere);
        let window = BrowserWindow::new(&browser);
        window.present();
        let page = server.url("/page");
        let source = format!("view-source:{page}");
        let tab = window.open_tab(Some(&source), None, Focus::Foreground);
        wait_until("the error page", || tab.shows_error_page());
        let upgraded = fetched.load(Ordering::SeqCst);

        let origin = Origin::of(&Url::parse(&page).expect("the page's URL")).expect("an http origin");
        let exception = |setting| browser.core().borrow_mut().site_permissions_in(Browsing::Normal).set(&origin, Permission::Http, setting);
        exception(Some(Setting::Allow)).expect("the exception stored");
        tab.load(&source);
        wait_until("the source", || tab.web_view().title().as_deref() == Some(source.as_str()));
        let excepted = fetched.load(Ordering::SeqCst);

        exception(None).expect("the exception removed");
        browser.set_https_reach(Reach::Public);
        browser.reset_pref(&keys::HTTPS_ONLY);
        window.destroy();
        assert_eq!(upgraded, 0, "with HTTPS-only on, the page was not fetched over http");
        assert_eq!(excepted, 1, "with an exception for the site it was");
    }
}
