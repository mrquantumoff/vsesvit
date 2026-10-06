//! The tab list's search button, atop the vertical pane and after the tabs on top, opens tab
//! search under it, listing the window's open tabs.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::prefs::TabsPosition;

use super::{STEP_TIMEOUT, invoke, shoot, wait_layout};
use crate::exec;
use crate::window::BrowserWindow;

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let browser = window.browser().ok_or("no browser")?;
    for (position, name) in [
        (TabsPosition::Left, "44a-tab-search-from-the-pane"),
        (TabsPosition::Top, "44b-tab-search-from-the-strip"),
    ] {
        browser.set_tabs_position(position);
        wait_layout(window, position).await;
        let button = window.tab_search_button().ok_or("no tab search button")?;
        invoke(&button).map_err(|e| e.to_string())?;
        let search = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
            window.tab_search()
        })
        .await
        .ok_or_else(|| format!("{name}: the button opened no tab search"))?;
        exec::sleep(Duration::from_millis(500)).await;
        let titles: Vec<String> = window
            .tabs_in_order()
            .iter()
            .map(|t| t.state().title)
            .collect();
        shoot(window, out_dir, name, steps, |_| {
            let lines = search.lines();
            let listed = titles
                .iter()
                .all(|title| lines.iter().any(|line| line.contains(title.as_str())));
            json!({
                "lines": lines,
                "tabs": titles,
                "ok": lines.first().is_some_and(|l| l == "Open tabs") && listed,
            })
        })
        .await;
        search.close();
        exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
            window.tab_search().is_none().then_some(())
        })
        .await
        .ok_or_else(|| format!("{name}: tab search did not close"))?;
    }
    browser.set_tabs_position(TabsPosition::Left);
    wait_layout(window, TabsPosition::Left).await;
    Ok(())
}
