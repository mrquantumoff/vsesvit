//! The security icon's popup on an HTTPS page, a plain HTTP page and a local page. The HTTPS
//! page is a real site, so without a network that step is skipped. The Windows certificate
//! viewer is a modal dialog that takes the focus, so it is only opened and captured when
//! `VSESVIT_SMOKE_CERT_VIEWER=1`.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::permission_steps::{open_site_info, text_of};
use super::{save, shoot, wait_loaded};
use crate::bindings::*;
use crate::tab::Tab;
use crate::window::BrowserWindow;
use crate::{capture, exec};

const SECURE_PAGE: &str = "https://example.com/";

fn has(popup: &FrameworkElement, name: &str) -> bool {
    popup.FindName(name).is_ok()
}

fn chain_length(popup: &FrameworkElement) -> u32 {
    popup
        .FindName("ConnectionChain")
        .ok()
        .and_then(|c| c.cast::<Panel>().ok())
        .and_then(|c| c.Children().ok())
        .and_then(|c| c.Size().ok())
        .unwrap_or(0)
}

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let tab = window
        .open_url_tab(SECURE_PAGE, true)
        .map_err(|e| e.to_string())?;
    match wait_loaded(&tab).await {
        Ok(()) => {
            let popup = open_site_info(window).await?;
            let (title, certificate, tls, chain) = (
                text_of(&popup, "ConnectionTitle"),
                has(&popup, "ConnectionCertificate"),
                has(&popup, "ConnectionTls"),
                chain_length(&popup),
            );
            shoot(window, out_dir, "13a-connection-secure", steps, |_| {
                json!({
                    "title": title,
                    "certificate": certificate,
                    "tls": tls,
                    "chain": chain,
                    "ok": title == "Connection is secure" && certificate && tls && chain >= 2,
                })
            })
            .await;
            if std::env::var("VSESVIT_SMOKE_CERT_VIEWER").as_deref() == Ok("1") {
                steps.push(certificate_viewer(&popup, out_dir).await);
            }
            window.hide_connection();
            steps.push(secure_after_navigating(window, &tab).await?);
        }
        Err(e) => steps.push(json!({
            "name": "13a-connection-secure",
            "skipped": format!("{SECURE_PAGE} did not load, probably no network: {e}"),
            "ok": true,
        })),
    }

    tab.navigate(server.url("/page2.html").as_str());
    exec::sleep(Duration::from_millis(300)).await;
    wait_loaded(&tab).await?;
    let popup = open_site_info(window).await?;
    let (title, certificate) = (
        text_of(&popup, "ConnectionTitle"),
        has(&popup, "ConnectionCertificate"),
    );
    shoot(window, out_dir, "13b-connection-not-secure", steps, |_| {
        json!({
            "title": title,
            "certificate": certificate,
            "ok": title == "Connection is not secure" && !certificate,
        })
    })
    .await;
    window.hide_connection();

    tab.navigate("data:text/html,<title>Local</title>local");
    exec::sleep(Duration::from_millis(300)).await;
    wait_loaded(&tab).await?;
    let popup = open_site_info(window).await?;
    let title = text_of(&popup, "ConnectionTitle");
    steps.push(json!({
        "name": "13c-connection-local",
        "title": title,
        "ok": title == "This page is on your device or inside the browser",
    }));
    window.hide_connection();
    window.close_tab(tab.id);
    Ok(())
}

/// The report set aside when a navigation starts is replaced by the next page's, so a secure
/// page reached from a secure page reads as secure again.
async fn secure_after_navigating(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
) -> Result<Value, String> {
    let page = format!("{SECURE_PAGE}?again");
    tab.navigate(&page);
    exec::sleep(Duration::from_millis(300)).await;
    wait_loaded(tab).await?;
    let popup = open_popup(window).await?;
    let (title, certificate) = (headline(&popup), has(&popup, "ConnectionCertificate"));
    window.hide_connection();
    Ok(json!({
        "name": "13a2-connection-secure-after-navigating",
        "url": tab.state().url,
        "title": title,
        "certificate": certificate,
        "ok": tab.state().url == page && title == "Connection is secure" && certificate,
    }))
}

/// "Show certificate" opens the Windows certificate viewer with the site's certificate.
async fn certificate_viewer(popup: &FrameworkElement, out_dir: &Path) -> Value {
    let invoked = popup
        .FindName("ShowCertificate")
        .and_then(|b| b.cast::<Button>())
        .and_then(|b| super::dialog_steps::invoke(&b));
    let dialog = exec::wait_for(Duration::from_secs(10), Duration::from_millis(100), || {
        capture::find_window("Certificate")
    })
    .await;
    exec::sleep(Duration::from_millis(800)).await;
    let shot = match dialog {
        Some(hwnd) => {
            let png = capture::single_window_png(hwnd).await;
            unsafe {
                let _ = PostMessageW(hwnd, WM_CLOSE as u32, 0, 0);
            }
            png.map_err(|e| e.to_string())
                .and_then(|png| save(out_dir, "13d-certificate-viewer", &png))
        }
        None => Err("no Certificate window".into()),
    };
    json!({
        "name": "13d-certificate-viewer",
        "invoked": invoked.is_ok(),
        "screenshot": format!("{shot:?}"),
        "ok": invoked.is_ok() && shot.is_ok(),
    })
}
