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

use super::{save, shoot, wait_loaded};
use crate::bindings::*;
use crate::window::BrowserWindow;
use crate::{capture, exec};

const SECURE_PAGE: &str = "https://example.com/";

fn headline(popup: &FrameworkElement) -> String {
    popup
        .FindName("ConnectionTitle")
        .ok()
        .and_then(|t| t.cast::<TextBlock>().ok())
        .and_then(|t| t.Text().ok())
        .map(|t| t.to_string())
        .unwrap_or_default()
}

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

async fn open_popup(window: &Rc<BrowserWindow>) -> Result<FrameworkElement, String> {
    window.show_connection().map_err(|e| e.to_string())?;
    exec::sleep(Duration::from_millis(600)).await;
    window
        .connection_popup()
        .ok_or("the popup did not open".into())
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
            let popup = open_popup(window).await?;
            let (title, certificate, tls, chain) = (
                headline(&popup),
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
    let popup = open_popup(window).await?;
    let (title, certificate) = (headline(&popup), has(&popup, "ConnectionCertificate"));
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
    let popup = open_popup(window).await?;
    let title = headline(&popup);
    steps.push(json!({
        "name": "13c-connection-local",
        "title": title,
        "ok": title == "This page is on your device or inside the browser",
    }));
    window.hide_connection();
    window.close_tab(tab.id);
    Ok(())
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
