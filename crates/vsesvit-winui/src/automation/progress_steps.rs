//! The address pill shows a page's load progress while it loads, and not once it has. The page
//! waits on an image from a listener that never answers, until the listener closes.

use std::net::TcpListener;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};

use super::{STEP_TIMEOUT, shoot};
use crate::exec;
use crate::window::BrowserWindow;

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let stall = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = stall.local_addr().map_err(|e| e.to_string())?.port();
    let page = format!(
        "data:text/html,<title>Slow page</title><h1>Slow page</h1>\
         <img src='http://127.0.0.1:{port}/stall.png'>"
    );
    let tab = window.open_url_tab(&page, true).map_err(|e| e.to_string())?;
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(50), || {
        (tab.state().title == "Slow page").then_some(())
    })
    .await
    .ok_or("the slow page never committed")?;
    exec::sleep(Duration::from_millis(1500)).await;
    shoot(window, out_dir, "12f-address-progress", steps, |w| {
        let fraction = w.address_progress_shown();
        json!({
            "loading": tab.state().loading(),
            "fraction": fraction,
            "ok": tab.state().loading() && fraction.is_some_and(|f| f > 0.3 && f < 0.85),
        })
    })
    .await;
    drop(stall);
    let hidden = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(50), || {
        (!tab.state().loading() && window.address_progress_shown().is_none()).then_some(())
    })
    .await;
    steps.push(json!({
        "name": "12g-address-progress-goes-when-loaded",
        "ok": hidden.is_some(),
    }));
    window.close_tab(tab.id);
    Ok(())
}
