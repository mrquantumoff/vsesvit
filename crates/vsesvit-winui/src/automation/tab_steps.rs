//! A tab that plays sound shows a speaker, and its context menu pins it, splits the view with
//! it and copies its link; Ctrl+Shift+C and Ctrl+Alt+Shift+C copy the clean and the whole link.
//! The sidebar player follows the tab, drives its page, and once its site allows
//! picture-in-picture (`pip_steps`) shows its video in the pane while another tab is selected,
//! until the tabs grow into that space.
//! The media page (the fixture site's `/media.html`) plays a generated video with a tone, muted
//! before it starts, so the run makes no sound.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::prefs::TabsPosition;
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::{
    STEP_TIMEOUT, devtools, eval, press, shoot, wait_for_tab_count, wait_layout, wait_loaded,
    wait_title,
};
use crate::bindings::*;
use crate::media::MediaAction;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::tab_header::Audio;
use crate::window::{BrowserWindow, TabAction};
use crate::{exec, xaml};

/// Opens `page` in a background tab, muted, and calls its `start()`.
async fn open_playing(window: &Rc<BrowserWindow>, page: &str) -> Result<Rc<Tab>, String> {
    let media = window
        .open_url_tab(page, false)
        .map_err(|e| e.to_string())?;
    wait_loaded(&media).await?;
    media.set_muted(true);
    let play = json!({ "expression": "start()", "userGesture": true, "awaitPromise": true });
    devtools(&media, "Runtime.evaluate", &play).await?;
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        media.state().audible.then_some(())
    })
    .await
    .ok_or("the media page never became audible")?;
    Ok(media)
}

pub(super) fn select(window: &BrowserWindow, tab: &Tab) {
    if let Some(index) = window.tabs_in_order().iter().position(|t| t.id == tab.id) {
        window.run(Command::SelectTab(u8::try_from(index).unwrap_or(u8::MAX)));
    }
}

fn order(window: &BrowserWindow) -> Vec<u64> {
    window.tabs_in_order().iter().map(|t| t.id).collect()
}

fn width_if_shown(tab: &Tab) -> Option<f64> {
    xaml::is_visible(tab.view())
        .then(|| tab.view().cast::<FrameworkElement>().ok()?.ActualWidth().ok())
        .flatten()
}

/// The menu's labels, a submenu's as `label > child, child`.
fn menu_labels(window: &BrowserWindow, tab: &Tab) -> Result<Vec<String>, String> {
    let menu = MenuFlyout::new().map_err(|e| e.to_string())?;
    window.fill_tab_menu(tab.id, &menu);
    let items = menu.Items().map_err(|e| e.to_string())?;
    let label = |item: &MenuFlyoutItemBase| -> Option<String> {
        if let Ok(item) = item.cast::<MenuFlyoutItem>() {
            return item.Text().ok().map(|t| t.to_string());
        }
        let submenu = item.cast::<MenuFlyoutSubItem>().ok()?;
        let children = submenu.Items().ok()?;
        let children: Vec<String> = (0..children.Size().unwrap_or(0))
            .filter_map(|i| children.GetAt(i).ok()?.cast::<MenuFlyoutItem>().ok()?.Text().ok())
            .map(|t| t.to_string())
            .collect();
        Some(format!("{} > {}", submenu.Text().ok()?, children.join(", ")))
    };
    Ok((0..items.Size().unwrap_or(0))
        .filter_map(|i| items.GetAt(i).ok())
        .filter_map(|item| label(&item))
        .collect())
}

async fn clipboard_text() -> String {
    async {
        let text = Clipboard::GetContent()?.GetTextAsync()?.await?;
        Ok::<_, windows_core::Error>(text.to_string_lossy())
    }
    .await
    .unwrap_or_default()
}

/// Ctrl+Tab and Ctrl+Shift+Tab step through the tabs and wrap around at the ends, and Ctrl+9
/// selects the last tab, in the strip on top and in the pane.
async fn cycle_steps(window: &Rc<BrowserWindow>, steps: &mut Vec<Value>) -> Result<(), String> {
    let browser = window.browser().ok_or("no browser")?;
    let mut opened = Vec::new();
    while window.tab_count() < 3 {
        opened.push(window.open_url_tab("about:blank", false).map_err(|e| e.to_string())?);
    }
    for (position, name) in [
        (TabsPosition::Top, "29-next-and-previous-tab-on-top"),
        (TabsPosition::Left, "29b-next-and-previous-tab-in-the-pane"),
    ] {
        browser.set_tabs_position(position);
        wait_layout(window, position).await;
        let tabs = order(window);
        let last = tabs.len() - 1;
        let mut seen = Vec::new();
        for command in [
            Command::SelectTab(u8::try_from(last).unwrap_or(u8::MAX)),
            Command::NextTab,
            Command::PreviousTab,
            Command::PreviousTab,
            Command::SelectLastTab,
        ] {
            window.run(command);
            seen.push(window.active_tab().map(|t| t.id));
        }
        let want = [tabs[last], tabs[0], tabs[last], tabs[last - 1], tabs[last]].map(Some);
        steps.push(json!({
            "name": name,
            "order": tabs,
            "selected": seen,
            "ok": seen == want,
        }));
    }
    for tab in opened {
        window.close_tab(tab.id);
    }
    Ok(())
}

/// Ctrl+T, which WebView2 never hands to the shell, works with the focus in a frame: one the
/// page's own process runs, and one from another site, which runs in a process of its own.
async fn frame_shortcut_steps(
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let other_site = format!("http://localhost:{}/page2.html", server.port());
    let input = "<body style='margin:0'><input id=i style='width:100%;height:290px'>";
    for (name, frame) in [
        ("36b-ctrl-t-in-a-frame", format!("f.srcdoc = \"{input}\"")),
        ("36c-ctrl-t-in-another-sites-frame", format!("f.src = '{other_site}'")),
    ] {
        let tab = window
            .open_url_tab(server.url("/page2.html").as_str(), true)
            .map_err(|e| e.to_string())?;
        wait_loaded(&tab).await?;
        let add = format!(
            "const f = document.createElement('iframe'); \
             f.style.cssText = 'width:400px;height:300px;border:0'; \
             f.onload = () => {{ document.title = 'Framed' }}; {frame}; document.body.append(f); 0"
        );
        eval(&tab, &add).await?;
        wait_title(&tab, "Framed").await?;
        // A click near the frame's bottom focuses it (and the input in the first one).
        let point = eval(
            &tab,
            "(() => { const r = document.querySelector('iframe').getBoundingClientRect(); \
             return [r.x + r.width / 2, r.y + r.height - 20]; })()",
        )
        .await?;
        let [x, y] = serde_json::from_str::<[f64; 2]>(&point).map_err(|e| e.to_string())?;
        for kind in ["mousePressed", "mouseReleased"] {
            let params = json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 });
            devtools(&tab, "Input.dispatchMouseEvent", &params).await?;
        }
        let focused = eval(&tab, "document.activeElement === document.querySelector('iframe')")
            .await
            .unwrap_or_default()
            == "true";
        let before = window.tab_count();
        press(&tab, 0x54, 2).await?;
        let opened = wait_for_tab_count(window, before + 1).await.ok();
        if let Some(new) = &opened {
            window.close_tab(new.id);
        }
        window.close_tab(tab.id);
        steps.push(json!({
            "name": name,
            "frame_focused": focused,
            "ctrl_t_opened_a_tab": opened.is_some(),
            "ok": focused && opened.is_some(),
        }));
    }
    Ok(())
}

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let first = window.active_tab().ok_or("no tab")?;
    cycle_steps(window, steps).await?;
    frame_shortcut_steps(window, server, steps).await?;
    select(window, &first);
    let media = open_playing(window, server.url("/media.html").as_str()).await?;
    exec::sleep(Duration::from_millis(300)).await;
    shoot(window, out_dir, "30-tab-plays-sound", steps, |_| {
        let state = media.state();
        json!({
            "audible": state.audible,
            "muted": state.muted,
            "ok": state.audible && state.muted && media.look().audio == Audio::Muted,
        })
    })
    .await;

    let labels = menu_labels(window, &media)?;
    let others: Vec<String> = window
        .tabs_in_order()
        .iter()
        .filter(|t| t.id != media.id)
        .map(|t| t.state().title)
        .collect();
    let split_with = format!("Split view with > New tab, {}", others.join(", "));
    steps.push(json!({
        "name": "31-tab-menu",
        "labels": labels,
        "ok": labels == [split_with.as_str(), "Pin tab", "Unmute tab", "Copy link", "Close tab"],
    }));

    window.tab_action(media.id, TabAction::Pin(true));
    exec::sleep(Duration::from_millis(300)).await;
    shoot(window, out_dir, "32-pinned-tab", steps, |w| {
        let order = order(w);
        json!({
            "order": order,
            "ok": order.first() == Some(&media.id) && media.is_pinned() && media.look().pinned,
        })
    })
    .await;
    let browser = window.browser().ok_or("no browser")?;
    browser.set_tabs_position(TabsPosition::Top);
    wait_layout(window, TabsPosition::Top).await;
    exec::sleep(Duration::from_millis(300)).await;
    shoot(window, out_dir, "32b-pinned-tab-on-top", steps, |w| {
        let widths = w.top_tab_widths();
        let icon_only = widths
            .iter()
            .all(|(id, width)| if *id == media.id { *width < 60.0 } else { *width >= 100.0 });
        json!({
            "widths": format!("{widths:?}"),
            "ok": icon_only && widths.first().is_some_and(|(id, _)| *id == media.id),
        })
    })
    .await;
    window.tab_action(media.id, TabAction::Pin(false));
    exec::sleep(Duration::from_millis(300)).await;
    let widths = window.top_tab_widths();
    browser.set_tabs_position(TabsPosition::Left);
    wait_layout(window, TabsPosition::Left).await;
    let equal = widths.windows(2).all(|pair| (pair[0].1 - pair[1].1).abs() < 1.0);
    steps.push(json!({
        "name": "32c-unpinned-tab-on-top",
        "widths": format!("{widths:?}"),
        "ok": equal && widths.first().is_some_and(|(_, width)| *width >= 100.0),
    }));
    let unpinned = order(window);
    steps.push(json!({
        "name": "33-unpinned-tab",
        "order": unpinned,
        "ok": !media.is_pinned() && unpinned.first() == Some(&media.id),
    }));

    // Pinning the selected tab moves its row in the strip; the selection stays on it, and no
    // other tab is selected on the way.
    browser.set_tabs_position(TabsPosition::Top);
    wait_layout(window, TabsPosition::Top).await;
    let last_active = |w: &BrowserWindow| {
        let mut others: Vec<(u64, i64)> = w
            .tabs_in_order()
            .iter()
            .filter(|t| t.id != first.id)
            .map(|t| (t.id, t.last_active_ms()))
            .collect();
        others.sort_unstable();
        others
    };
    let selected_before = window.active_tab().is_some_and(|t| t.id == first.id);
    let before = last_active(window);
    window.tab_action(first.id, TabAction::Pin(true));
    exec::sleep(Duration::from_millis(300)).await;
    let after = last_active(window);
    let pinned_first = order(window).first() == Some(&first.id);
    let selected_after = window.active_tab().is_some_and(|t| t.id == first.id);
    window.tab_action(first.id, TabAction::Pin(false));
    browser.set_tabs_position(TabsPosition::Left);
    wait_layout(window, TabsPosition::Left).await;
    steps.push(json!({
        "name": "33b-pinning-the-selected-tab-keeps-it-selected",
        "last_active_before": before,
        "last_active_after": after,
        "ok": selected_before && pinned_first && selected_after && before == after,
    }));

    window.tab_action(first.id, TabAction::SplitWith(media.id));
    exec::sleep(Duration::from_millis(500)).await;
    let halves = (width_if_shown(&first), width_if_shown(&media));
    shoot(window, out_dir, "34-split-view", steps, |w| {
        let (left, right) = halves;
        let halves = left.zip(right).is_some_and(|(l, r)| l > 100.0 && (l - r).abs() < 2.0);
        json!({
            "widths": format!("{:?}", (left, right)),
            "active": w.active_tab().map(|t| t.id),
            "ok": halves && w.active_tab().is_some_and(|t| t.id == first.id),
        })
    })
    .await;
    let others: Vec<Rc<Tab>> = window
        .tabs_in_order()
        .into_iter()
        .filter(|t| t.id != first.id && t.id != media.id)
        .collect();
    let alone = match others.first() {
        Some(other) => {
            select(window, other);
            // The playing tab of the pair may show in the pane's picture in picture instead.
            let shown = width_if_shown(other).is_some()
                && width_if_shown(&first).is_none()
                && width_if_shown(&media).is_none_or(|w| w < 300.0);
            select(window, &media);
            shown
        }
        None => false,
    };
    let back = width_if_shown(&first).is_some() && width_if_shown(&media).is_some();
    window.tab_action(media.id, TabAction::CloseSplit);
    let closed = width_if_shown(&first).is_none() && width_if_shown(&media).is_some();
    steps.push(json!({
        "name": "35-split-view-follows-the-selection",
        "other_tab_alone": alone,
        "split_returns": back,
        "split_closes": closed,
        "ok": alone && back && closed,
    }));

    // The divider shares the width; let go with the left page squeezed out, the right one
    // stays alone.
    window.tab_action(first.id, TabAction::SplitWith(media.id));
    window.split_dragged(0.3);
    window.split_drag_ended();
    exec::sleep(Duration::from_millis(300)).await;
    let shared = width_if_shown(&first)
        .zip(width_if_shown(&media))
        .is_some_and(|(left, right)| left < right * 0.6);
    window.split_dragged(0.08);
    window.split_drag_ended();
    exec::sleep(Duration::from_millis(300)).await;
    let squeezed_out = width_if_shown(&first).is_none()
        && width_if_shown(&media).is_some_and(|w| w > 300.0)
        && window.active_tab().is_some_and(|t| t.id == media.id);
    steps.push(json!({
        "name": "35b-divider-shares-and-squeezes-out",
        "shared": shared,
        "squeezed_out": squeezed_out,
        "ok": shared && squeezed_out,
    }));

    let tracked = server.url("/page2.html?a=1&utm_source=smoke&fbclid=x");
    first.navigate(tracked.as_str());
    select(window, &first);
    wait_loaded(&first).await?;
    let raw = first.state().url;
    let clean = server.url("/page2.html?a=1").to_string();
    window.run(Command::CopyCleanLink);
    exec::sleep(Duration::from_millis(300)).await;
    let button = clipboard_text().await;
    press(&first, 0x43, 2 | 8).await?;
    exec::sleep(Duration::from_millis(300)).await;
    let shortcut = clipboard_text().await;
    press(&first, 0x43, 1 | 2 | 8).await?;
    exec::sleep(Duration::from_millis(300)).await;
    let whole = clipboard_text().await;
    steps.push(json!({
        "name": "36-copy-link",
        "copied": [&button, &shortcut, &whole],
        "ok": button == clean && shortcut == clean && whole == raw && raw != clean,
    }));

    player_steps(window, &first, &media, out_dir, steps).await?;
    window.close_tab(media.id);
    select(window, &first);
    Ok(())
}

/// A page that plays sound alone shows its artwork in the box instead of the page.
async fn audio_steps(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    const ARTWORK: &str = "https://vsesvit.test/cover.png";
    let page = format!(
        "data:text/html,<title>Audio</title><audio id=a></audio><script>\
         navigator.mediaSession.metadata=new MediaMetadata({{title:'Smoke song',\
         artwork:[{{src:'{ARTWORK}',sizes:'512x512'}}]}});\
         window.start=async()=>{{const c=new AudioContext();const o=c.createOscillator();\
         const n=c.createGain();n.gain.value=0.01;const d=c.createMediaStreamDestination();\
         o.connect(n).connect(d);o.start();const a=document.getElementById('a');\
         a.srcObject=d.stream;await a.play();return 'playing'}};</script>"
    );
    let audio = open_playing(window, &page).await?;
    select(window, first);
    let artwork = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.pip_artwork().filter(|url| url == ARTWORK)
    })
    .await;
    shoot(window, out_dir, "37b-sound-alone-shows-artwork", steps, |w| {
        json!({
            "followed": w.media_tab(),
            "artwork": w.pip_artwork(),
            "page_in_box": w.pip_tab(),
            "ok": w.media_tab() == Some(audio.id) && artwork.is_some() && w.pip_tab().is_none()
                && !xaml::is_visible(audio.view()),
        })
    })
    .await;
    window.close_tab(audio.id);
    let back = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.pip_tab() == Some(media.id)).then_some(())
    })
    .await
    .is_some();
    steps.push(json!({
        "name": "37c-the-still-playing-tab-takes-over",
        "followed": window.media_tab(),
        "ok": back && window.media_tab() == Some(media.id),
    }));
    Ok(())
}

async fn player_steps(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    super::pip_steps::opt_in(window, first, media, out_dir, steps).await?;
    select(window, first);
    let in_pip = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.pip_tab() == Some(media.id)).then_some(())
    })
    .await
    .is_some();
    // Past the page's own resize handling, which moves the player.
    exec::sleep(Duration::from_millis(1200)).await;
    let video = media
        .eval(
            "(() => { const v = document.getElementById('v'); const r = v.getBoundingClientRect(); \
             const moved = document.getElementById('narrow').contains(v) ? 1 : 0; \
             return [r.x, r.y, r.width, r.height, innerWidth, innerHeight, moved]; })()",
        )
        .await
        .ok()
        .and_then(|json| serde_json::from_str::<[f64; 7]>(&json).ok());
    // Only the video shows: it covers the whole (small) page, also after the page moved it.
    let video_only = video.is_some_and(|[x, y, w, h, vw, vh, moved]| {
        x.abs() < 1.0
            && y.abs() < 1.0
            && (w - vw).abs() < 1.0
            && (h - vh).abs() < 1.0
            && moved == 1.0
    });
    shoot(window, out_dir, "37-sidebar-player-pip", steps, |w| {
        json!({
            "followed": w.media_tab(),
            "pip": w.pip_tab(),
            "player": w.player_shown(),
            "video_rect_and_viewport": video,
            "ok": in_pip && w.media_tab() == Some(media.id) && w.player_shown()
                && xaml::is_visible(media.view()) && video_only,
        })
    })
    .await;

    window.player_action(MediaAction::Next);
    let next = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (media.state().title == "Next track").then_some(())
    })
    .await
    .is_some();
    window.player_action(MediaAction::PlayPause);
    let paused = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(200), || {
        (!media.state().audible).then_some(())
    })
    .await
    .is_some();
    window.player_action(MediaAction::PlayPause);
    steps.push(json!({
        "name": "38-player-buttons-drive-the-page",
        "next_track": next,
        "paused": paused,
        "ok": next && paused,
    }));

    let mut filler = Vec::new();
    for _ in 0..16 {
        filler.push(window.open_url_tab("about:blank", false).map_err(|e| e.to_string())?);
    }
    let hidden = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        window.pip_tab().is_none().then_some(())
    })
    .await
    .is_some();
    exec::sleep(Duration::from_millis(500)).await;
    shoot(window, out_dir, "39-tabs-reach-the-pip", steps, |w| {
        json!({
            "pip": w.pip_tab(),
            "player": w.player_shown(),
            "ok": hidden && w.player_shown() && !xaml::is_visible(media.view()),
        })
    })
    .await;
    for tab in &filler {
        window.close_tab(tab.id);
    }
    let back = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.pip_tab() == Some(media.id)).then_some(())
    })
    .await
    .is_some();
    select(window, media);
    let home = window.pip_tab().is_none() && width_if_shown(media).is_some_and(|w| w > 300.0);
    steps.push(json!({
        "name": "40-pip-returns-and-goes-home",
        "pip_after_closing_tabs": back,
        "view_back_in_the_page_grid": home,
        "ok": back && home,
    }));

    let browser = window.browser().ok_or("no browser")?;
    audio_steps(window, first, media, out_dir, steps).await?;

    select(window, first);
    browser.set_tab_pane_width(360);
    exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.pip_tab() == Some(media.id)).then_some(())
    })
    .await;
    exec::sleep(Duration::from_millis(700)).await;
    shoot(window, out_dir, "41-wider-pane", steps, |w| {
        let (pane, _, _) = w.layout_geometry();
        let pip = media.view().cast::<FrameworkElement>().ok().and_then(|v| {
            Some((v.ActualWidth().ok()?, v.ActualHeight().ok()?))
        });
        json!({
            "pane": format!("{pane:?}"),
            "pip": format!("{pip:?}"),
            "ok": pane.is_some_and(|p| (p.width - 360.0).abs() < 2.0)
                && pip.is_some_and(|(w, h)| w > 300.0 && (h - w * 9.0 / 16.0).abs() < 3.0),
        })
    })
    .await;
    browser.set_tab_pane_width(240);
    super::pip_steps::rows_and_switches(window, first, media, out_dir, steps).await
}
