//! Tabs come and go with motion, and the tab model never waits for it: a new tab is in `order`
//! at once and its row slides in, a closed tab leaves `order` at once while its row folds away,
//! a pinned tab's row is whole at its new place, and the collapsing pane passes through the
//! widths in between. With animations off in Windows, every change is instant instead.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vsesvit_core::prefs::TabsPosition;
use windows_core::Interface;

use super::{save, shoot, wait_layout, wait_loaded};
use crate::bindings::*;
use crate::tab::TabId;
use crate::window::{BrowserWindow, TabAction};
use crate::{anim, exec};

const PAGE: &str = "data:text/html,<title>Motion</title><h1>Motion</h1>";
/// Enough for any animation here to have ended.
const SETTLE: Duration = Duration::from_millis(600);
const SAMPLE: Duration = Duration::from_millis(15);
/// Brightness spread that a row with its title drawn has, and a faded-out row lacks.
const DRAWN: u8 = 60;

/// One row of the vertical pane as drawn: its tab (`None` while leaving), its target opacity
/// and the brightness spread of its pixels.
#[derive(Debug)]
struct Drawn {
    tab: Option<TabId>,
    opacity: f64,
    contrast: Option<u8>,
}

/// Captures the window and measures every pane row in the capture; saves it as `name`.
async fn drawn_rows(
    window: &BrowserWindow,
    out_dir: &Path,
    name: &str,
) -> Result<Vec<Drawn>, String> {
    let shot = window.capture().await.map_err(|e| e.to_string())?;
    save(out_dir, name, &shot.png)?;
    let scale = window
        .xaml_root()
        .and_then(|r| r.RasterizationScale())
        .map_err(|e| e.to_string())?;
    Ok(window
        .pane_rows()
        .into_iter()
        .map(|(tab, row)| {
            let element = row.cast::<UIElement>().ok();
            let origin = element
                .as_ref()
                .and_then(|e| e.TransformToVisual(None::<&UIElement>).ok())
                .and_then(|t| t.TransformPoint(Point { x: 0.0, y: 0.0 }).ok());
            let size = (
                row.ActualWidth().unwrap_or(0.0),
                row.ActualHeight().unwrap_or(0.0),
            );
            let contrast = origin.and_then(|o| {
                let px = |v: f64| (v * scale).round().max(0.0) as u32;
                let (x, y) = (f64::from(o.x) + 8.0, f64::from(o.y) + 6.0);
                shot.contrast(px(x), px(y), px(size.0 - 16.0), px(size.1 - 12.0))
            });
            Drawn {
                tab,
                opacity: element.and_then(|e| e.Opacity().ok()).unwrap_or(-1.0),
                contrast,
            }
        })
        .collect())
}

fn all_whole(rows: &[Drawn]) -> bool {
    !rows.is_empty()
        && rows
            .iter()
            .all(|r| r.tab.is_some() && r.opacity == 1.0 && r.contrast.is_some_and(|c| c >= DRAWN))
}

/// Values of `probe` every few milliseconds until `done` holds or `limit` passes.
async fn samples(limit: Duration, probe: impl Fn() -> f64, done: impl Fn(f64) -> bool) -> Vec<f64> {
    let deadline = Instant::now() + limit;
    let mut seen = vec![probe()];
    while !done(*seen.last().unwrap_or(&0.0)) && Instant::now() < deadline {
        exec::sleep(SAMPLE).await;
        seen.push(probe());
    }
    seen
}

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let animated = anim::enabled();
    let before = window.tab_count();
    let tab = window.open_url_tab(PAGE, true).map_err(|e| e.to_string())?;
    let listed_at_once =
        window.tab_count() == before + 1 && window.tabs_in_order().iter().any(|t| t.id == tab.id);
    // Caught on its way in: fainter than at rest while animations are on.
    let coming = drawn_rows(window, out_dir, "42a-tab-row-coming-in").await?;
    let coming = coming
        .iter()
        .find(|r| r.tab == Some(tab.id))
        .and_then(|r| r.contrast);
    wait_loaded(&tab).await?;
    exec::sleep(SETTLE).await;
    let rows = drawn_rows(window, out_dir, "42a-tab-row-comes-in").await?;
    steps.push(json!({
        "name": "42a-tab-row-comes-in",
        "animations": animated,
        "listed_at_once": listed_at_once,
        "contrast_on_its_way_in": coming,
        "rows": format!("{rows:?}"),
        "screenshot": { "file": "42a-tab-row-comes-in.png" },
        "ok": listed_at_once && all_whole(&rows) && rows.iter().any(|r| r.tab == Some(tab.id)),
    }));

    window.close_tab(tab.id);
    let gone_at_once =
        window.tab_count() == before && !window.tabs_in_order().iter().any(|t| t.id == tab.id);
    let leaving = || {
        window
            .pane_rows()
            .into_iter()
            .find(|(tab, _)| tab.is_none())
            .map_or(-1.0, |(_, row)| row.ActualHeight().unwrap_or(-1.0))
    };
    let heights = samples(SETTLE, leaving, |h| h < 0.0).await;
    let folded_through = heights.iter().any(|h| *h > 0.5 && *h < 35.5);
    let rows = drawn_rows(window, out_dir, "42b-tab-row-folds-away").await?;
    steps.push(json!({
        "name": "42b-tab-row-folds-away",
        "animations": animated,
        "gone_at_once": gone_at_once,
        "leaving_row_heights": heights,
        "rows": format!("{rows:?}"),
        "screenshot": { "file": "42b-tab-row-folds-away.png" },
        "ok": gone_at_once && folded_through == animated && heights.last() == Some(&-1.0)
            && all_whole(&rows) && window.pane_rows().len() == window.tab_count(),
    }));

    let pinned = window
        .open_url_tab(PAGE, false)
        .map_err(|e| e.to_string())?;
    wait_loaded(&pinned).await?;
    window.tab_action(pinned.id, TabAction::Pin(true));
    let first_at_once = window.tabs_in_order().first().map(|t| t.id) == Some(pinned.id);
    exec::sleep(SETTLE).await;
    let rows = drawn_rows(window, out_dir, "42c-pinned-row-is-whole").await?;
    steps.push(json!({
        "name": "42c-pinned-row-is-whole",
        "first_at_once": first_at_once,
        "rows": format!("{rows:?}"),
        "screenshot": { "file": "42c-pinned-row-is-whole.png" },
        "ok": first_at_once && all_whole(&rows) && window.pane_rows().len() == window.tab_count(),
    }));
    window.tab_action(pinned.id, TabAction::Pin(false));
    window.close_tab(pinned.id);

    let browser = window.browser().ok_or("no browser")?;
    let width = || window.layout_geometry().0.map_or(-1.0, |pane| pane.width);
    let expanded = width();
    browser.set_tab_pane_collapsed(true);
    let narrowing = samples(SETTLE, width, |w| w <= 48.5).await;
    browser.set_tab_pane_collapsed(false);
    let widening = samples(SETTLE, width, |w| (w - expanded).abs() < 0.5).await;
    let between = |seen: &[f64]| seen.iter().any(|w| *w > 49.0 && *w < expanded - 1.0);
    steps.push(json!({
        "name": "42d-pane-folds-and-unfolds",
        "animations": animated,
        "narrowing": narrowing,
        "widening": widening,
        "ok": narrowing.last().is_some_and(|w| *w <= 48.5)
            && widening.last().is_some_and(|w| (w - expanded).abs() < 0.5)
            && between(&narrowing) == animated && between(&widening) == animated,
    }));

    browser.set_tabs_position(TabsPosition::Top);
    wait_layout(window, TabsPosition::Top).await;
    // The rebuilt strip's tabs come in with its theme transition first.
    exec::sleep(SETTLE * 2).await;
    let before = window.tab_count();
    let top = window.open_url_tab(PAGE, true).map_err(|e| e.to_string())?;
    let listed = window.tab_count() == before + 1;
    // The strip's own theme transition: caught on its way in, then at rest.
    let coming = window.capture().await.map_err(|e| e.to_string())?;
    save(out_dir, "42e-top-tab-coming-in", &coming.png)?;
    wait_loaded(&top).await?;
    shoot(window, out_dir, "42e-top-tab-at-rest", steps, |w| {
        json!({ "listed_at_once": listed, "ok": listed && w.top_tab_widths().len() == w.tab_count() })
    })
    .await;
    window.close_tab(top.id);
    let closed = window.tab_count() == before;
    browser.set_tabs_position(TabsPosition::Left);
    let back = wait_layout(window, TabsPosition::Left).await;
    steps.push(json!({
        "name": "42f-top-tab-closes-at-once",
        "ok": closed && back == Some(TabsPosition::Left),
    }));
    Ok(())
}
