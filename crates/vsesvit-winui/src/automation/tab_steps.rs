//! A tab that plays sound shows a speaker, and its context menu pins it, splits the view with
//! it and copies its link; Ctrl+Shift+C and Ctrl+Alt+Shift+C copy the clean and the whole link.
//! The sidebar player follows the tab, drives its page, and shows its video in the pane while
//! another tab is selected, until the tabs grow into that space.
//! The media page plays a generated video with a tone, muted before it starts, so the run makes
//! no sound.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::{STEP_TIMEOUT, devtools, press, shoot, wait_loaded};
use crate::bindings::*;
use crate::media::MediaAction;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::tab_header::Audio;
use crate::window::{BrowserWindow, TabAction};
use crate::{exec, xaml};

/// A canvas animation and a quiet tone as one media stream in a `<video>`; `start()` plays it.
pub(super) const MEDIA_PAGE: &str = "data:text/html,<title>Media</title>\
<body style='margin:0;background:rgb(10,20,40)'><video id=v width=320 height=180></video><script>\
const c=document.createElement('canvas');c.width=320;c.height=180;const g=c.getContext('2d');let f=0;\
setInterval(()=>{f++;g.fillStyle='rgb('+(f*5%256)+',90,160)';g.fillRect(0,0,320,180);\
g.fillStyle='white';g.font='64px sans-serif';g.fillText(f,40,120)},33);\
navigator.mediaSession.metadata=new MediaMetadata({title:'Smoke tone',artist:'Vsesvit'});\
navigator.mediaSession.setActionHandler('nexttrack',()=>{document.title='Next track'});\
window.start=async()=>{const a=new AudioContext();const o=a.createOscillator();const n=a.createGain();\
n.gain.value=0.01;const d=a.createMediaStreamDestination();o.connect(n).connect(d);o.start();\
const s=c.captureStream(30);s.addTrack(d.stream.getAudioTracks()[0]);const v=document.getElementById('v');\
v.srcObject=s;await v.play();return 'playing'};</script></body>";

/// Opens the media page in a background tab, muted, and plays it.
pub(super) async fn open_media(window: &Rc<BrowserWindow>) -> Result<Rc<Tab>, String> {
    let media = window
        .open_url_tab(MEDIA_PAGE, false)
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

fn menu_labels(window: &BrowserWindow, tab: &Tab) -> Result<Vec<String>, String> {
    let menu = MenuFlyout::new().map_err(|e| e.to_string())?;
    window.fill_tab_menu(tab.id, &menu);
    let items = menu.Items().map_err(|e| e.to_string())?;
    Ok((0..items.Size().unwrap_or(0))
        .filter_map(|i| items.GetAt(i).ok())
        .filter_map(|item| item.cast::<MenuFlyoutItem>().ok())
        .filter_map(|item| item.Text().ok())
        .map(|text| text.to_string())
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

pub(super) async fn run(
    window: &Rc<BrowserWindow>,
    server: &FixtureServer,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let first = window.active_tab().ok_or("no tab")?;
    let media = open_media(window).await?;
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
    steps.push(json!({
        "name": "31-tab-menu",
        "labels": labels,
        "ok": labels == ["Split view with current tab", "Pin tab", "Unmute tab", "Copy link", "Close tab"],
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
    window.tab_action(media.id, TabAction::Pin(false));
    let unpinned = order(window);
    steps.push(json!({
        "name": "33-unpinned-tab",
        "order": unpinned,
        "ok": !media.is_pinned() && unpinned.first() == Some(&media.id),
    }));

    select(window, &first);
    window.tab_action(media.id, TabAction::SplitWithActive);
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

async fn player_steps(
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    media: &Rc<Tab>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    select(window, first);
    let in_pip = exec::wait_for(STEP_TIMEOUT, Duration::from_millis(100), || {
        (window.pip_tab() == Some(media.id)).then_some(())
    })
    .await
    .is_some();
    exec::sleep(Duration::from_millis(700)).await;
    shoot(window, out_dir, "37-sidebar-player-pip", steps, |w| {
        json!({
            "followed": w.media_tab(),
            "pip": w.pip_tab(),
            "player": w.player_shown(),
            "ok": in_pip && w.media_tab() == Some(media.id) && w.player_shown()
                && xaml::is_visible(media.view()),
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
    Ok(())
}
