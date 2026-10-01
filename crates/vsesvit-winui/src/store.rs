//! Installs from the Chrome Web Store and Edge Add-ons pages.
//!
//! Both stores install through `chrome.webstorePrivate`, which WebView2 gives only the Chrome Web
//! Store and cannot complete there (it has no install prompt, so the store waits forever).
//! `js/store.js` replaces it in the stores' main world and sends each request as a DOM event. The
//! shortcut script's isolated world forwards the event through its binding together with the host
//! it runs on, which the page cannot fake. The host alone would let a plain-HTTP page that a
//! network attacker serves under a store's name speak for the store, so the shell also requires
//! the tab to show the store over HTTPS. It asks the user, installs or removes through core, and
//! answers the page.

use std::rc::Rc;

use serde_json::{Value, json};
use vsesvit_core::extensions::{ExtensionId, InstallSource, InstalledExtension};

use crate::dialogs;
use crate::tab::TabId;
use crate::window::BrowserWindow;

/// Runs in the main world of every document; it does nothing outside the stores.
pub(crate) const MAIN_WORLD_SCRIPT: &str = include_str!("js/store.js");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store {
    Chrome,
    Edge,
}

impl Store {
    fn from_host(host: &str) -> Option<Self> {
        match host {
            "chromewebstore.google.com" => Some(Self::Chrome),
            "microsoftedge.microsoft.com" => Some(Self::Edge),
            _ => None,
        }
    }

    /// The store a page on `origin` (as `Origin` serializes it) is: only ever an HTTPS one.
    fn from_origin(origin: &str) -> Option<Self> {
        match origin {
            "https://chromewebstore.google.com" => Some(Self::Chrome),
            "https://microsoftedge.microsoft.com" => Some(Self::Edge),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Chrome => "the Chrome Web Store",
            Self::Edge => "Edge Add-ons",
        }
    }

    fn source(self, id: ExtensionId) -> InstallSource {
        match self {
            Self::Chrome => InstallSource::ChromeWebStore { id },
            Self::Edge => InstallSource::EdgeAddons { id },
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoreRequest {
    pub store: Store,
    /// The page's number for the request, echoed in the answer.
    pub seq: u64,
    pub op: StoreOp,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StoreOp {
    List,
    Install { id: ExtensionId, name: String },
    Uninstall { id: ExtensionId },
}

/// A request from `js/store.js`, as `detail` of its event, sent by a document on `host`.
pub(crate) fn parse_request(host: &str, detail: &str) -> Option<StoreRequest> {
    let store = Store::from_host(host)?;
    let value: Value = serde_json::from_str(detail).ok()?;
    let id = || {
        ExtensionId::parse(value.get("id")?.as_str()?)
            .ok()
            .filter(ExtensionId::is_chrome_style)
    };
    let op = match value.get("op")?.as_str()? {
        "list" => StoreOp::List,
        "install" => StoreOp::Install {
            id: id()?,
            name: value.get("name")?.as_str()?.to_owned(),
        },
        "uninstall" => StoreOp::Uninstall { id: id()? },
        _ => return None,
    };
    Some(StoreRequest {
        store,
        seq: value.get("seq")?.as_u64()?,
        op,
    })
}

/// Carries out `request` from tab `tab` and answers the page.
pub(crate) async fn answer(window: Rc<BrowserWindow>, tab: TabId, request: StoreRequest) {
    let origin = window.tab(tab).and_then(|tab| tab.origin());
    if origin.as_ref().and_then(|o| Store::from_origin(o.as_str())) != Some(request.store) {
        log::warn!(
            "ignoring a store request from {}",
            origin.as_ref().map_or("an opaque origin", |o| o.as_str())
        );
        return;
    }
    let mut reply = match carry_out(&window, &request).await {
        Ok(reply) => reply,
        Err(error) => json!({ "ok": false, "error": error }),
    };
    reply["seq"] = request.seq.into();
    let Some(tab) = window.tab(tab) else { return };
    if let Err(e) = tab.eval(&reply_script(&reply)).await {
        log::warn!("answering the store page: {e}");
    }
}

async fn carry_out(window: &Rc<BrowserWindow>, request: &StoreRequest) -> Result<Value, String> {
    let browser = window.browser().ok_or("the browser is closing")?;
    let installed = |id: &ExtensionId| {
        browser
            .core(|p| p.extensions().get(id))
            .map_err(|e| e.to_string())
    };
    match &request.op {
        StoreOp::List => {
            let list = browser
                .core(|p| p.extensions().list())
                .map_err(|e| e.to_string())?;
            Ok(json!({ "extensions": list.iter().map(describe).collect::<Vec<_>>() }))
        }
        StoreOp::Install { id, name } => {
            if let Some(ext) = installed(id)? {
                return Ok(json!({ "ok": true, "installed": describe(&ext) }));
            }
            let name = if name.trim().is_empty() {
                id.as_str()
            } else {
                name.trim()
            };
            let question = format!(
                "Vsesvit will download it from {} and it can then run on the sites its \
                 permissions allow.",
                request.store.name()
            );
            let yes =
                dialogs::confirm_for_page(window, &format!("Add “{name}”?"), &question, "Add")
                    .await
                    .map_err(|e| e.message())?;
            if !yes {
                return Ok(json!({ "ok": false, "cancelled": true, "error": "User cancelled install" }));
            }
            let ext = browser
                .install_extension(request.store.source(id.clone()), &|_| {})
                .await?;
            if let Some(e) = browser.extensions.engine_error(&ext.id) {
                return Err(format!("WebView2 did not load it: {e}"));
            }
            Ok(json!({ "ok": true, "installed": describe(&ext) }))
        }
        StoreOp::Uninstall { id } => {
            let ext = installed(id)?.ok_or("This extension is not installed")?;
            let name = &ext.manifest.name;
            let yes = dialogs::confirm_for_page(window, &format!("Remove “{name}”?"), "", "Remove")
                .await
                .map_err(|e| e.message())?;
            if !yes {
                return Err("User cancelled uninstall".into());
            }
            browser.uninstall_extension(id).await?;
            Ok(json!({ "ok": true }))
        }
    }
}

/// An installed extension as `chrome.management` describes one.
fn describe(ext: &InstalledExtension) -> Value {
    json!({
        "id": ext.id.as_str(),
        "name": ext.manifest.name,
        "version": ext.version,
        "enabled": ext.enabled,
    })
}

/// Hands `reply` to the page's `js/store.js`.
fn reply_script(reply: &Value) -> String {
    let detail = serde_json::to_string(&reply.to_string()).expect("a string serializes");
    format!(r#"dispatchEvent(new CustomEvent("vsesvit-store-reply", {{ detail: {detail} }}));"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "gcllgfdnfnllodcaambdaknbipemelie";

    #[test]
    fn requests_parse_only_on_the_stores() {
        let install = format!(r#"{{"seq":3,"op":"install","store":"chrome","id":"{ID}","name":"P"}}"#);
        assert_eq!(
            parse_request("microsoftedge.microsoft.com", &install),
            Some(StoreRequest {
                store: Store::Edge,
                seq: 3,
                op: StoreOp::Install {
                    id: ExtensionId::parse(ID).unwrap(),
                    name: "P".into()
                }
            }),
            "the store is the sender's host, not what the page claims"
        );
        assert_eq!(parse_request("example.com", &install), None);
        assert_eq!(
            parse_request("chromewebstore.google.com", r#"{"seq":1,"op":"list"}"#),
            Some(StoreRequest {
                store: Store::Chrome,
                seq: 1,
                op: StoreOp::List
            })
        );
    }

    #[test]
    fn only_the_stores_https_pages_are_the_stores() {
        assert_eq!(
            Store::from_origin("https://chromewebstore.google.com"),
            Some(Store::Chrome)
        );
        assert_eq!(
            Store::from_origin("https://microsoftedge.microsoft.com"),
            Some(Store::Edge)
        );
        for origin in [
            "http://microsoftedge.microsoft.com",
            "http://chromewebstore.google.com",
            "https://chromewebstore.google.com:8443",
            "https://example.com",
        ] {
            assert_eq!(Store::from_origin(origin), None, "{origin}");
        }
    }

    #[test]
    fn requests_need_a_chrome_style_id_and_a_known_op() {
        let host = "chromewebstore.google.com";
        assert_eq!(
            parse_request(host, r#"{"seq":1,"op":"uninstall","id":"x@y"}"#),
            None
        );
        assert_eq!(
            parse_request(host, &format!(r#"{{"seq":1,"op":"enable","id":"{ID}"}}"#)),
            None
        );
        assert_eq!(parse_request(host, r#"{"op":"list"}"#), None);
        assert_eq!(parse_request(host, "not json"), None);
    }

    #[test]
    fn the_reply_is_a_json_string_in_the_event() {
        let script = reply_script(&json!({ "seq": 2, "error": "a \"b\" </script>" }));
        assert_eq!(
            script,
            r#"dispatchEvent(new CustomEvent("vsesvit-store-reply", { detail: "{\"error\":\"a \\\"b\\\" </script>\",\"seq\":2}" }));"#
        );
    }
}
