//! Owned by W1.8.
//! Wave 1's checks (design section 8.3, W1.8): the profile, navigation, tabs and their layout, the
//! session, a first set of shortcuts and the screenshot, then section 7.5's standing regression
//! checks: start-up on one thread, keys through the two GPUI patches, the page-focus policy, resources
//! over open and close cycles, and shutdown, three cases of which run as re-executed phases.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, AsyncApp, Entity, Keystroke, WindowHandle};
use vsesvit_core::prefs::{TabsPosition, keys};
use vsesvit_core::private::Browsing;
use vsesvit_core::testkit::FixtureResponse;
use vsesvit_engine::{InitialPage, NativeHandle, TabId, TestShutdown, Url};

use crate::app::actions::{CloseTab, FocusAddress, NewTab, Quit, SelectTab1, SelectTab2};
use crate::app::quit::{self, EXIT_SHUTDOWN_STUCK, ExitIntent};
use crate::app::shell::Shell;
use crate::self_test::capture;
use crate::self_test::probes::{self, Process};
use crate::self_test::runner::{
    DEFAULT_TIMEOUT, Env, POLL, Probe, Run, dispatch, eval, first_window, loaded, loaded_at, selected, sleep, until,
};
use crate::tab::{Tab, TabChanged};
use crate::tab::model::Insert;
use crate::window::BrowserWindow;
use crate::window::layout::classify;

const FIXTURE_TITLE: &str = "Vsesvit fixture";
const PAGE2_TITLE: &str = "Vsesvit fixture 2";
/// A page with a text field that takes focus itself as it loads, and records the keys it sees.
const FOCUS_PATH: &str = "/vsesvit-selftest/focus";
const FOCUS_TITLE: &str = "Vsesvit focus fixture";
const FOCUS_PAGE: &str = r#"<!doctype html>
<title>Vsesvit focus fixture</title>
<style>body { margin: 0 } #field { position: absolute; left: 40px; top: 40px; width: 320px; height: 48px; font-size: 24px }</style>
<input id="field" autofocus>
<script>
window.__keys = [];
addEventListener('keydown', e => __keys.push(e.key));
addEventListener('load', () => document.getElementById('field').focus());
</script>"#;
/// What the focus page saw, as JSON.
const FOCUS_STATE: &str = "({ keys: window.__keys ? __keys.join('') : null, value: document.getElementById('field')?.value ?? null, \
    active: document.activeElement ? document.activeElement.id : null })";
const FOCUS_CLEAR: &str = "window.__keys = []; document.getElementById('field').value = ''; true";
/// The field's centre in CSS pixels, and the page's scale.
const FIELD_CENTRE: &str = "(() => { const r = document.getElementById('field').getBoundingClientRect(); \
    return { x: r.x + r.width / 2, y: r.y + r.height / 2, scale: devicePixelRatio }; })()";
/// Long enough for a focus request the policy refuses to have arrived and been refused.
const SETTLE: Duration = Duration::from_millis(700);
const RESOURCES_TIMEOUT: Duration = Duration::from_secs(240);
/// Q23: idle with one page loaded, every process of the browser.
const IDLE: Duration = Duration::from_secs(15);
const WARM_UP_CYCLES: usize = 5;
const CYCLES: usize = 25;
/// What Chromium's caches may hold on to after the cycles (SC section 6, SW section 6 saw no growth).
const HANDLE_SLACK: usize = 16;
/// Boot, the case, and a stuck shutdown's 15 s cap.
const PHASE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a phase's processes may take to end after it exited.
const PROCESSES_END: Duration = Duration::from_secs(10);
/// `DelayClose`'s delay: past the draft's old 5 s cap, under the 15 s one (design section 3.6).
const CLOSE_DELAY: Duration = Duration::from_secs(8);
const CEF_SHUTDOWN: &str = "vsesvit-cef: cef::shutdown took";

/// The checks, in the order of the GPUI flavour's list.
pub async fn run(run: &mut Run, cx: &mut AsyncApp) -> Result<(), String> {
    run.env.server.route(FOCUS_PATH, |_| FixtureResponse::ok("text/html; charset=utf-8", FOCUS_PAGE));
    cx.update(|cx| Shell::state(cx).update(cx, |state, cx| state.set_pref(&keys::SEARCH_SUGGESTIONS, &false, cx)));
    run.check("profile_open", DEFAULT_TIMEOUT, cx, profile_open).await;
    run.check("single_thread_init", DEFAULT_TIMEOUT, cx, single_thread_init).await;
    run.check("navigate", DEFAULT_TIMEOUT, cx, navigate).await;
    run.check("tabs", DEFAULT_TIMEOUT, cx, tabs_check).await;
    run.check("tab_layout", DEFAULT_TIMEOUT, cx, tab_layout).await;
    run.check("session", DEFAULT_TIMEOUT, cx, session).await;
    run.check("shortcuts", Duration::from_secs(30), cx, shortcuts).await;
    run.check("keys", DEFAULT_TIMEOUT, cx, keys_check).await;
    run.check("page_focus_policy", Duration::from_secs(60), cx, page_focus_policy).await;
    run.check("resources", RESOURCES_TIMEOUT, cx, resources).await;
    run.check("screenshot", DEFAULT_TIMEOUT, cx, screenshot).await;
    run.check("shutdown", PHASE_TIMEOUT, cx, shutdown).await;
    run.check("shutdown_delayed", PHASE_TIMEOUT, cx, shutdown_delayed).await;
    run.check("shutdown_stuck", PHASE_TIMEOUT, cx, shutdown_stuck).await;
    run.check("shutdown_late_creation", PHASE_TIMEOUT, cx, shutdown_late_creation).await;
    Ok(())
}

async fn profile_open(env: &Env, cx: &mut AsyncApp, _: &Probe) -> Result<String, String> {
    let root = cx.update(|cx| Shell::get(cx).location.root.clone());
    let want = env.out_dir.join("profile");
    if root != want {
        return Err(format!("the profile is {}, not {}", root.display(), want.display()));
    }
    Ok(format!("opened {}", root.display()))
}

async fn single_thread_init(_: &Env, _: &mut AsyncApp, _: &Probe) -> Result<String, String> {
    let threads = probes::threads_before_engine().ok_or("boot did not count the threads before the engine started")?;
    match threads {
        1 => Ok("1 thread ran when the engine started".into()),
        n if cfg!(windows) => Ok(format!("{n} threads ran when the engine started; only Linux's sandbox needs one")),
        n => Err(format!("{n} threads ran before cef::initialize; the sandbox wants one (design section 3.1)")),
    }
}

/// Types `text` into the window's address field and submits it.
fn submit(window: WindowHandle<BrowserWindow>, text: &str, cx: &mut AsyncApp) -> Result<(), String> {
    window.update(cx, |view, window, cx| view.submit_address(text, window, cx)).map_err(|e| e.to_string())
}

async fn navigate(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let url = env.server.url("/index.html");
    let tab = cx.update(|cx| selected(&env.view, cx)).ok_or("the window has no tab")?;
    submit(env.window, url.as_str(), cx)?;
    loaded(&tab, FIXTURE_TITLE, cx, probe).await;
    let at = cx.update(|cx| tab.read(cx).committed_url().cloned());
    match at {
        Some(at) if at == url => Ok(format!("title={FIXTURE_TITLE} url={at}")),
        other => Err(format!("the tab shows {other:?}, not {url}")),
    }
}

fn ids(env: &Env, cx: &mut AsyncApp) -> Vec<TabId> {
    cx.update(|cx| env.view.read(cx).tabs().read(cx).tabs().to_vec())
}

fn selected_id(env: &Env, cx: &mut AsyncApp) -> Option<TabId> {
    cx.update(|cx| env.view.read(cx).tabs().read(cx).selected())
}

async fn selects(env: &Env, want: TabId, cx: &mut AsyncApp, probe: &Probe) {
    until(cx, probe, |cx, p| {
        let now = env.view.read(cx).tabs().read(cx).selected();
        p.observe(format!("selected {now:?}, want {want:?}"));
        (now == Some(want)).then_some(())
    })
    .await
}

/// Waits until the window holds exactly `want`.
async fn holds(env: &Env, want: &[TabId], cx: &mut AsyncApp, probe: &Probe) {
    until(cx, probe, |cx, p| {
        let now = env.view.read(cx).tabs().read(cx).tabs().to_vec();
        p.observe(format!("tabs {now:?}, want {want:?}"));
        (now == want).then_some(())
    })
    .await
}

/// Waits for the tab a new-tab command opened after `before`, selected.
async fn new_selected_tab(env: &Env, before: &[TabId], cx: &mut AsyncApp, probe: &Probe) -> Entity<Tab> {
    until(cx, probe, |cx, p| {
        let now = env.view.read(cx).tabs().read(cx).tabs().to_vec();
        p.observe(format!("tabs {now:?} after {before:?}"));
        let new = now.iter().find(|id| !before.contains(id))?;
        let tab = Shell::state(cx).read(cx).tab(*new)?;
        (env.view.read(cx).tabs().read(cx).selected() == Some(*new)).then_some(tab)
    })
    .await
}

async fn tabs_check(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let first = selected_id(env, cx).ok_or("no tab is selected")?;
    let before = ids(env, cx);
    dispatch(env.window, NewTab, cx)?;
    let second = new_selected_tab(env, &before, cx, probe).await;
    let second_id = cx.update(|cx| second.read(cx).id());
    submit(env.window, env.server.url("/page2.html").as_str(), cx)?;
    loaded(&second, PAGE2_TITLE, cx, probe).await;
    let opened = ids(env, cx).len();
    dispatch(env.window, SelectTab1, cx)?;
    selects(env, first, cx, probe).await;
    dispatch(env.window, SelectTab2, cx)?;
    selects(env, second_id, cx, probe).await;
    dispatch(env.window, CloseTab, cx)?;
    holds(env, &[first], cx, probe).await;
    let url = cx.update(|cx| Shell::state(cx).read(cx).tab(first).and_then(|t| t.read(cx).committed_url().cloned()));
    let want = env.server.url("/index.html");
    if url.as_ref() != Some(&want) {
        return Err(format!("the remaining tab shows {url:?}, not {want}"));
    }
    Ok(format!("opened {opened} tabs, switched with Go to tab 1 and 2, after Close tab: 1 tab at {want}"))
}

async fn tab_layout(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let default = cx.update(|cx| Shell::state(cx).read(cx).pref(&keys::TABS_POSITION, cx));
    let mut seen = Vec::new();
    for (i, want) in [TabsPosition::Left, TabsPosition::Right, TabsPosition::Top, TabsPosition::Left].into_iter().enumerate() {
        if i > 0 {
            cx.update(|cx| Shell::state(cx).update(cx, |state, cx| state.set_pref(&keys::TABS_POSITION, &want, cx)));
        }
        let layout = until(cx, probe, |cx, p| {
            let layout = env.view.read(cx).layout_probe();
            p.observe(format!("want {want:?}, laid out {layout:?}"));
            (classify(&layout) == Some(want)).then_some(layout)
        })
        .await;
        let page = layout.page.map(|b| format!("page x={:.0}..{:.0} y={:.0}", f32::from(b.left()), f32::from(b.right()), f32::from(b.top())));
        seen.push(format!("{want:?}: {}", page.unwrap_or_default()));
    }
    let stored = cx.update(|cx| Shell::state(cx).read(cx).pref(&keys::TABS_POSITION, cx));
    let detail = format!("default {default:?}; {}; stored {stored:?}", seen.join("; "));
    (default == TabsPosition::Left && stored == TabsPosition::Left).then_some(detail.clone()).ok_or(detail)
}

async fn session(env: &Env, cx: &mut AsyncApp, _: &Probe) -> Result<String, String> {
    cx.update(|cx| Shell::state(cx).update(cx, |state, cx| state.save_session_now(cx)));
    let restored = cx
        .update(|cx| Shell::core(cx).borrow_mut().session().restore())
        .map_err(|e| e.to_string())?
        .ok_or("restore() returned nothing")?;
    let urls: Vec<Vec<String>> = restored.windows.iter().map(|w| w.tabs.iter().map(|t| t.url.to_string()).collect()).collect();
    let want = env.server.url("/index.html").to_string();
    let detail = format!("restored {} window(s): {urls:?}", restored.windows.len());
    urls.iter().flatten().any(|url| *url == want).then_some(detail.clone()).ok_or(detail)
}

/// Presses `keys` (GPUI's notation) in the window, as the keyboard would while GPUI has it.
fn press(env: &Env, keys: &str, cx: &mut AsyncApp) -> Result<(), String> {
    let keystroke = Keystroke::parse(keys).map_err(|e| format!("{keys}: {e:?}"))?;
    // Through the window alone: the window's own update would hold its view, which the key's action
    // then updates.
    cx.update_window(env.window.into(), |_, window, cx| window.dispatch_keystroke(keystroke, cx)).map_err(|e| e.to_string())?;
    Ok(())
}

/// The keymap's bindings while GPUI has the keyboard, through GPUI's key dispatch. The page's own
/// routing of these keys is the engine's, and the Settings editor is W2's.
async fn shortcuts(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let first = selected_id(env, cx).ok_or("no tab is selected")?;
    let before = ids(env, cx);
    press(env, "ctrl-t", cx)?;
    let second = new_selected_tab(env, &before, cx, probe).await;
    let second_id = cx.update(|cx| second.read(cx).id());
    let page2 = env.server.url("/page2.html");
    submit(env.window, page2.as_str(), cx)?;
    loaded(&second, PAGE2_TITLE, cx, probe).await;
    for (keys, want) in [("ctrl-1", first), ("ctrl-9", second_id), ("ctrl-shift-tab", first), ("ctrl-tab", second_id)] {
        press(env, keys, cx)?;
        selects(env, want, cx, probe).await;
    }
    press(env, "ctrl-w", cx)?;
    holds(env, &[first], cx, probe).await;
    press(env, "ctrl-shift-t", cx)?;
    let reopened = new_selected_tab(env, &[first], cx, probe).await;
    loaded(&reopened, PAGE2_TITLE, cx, probe).await;
    press(env, "ctrl-w", cx)?;
    holds(env, &[first], cx, probe).await;
    press(env, "ctrl-l", cx)?;
    until(cx, probe, |cx, p| {
        let focused = env.window.update(cx, |view, window, cx| view.address(window, cx).1).unwrap_or(false);
        p.observe(format!("address field focused: {focused}"));
        focused.then_some(())
    })
    .await;
    Ok("Ctrl+T opened a tab; Ctrl+1, Ctrl+9, Ctrl+Shift+Tab and Ctrl+Tab selected; Ctrl+W closed it; Ctrl+Shift+T reopened \
        page2.html; Ctrl+L focused the address field"
        .into())
}

/// Every tab's page, by its native window.
fn pages(cx: &mut AsyncApp) -> Vec<(TabId, NativeHandle)> {
    cx.update(|cx| {
        let state = Shell::state(cx);
        let ids: Vec<TabId> = state.read(cx).windows().iter().flat_map(|w| w.tabs.read(cx).tabs().to_vec()).collect();
        ids.into_iter().filter_map(|id| Some((id, state.read(cx).tab(id)?.read(cx).native()?))).collect()
    })
}

/// Where the keyboard is, as GPUI and the system each see it.
#[derive(Debug)]
struct Keyboard {
    /// GPUI's focus is in the window's address field.
    address: bool,
    active: bool,
    /// The page whose native window has the system's keyboard focus.
    page: Option<TabId>,
    native: Option<NativeHandle>,
}

impl std::fmt::Display for Keyboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let page = self.page.map_or("no page".to_owned(), |t| format!("tab {}", t.get()));
        write!(f, "address field {}, window {}, {page} has the keyboard", focus(self.address), if self.active { "active" } else { "inactive" })
    }
}

fn focus(on: bool) -> &'static str {
    if on { "focused" } else { "unfocused" }
}

fn keyboard(window: WindowHandle<BrowserWindow>, cx: &mut AsyncApp) -> Result<Keyboard, String> {
    let (address, active) =
        window.update(cx, |view, window, cx| (view.address(window, cx).1, window.is_window_active())).map_err(|e| e.to_string())?;
    let native = probes::native_focus();
    let page = native.and_then(|n| pages(cx).into_iter().find(|(_, p)| probes::within(n, *p)).map(|(t, _)| t));
    Ok(Keyboard { address, active, page, native })
}

/// Waits until `want` holds for the keyboard; the check's timeout bounds it.
async fn keyboard_until(
    window: WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    probe: &Probe,
    case: &str,
    want: impl Fn(&Keyboard) -> bool,
) -> Result<String, String> {
    loop {
        let seen = keyboard(window, cx)?;
        probe.observe(format!("{case}: {seen} ({:?})", seen.native));
        if want(&seen) {
            return Ok(format!("{case}: {seen}"));
        }
        sleep(cx, POLL).await;
    }
}

/// After the page has had time to ask for focus, `want` must hold.
async fn keyboard_stays(
    window: WindowHandle<BrowserWindow>,
    cx: &mut AsyncApp,
    probe: &Probe,
    case: &str,
    want: impl Fn(&Keyboard) -> bool,
) -> Result<String, String> {
    sleep(cx, SETTLE).await;
    let seen = keyboard(window, cx)?;
    probe.observe(format!("{case}: {seen}"));
    if want(&seen) { Ok(format!("{case}: {seen}")) } else { Err(format!("{case}: {seen} ({:?})", seen.native)) }
}

/// Waits until the page's state, read by script, satisfies `want`.
async fn page_until(tab: TabId, cx: &mut AsyncApp, probe: &Probe, want: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    loop {
        let state = eval(tab, FOCUS_STATE, cx).await.unwrap_or_else(serde_json::Value::String);
        probe.observe(format!("page {}: {state}", tab.get()));
        if want(&state) {
            return state;
        }
        sleep(cx, POLL).await;
    }
}

/// Types `c` as the keyboard does: into whatever has the system's keyboard focus.
fn type_char(env: &Env, c: char) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        env.input.key(probes::keycode(u32::from(c))?)
    }
    #[cfg(windows)]
    {
        let focused = probes::native_focus().ok_or("nothing has the keyboard")?;
        env.input.post_key(focused.hwnd(), c.to_ascii_uppercase() as u16)
    }
}

/// The pointer moves over `page` at (`x`, `y`), the page's own physical pixels.
fn point_at(env: &Env, page: NativeHandle, x: i32, y: i32) -> Result<(), String> {
    let (sx, sy) = probes::to_screen(page, x, y)?;
    #[cfg(target_os = "linux")]
    {
        env.input.move_pointer(screen(sx)?, screen(sy)?)
    }
    #[cfg(windows)]
    {
        let (target, tx, ty) = probes::target_at(page, sx, sy);
        env.input.post_mouse_move(target, screen(tx)?, screen(ty)?)
    }
}

/// A left click on `page` at (`x`, `y`), the page's own physical pixels.
fn click_at(env: &Env, page: NativeHandle, x: i32, y: i32) -> Result<(), String> {
    let (sx, sy) = probes::to_screen(page, x, y)?;
    #[cfg(target_os = "linux")]
    {
        env.input.click(screen(sx)?, screen(sy)?)
    }
    #[cfg(windows)]
    {
        let (target, tx, ty) = probes::target_at(page, sx, sy);
        env.input.post_click(target, screen(tx)?, screen(ty)?)
    }
}

fn screen(v: i32) -> Result<i16, String> {
    i16::try_from(v).map_err(|_| format!("{v} is off any screen"))
}

/// Loads the focus page (`?query`) in the selected tab from the address field, which gives the page
/// the keyboard once it shows; returns the tab and its page's window.
async fn focus_page_selected(
    env: &Env,
    query: &str,
    cx: &mut AsyncApp,
    probe: &Probe,
) -> Result<(Entity<Tab>, TabId, NativeHandle), String> {
    let tab = cx.update(|cx| selected(&env.view, cx)).ok_or("no tab is selected")?;
    let url = env.server.url(&format!("{FOCUS_PATH}?{query}"));
    submit(env.window, url.as_str(), cx)?;
    loaded_at(&tab, &url, FOCUS_TITLE, cx, probe).await;
    let (id, native) = cx.update(|cx| (tab.read(cx).id(), tab.read(cx).native()));
    Ok((tab, id, native.ok_or("the tab has no page")?))
}

/// Section 7.5's keys: a key reaches the page that has the keyboard (the Windows key patch lets it
/// through GPUI's message loop), and a key typed into the address field with the pointer over the
/// page stays there (the X11 focus patch keeps it from the window under the pointer).
async fn keys_check(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let (_, tab, page) = focus_page_selected(env, "keys", cx, probe).await?;
    keyboard_until(env.window, cx, probe, "the page took the keyboard", |k| k.page == Some(tab)).await?;
    eval(tab, FOCUS_CLEAR, cx).await?;
    type_char(env, 'k')?;
    page_until(tab, cx, probe, |s| s["keys"] == "k" && s["value"] == "k").await;

    dispatch(env.window, FocusAddress, cx)?;
    keyboard_until(env.window, cx, probe, "the address field took the keyboard", |k| k.address && k.page.is_none()).await?;
    let content = cx.update(|cx| env.view.read(cx).page_slot().content()).ok_or("the page has no place")?;
    let (x, y) = (i32::try_from(content.width / 2).unwrap_or(0), i32::try_from(content.height / 2).unwrap_or(0));
    point_at(env, page, x, y)?;
    sleep(cx, POLL).await;
    type_char(env, 'q')?;
    until(cx, probe, |cx, p| {
        let text = env.window.update(cx, |view, window, cx| view.address(window, cx).0).unwrap_or_default();
        p.observe(format!("address field reads {text:?}"));
        (text == "q").then_some(())
    })
    .await;
    sleep(cx, SETTLE).await;
    let state = eval(tab, FOCUS_STATE, cx).await?;
    if state["keys"] != "k" {
        return Err(format!("the page saw the key typed into the address field: {state}"));
    }
    Ok("a key typed with the page's keyboard reached its field; with the pointer over the page, a key typed into the \
        address field stayed there"
        .into())
}

/// Section 1.5's page-focus policy: a page gets the keyboard only in the active window's selected tab
/// when the user gave it the keyboard; the extension popup case waits for W4.3's popups.
async fn page_focus_policy(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let mut seen = Vec::new();
    let (main_tab, tab, page) = focus_page_selected(env, "policy", cx, probe).await?;
    let window_id = cx.update(|cx| env.view.read(cx).id());
    dispatch(env.window, FocusAddress, cx)?;
    seen.push(keyboard_until(env.window, cx, probe, "address field", |k| k.address && k.page.is_none()).await?);

    let background_url = env.server.url(&format!("{FOCUS_PATH}?background"));
    let background = cx.update(|cx| {
        Shell::state(cx).update(cx, |state, cx| {
            let tab = state.new_tab_entity(Browsing::Normal, InitialPage::Url(background_url.clone()), None, cx);
            state.attach_tab(window_id, &tab, Insert::End, false, cx);
            tab
        })
    });
    loaded_at(&background, &background_url, FOCUS_TITLE, cx, probe).await;
    let kept = |k: &Keyboard| k.address && k.page.is_none() && k.active;
    seen.push(keyboard_stays(env.window, cx, probe, "a background tab's page focused itself", kept).await?);

    let again = env.server.url(&format!("{FOCUS_PATH}?background-navigation"));
    cx.update(|cx| background.update(cx, |tab, cx| tab.load(again.clone(), None, cx)));
    until(cx, probe, |cx, p| {
        let t = background.read(cx);
        p.observe(format!("background tab at {:?}, loading {}", t.committed_url().map(Url::as_str), t.loading()));
        (t.committed_url() == Some(&again) && !t.loading()).then_some(())
    })
    .await;
    seen.push(keyboard_stays(env.window, cx, probe, "a background navigation", kept).await?);

    eval(tab, "document.getElementById('field').focus(); true", cx).await?;
    seen.push(keyboard_stays(env.window, cx, probe, "the selected page's script focused its field", kept).await?);

    probes::force_native_focus(page)?;
    seen.push(keyboard_until(env.window, cx, probe, "the page's window focused natively, then given back", kept).await?);

    let at = eval(tab, FIELD_CENTRE, cx).await?;
    let coordinate = |key: &str| at[key].as_f64().unwrap_or(0.0) * at["scale"].as_f64().unwrap_or(1.0);
    click_at(env, page, coordinate("x").round() as i32, coordinate("y").round() as i32)?;
    let clicked = keyboard_until(env.window, cx, probe, "a click into the page's field", |k| k.page == Some(tab)).await?;
    seen.push(clicked);
    page_until(tab, cx, probe, |s| s["active"] == "field").await;
    eval(tab, FOCUS_CLEAR, cx).await?;
    type_char(env, 'v')?;
    page_until(tab, cx, probe, |s| s["value"] == "v").await;
    seen.push("a key typed after the click reached the field".into());

    let before: Vec<_> = cx.update(|cx| Shell::state(cx).read(cx).windows().iter().map(|w| w.id).collect());
    cx.update(|cx| Shell::state(cx).update(cx, |state, cx| state.open_window(Browsing::Normal, vec![env.server.url("/index.html")], cx)));
    let other = until(cx, probe, |cx, p| {
        let (handle, view) = first_window(cx)?;
        let id = view.read(cx).id();
        p.observe(format!("windows {before:?}, first now {id:?}"));
        if before.contains(&id) {
            return None;
        }
        let active = handle.update(cx, |_, window, _| window.is_window_active()).unwrap_or(false);
        active.then_some(handle)
    })
    .await;
    let inactive_load = env.server.url(&format!("{FOCUS_PATH}?inactive-window"));
    // Activation settles over a few turns while the new window and its first page are created; the
    // case starts once the shell, too, holds the first window inactive.
    let mut inactive_since: Option<Instant> = None;
    until(cx, probe, |cx, p| {
        let policy = crate::engine_bridge::focus(cx).published(tab);
        let first = env.window.update(cx, |_, window, _| window.is_window_active()).unwrap_or(true);
        p.observe(format!("first window active {first}, its page's policy {policy:?}"));
        if first || policy.is_none_or(|p| p.window_active) {
            inactive_since = None;
            return None;
        }
        (inactive_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(300)).then_some(())
    })
    .await;
    cx.update(|cx| main_tab.update(cx, |tab, cx| tab.load(inactive_load.clone(), None, cx)));
    until(cx, probe, |cx, p| {
        let t = main_tab.read(cx);
        p.observe(format!("inactive window's tab at {:?}, loading {}", t.committed_url().map(Url::as_str), t.loading()));
        (t.committed_url() == Some(&inactive_load) && !t.loading()).then_some(())
    })
    .await;
    let other_active = |cx: &mut AsyncApp| other.update(cx, |_, window, _| window.is_window_active()).unwrap_or(false);
    let stayed = keyboard_stays(env.window, cx, probe, "a page in an inactive window focused itself", |k| !k.active && k.page != Some(tab)).await?;
    if !other_active(cx) {
        return Err(format!("{}; {stayed}, and the other window lost activation", seen.join("; ")));
    }
    seen.push(format!("{stayed}, the other window stayed active"));
    dispatch(other, CloseTab, cx)?;
    until(cx, probe, |cx, p| {
        let count = Shell::state(cx).read(cx).windows().len();
        p.observe(format!("{count} windows"));
        (count == 1).then_some(())
    })
    .await;
    let background_id = cx.update(|cx| background.read(cx).id());
    env.window
        .update(cx, |view, _, cx| {
            view.tabs().update(cx, |model, cx| model.select(background_id, cx));
        })
        .map_err(|e| e.to_string())?;
    selects(env, background_id, cx, probe).await;
    dispatch(env.window, CloseTab, cx)?;
    holds(env, &[tab], cx, probe).await;
    seen.push("an extension popup's case waits for W4.3".into());
    Ok(seen.join("; "))
}

/// Opens a tab on the fixture page, then closes it as Ctrl+W does.
async fn cycle(env: &Env, n: usize, cx: &mut AsyncApp, probe: &Probe) -> Result<(), String> {
    let window_id = cx.update(|cx| env.view.read(cx).id());
    let url = env.server.url(&format!("/index.html?cycle={n}"));
    let tab = cx.update(|cx| {
        Shell::state(cx).update(cx, |state, cx| {
            let tab = state.new_tab_entity(Browsing::Normal, InitialPage::Url(url), None, cx);
            state.attach_tab(window_id, &tab, Insert::End, true, cx);
            tab
        })
    });
    let id = cx.update(|cx| tab.read(cx).id());
    loaded(&tab, FIXTURE_TITLE, cx, probe).await;
    drop(tab);
    dispatch(env.window, CloseTab, cx)?;
    until(cx, probe, |cx, p| {
        let open = Shell::state(cx).read(cx).tab(id).is_some();
        p.observe(format!("cycle {n}: tab {} still open: {open}", id.get()));
        (!open).then_some(())
    })
    .await;
    Ok(())
}

/// CPU time of `processes` so far.
fn cpu(processes: &[Process]) -> Duration {
    processes.iter().filter_map(|p| probes::cpu_time(*p)).sum()
}

/// Section 7.5's resources, and Q23's idle CPU (recorded in the detail, not judged).
async fn resources(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let me = probes::me().ok_or("this process is not in the process table")?;
    sleep(cx, Duration::from_secs(2)).await;
    let processes: Vec<Process> = std::iter::once(me).chain(probes::descendants(me.pid)).collect();
    probe.observe(format!("measuring idle CPU over {} processes", processes.len()));
    let (before, started) = (cpu(&processes), Instant::now());
    sleep(cx, IDLE).await;
    let (used, over) = (cpu(&processes).saturating_sub(before), started.elapsed());
    let build = if cfg!(debug_assertions) { "debug" } else { "release" };
    let idle = format!(
        "idle CPU {:.2} % of one core over {:.1} s, {} processes, {build} build",
        used.as_secs_f64() / over.as_secs_f64() * 100.0,
        over.as_secs_f64(),
        processes.len()
    );
    log::info!("self-test: {idle}");

    for n in 0..WARM_UP_CYCLES {
        cycle(env, n, cx, probe).await?;
    }
    sleep(cx, Duration::from_secs(2)).await;
    let baseline = probes::usage();
    for n in WARM_UP_CYCLES..WARM_UP_CYCLES + CYCLES {
        cycle(env, n, cx, probe).await?;
    }
    sleep(cx, Duration::from_secs(2)).await;
    // Renderers of closed tabs may take a moment more to go.
    let settled = until(cx, probe, |_, p| {
        let now = probes::usage();
        p.observe(format!("baseline {baseline}; after {CYCLES} cycles {now}"));
        (now.children <= baseline.children).then_some(now)
    })
    .await;
    let detail = format!("{idle}; baseline after {WARM_UP_CYCLES} cycles: {baseline}; after {CYCLES} more: {settled}");
    let grew = |before: usize, after: usize| after > before + HANDLE_SLACK;
    let gui_grew = match (baseline.gui, settled.gui) {
        (Some((gdi, user)), Some((gdi_now, user_now))) => grew(gdi as usize, gdi_now as usize) || grew(user as usize, user_now as usize),
        _ => false,
    };
    if settled.children != baseline.children || grew(baseline.handles, settled.handles) || gui_grew {
        return Err(detail);
    }
    Ok(detail)
}

async fn screenshot(env: &Env, cx: &mut AsyncApp, _: &Probe) -> Result<String, String> {
    sleep(cx, Duration::from_millis(500)).await;
    let shot = capture::window(&env.view, cx)?;
    let path = env.out_dir.join("window.png");
    capture::save(&shot, &path)?;
    let content = cx.update(|cx| env.view.read(cx).page_slot().content()).ok_or("the page has no place")?;
    let page = image::imageops::crop_imm(
        &shot,
        u32::try_from(content.x).unwrap_or(0),
        u32::try_from(content.y).unwrap_or(0),
        content.width,
        content.height,
    )
    .to_image();
    let (window, inside) = (capture::colours(&shot, 64), capture::colours(&page, 64));
    let detail = format!("{} ({}x{}, {window} colours; the page's {}x{} area {inside})", path.display(), shot.width(), shot.height(), page.width(), page.height());
    (window > 1 && inside > 1).then_some(detail.clone()).ok_or(detail)
}

/// A phase's run, as its parent saw it.
struct PhaseRun {
    name: &'static str,
    code: Option<i32>,
    took: Duration,
    log: String,
    processes: usize,
    left: Vec<Process>,
}

impl PhaseRun {
    fn exited(&self, want: i32) -> Result<(), String> {
        match self.code {
            Some(code) if code == want => Ok(()),
            other => Err(format!("phase {} exited with {other:?}, not {want}; {}", self.name, self.tail())),
        }
    }

    fn logged(&self, line: &str) -> Result<(), String> {
        if self.log.contains(line) { Ok(()) } else { Err(format!("phase {}'s log lacks {line:?}; {}", self.name, self.tail())) }
    }

    fn all_gone(&self) -> Result<(), String> {
        if self.left.is_empty() {
            Ok(())
        } else {
            Err(format!("phase {} left {} of its {} processes running: {:?}", self.name, self.left.len(), self.processes, self.left))
        }
    }

    fn summary(&self) -> String {
        format!(
            "exit {:?} after {:.1} s; its {} processes ended",
            self.code.unwrap_or(-1),
            self.took.as_secs_f64(),
            self.processes
        )
    }

    /// The end of the phase's log, for a failure's detail.
    fn tail(&self) -> String {
        let lines: Vec<&str> = self.log.lines().rev().take(6).collect();
        format!("its log ends: {}", lines.into_iter().rev().collect::<Vec<_>>().join(" | "))
    }
}

/// Ends a phase still running when its check gives up.
struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Re-executes this browser as `--self-test OUT_DIR/phases/NAME --phase NAME` in the same test
/// environment, on a profile of its own, and waits for it and every process it started.
async fn run_phase(env: &Env, name: &'static str, cx: &mut AsyncApp, probe: &Probe) -> Result<PhaseRun, String> {
    let dir = env.out_dir.join("phases").join(name);
    fresh(&dir)?;
    let image = quit::relaunch_image().map_err(|e| format!("the browser's image: {e}"))?;
    let output = |file: &str| std::fs::File::create(dir.join(file)).map_err(|e| format!("{}: {e}", dir.join(file).display()));
    let child = Command::new(&image)
        .arg("--self-test")
        .arg(&dir)
        .arg("--phase")
        .arg(name)
        .stdin(Stdio::null())
        .stdout(output("stdout.txt")?)
        .stderr(output("stderr.txt")?)
        .spawn()
        .map_err(|e| format!("starting {}: {e}", image.display()))?;
    let mut child = Child(child);
    let (pid, started) = (child.0.id(), Instant::now());
    let mut seen: HashSet<Process> = HashSet::new();
    let code = loop {
        seen.extend(probes::descendants(pid));
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            break status.code();
        }
        probe.observe(format!("phase {name} running for {:.1} s with {} processes", started.elapsed().as_secs_f64(), seen.len()));
        sleep(cx, POLL).await;
    };
    let took = started.elapsed();
    let ended = Instant::now();
    let left = loop {
        let left: Vec<Process> = seen.iter().copied().filter(|p| probes::alive(*p)).collect();
        if left.is_empty() || ended.elapsed() > PROCESSES_END {
            break left;
        }
        probe.observe(format!("phase {name} exited; {} of its processes still run", left.len()));
        sleep(cx, POLL).await;
    };
    let log = std::fs::read_to_string(dir.join("vsesvit.log")).unwrap_or_default();
    Ok(PhaseRun { name, code, took, log, processes: seen.len(), left })
}

fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// Quit with a page open: exit 0, `cef::shutdown`, and no process left (SC section 12, SW section 12).
async fn shutdown(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let phase = run_phase(env, "shutdown", cx, probe).await?;
    phase.exited(0)?;
    phase.logged(CEF_SHUTDOWN)?;
    phase.all_gone()?;
    Ok(format!("Quit: {}", phase.summary()))
}

/// A browser whose close takes 8 s: the sequence waits for it and still ends drained.
async fn shutdown_delayed(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let phase = run_phase(env, "shutdown-delayed", cx, probe).await?;
    phase.exited(0)?;
    phase.logged(CEF_SHUTDOWN)?;
    let after = closed_after(&phase.log).ok_or_else(|| format!("the phase did not log when its tab closed; {}", phase.tail()))?;
    if after < CLOSE_DELAY.saturating_sub(Duration::from_millis(500)) {
        return Err(format!("the delayed tab closed {} ms after the quit, before its {CLOSE_DELAY:?}", after.as_millis()));
    }
    phase.all_gone()?;
    Ok(format!("the tab closed {} ms after the quit; {}", after.as_millis(), phase.summary()))
}

/// A browser that never closes: the cap ends the run with 70, logs the live entry, calls no
/// `cef::shutdown`, and frees the profile's lock.
async fn shutdown_stuck(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let phase = run_phase(env, "shutdown-stuck", cx, probe).await?;
    phase.exited(i32::from(EXIT_SHUTDOWN_STUCK))?;
    phase.logged("vsesvit-cef: still alive:")?;
    if phase.log.contains(CEF_SHUTDOWN) {
        return Err(format!("a stuck shutdown still called cef::shutdown; {}", phase.tail()));
    }
    let profile = env.out_dir.join("phases").join("shutdown-stuck").join("profile");
    let options = vsesvit_core::OpenOptions { chrome_version: "154.0.8037.98".into(), ..vsesvit_core::OpenOptions::default() };
    drop(vsesvit_core::Profile::open(&profile, options).map_err(|e| format!("the stuck run's profile cannot be opened again: {e}"))?);
    phase.all_gone()?;
    Ok(format!("still-alive entry logged, no cef::shutdown, the profile's lock free; {}", phase.summary()))
}

/// A tab whose creation completes only after `begin_shutdown`: closed at once, and the run drains.
async fn shutdown_late_creation(env: &Env, cx: &mut AsyncApp, probe: &Probe) -> Result<String, String> {
    let phase = run_phase(env, "shutdown-late-creation", cx, probe).await?;
    phase.exited(0)?;
    phase.logged(CEF_SHUTDOWN)?;
    phase.logged(HELD_TAB_CLOSED)?;
    phase.all_gone()?;
    Ok(format!("the held tab closed without showing a page; {}", phase.summary()))
}

/// What the late-creation phase logs once its held tab ended without a page.
const HELD_TAB_CLOSED: &str = "self-test phase: the held tab closed without a page";
const CLOSED_AFTER: &str = "self-test phase: ms from the quit to the delayed tab's close: ";

/// The delay the delayed phase logged.
fn closed_after(log: &str) -> Option<Duration> {
    let line = log.lines().find_map(|line| line.split_once(CLOSED_AFTER))?.1;
    line.trim().parse().ok().map(Duration::from_millis)
}

/// A re-executed run's one scripted thing (`--phase NAME`). Every phase quits itself.
pub async fn phase(name: &str, cx: &mut AsyncApp) -> Result<(), String> {
    let started = Instant::now();
    let (window, view) = loop {
        if let Some(found) = cx.update(first_window) {
            break found;
        }
        if started.elapsed() > Duration::from_secs(60) {
            return Err("no browser window".into());
        }
        sleep(cx, POLL).await;
    };
    let probe = Probe::default();
    match name {
        "shutdown" => {
            let tab = cx.update(|cx| selected(&view, cx)).ok_or("no tab")?;
            created(&tab, cx, &probe).await;
            dispatch(window, Quit, cx)
        }
        "shutdown-delayed" => {
            let tab = armed(&view, |tab| TestShutdown::DelayClose { tab, delay: CLOSE_DELAY }, cx)?;
            created(&tab, cx, &probe).await;
            let asked = Instant::now();
            on_end(&tab, cx, move |_| log::info!("{CLOSED_AFTER}{}", asked.elapsed().as_millis()));
            cx.update(|cx| quit::request(ExitIntent::Quit, cx));
            Ok(())
        }
        "shutdown-stuck" => {
            let tab = armed(&view, |tab| TestShutdown::PinOpen { tab }, cx)?;
            created(&tab, cx, &probe).await;
            cx.update(|cx| quit::request(ExitIntent::Quit, cx));
            Ok(())
        }
        "shutdown-late-creation" => {
            let tab = armed(&view, |tab| TestShutdown::HoldCreation { tab }, cx)?;
            until(cx, &probe, |cx, _| (!tab.read(cx).needs_browser()).then_some(())).await;
            on_end(&tab, cx, |had_page| match had_page {
                false => log::info!("{HELD_TAB_CLOSED}"),
                true => log::error!("self-test phase: the held tab showed a page after the quit"),
            });
            cx.update(|cx| quit::request(ExitIntent::Quit, cx));
            Ok(())
        }
        other => Err(format!("no phase {other:?}")),
    }
}

/// A selected tab in `view`'s window, armed with a shutdown case before its browser is created.
fn armed(view: &Entity<BrowserWindow>, case: impl FnOnce(TabId) -> TestShutdown, cx: &mut AsyncApp) -> Result<Entity<Tab>, String> {
    cx.update(|cx| {
        let window = view.read(cx).id();
        let engine = Shell::engine(cx);
        Shell::state(cx).update(cx, |state, cx| {
            let tab = state.new_tab_entity(Browsing::Normal, InitialPage::Blank, None, cx);
            let id = tab.read(cx).id();
            engine.test_shutdown(case(id)).map_err(|e| format!("arming tab {}: {e}", id.get()))?;
            state.attach_tab(window, &tab, Insert::End, true, cx);
            Ok(tab)
        })
    })
}

async fn created(tab: &Entity<Tab>, cx: &mut AsyncApp, probe: &Probe) {
    until(cx, probe, |cx, _| tab.read(cx).native().map(drop)).await
}

/// Runs `done` when `tab` ends, with whether it ever had a page. A callback, not a poll: once the
/// last browser closes, the run quits within the same turn.
fn on_end(tab: &Entity<Tab>, cx: &mut AsyncApp, done: impl FnOnce(bool) + 'static) {
    let had_page = Rc::new(Cell::new(false));
    let done = Cell::new(Some(done));
    cx.update(|cx| {
        let seen = had_page.clone();
        cx.observe(tab, move |tab, cx| seen.set(seen.get() || tab.read(cx).native().is_some())).detach();
        cx.subscribe(tab, move |_, change: &TabChanged, _| {
            if matches!(change, TabChanged::EngineClosed | TabChanged::ClosedByPage)
                && let Some(done) = done.take()
            {
                done(had_page.get());
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_is_read_back_from_the_phases_log() {
        let log = format!("[   9.002 INFO  18392 self_test::checks::foundation] {CLOSED_AFTER}8013\n[   9.1 INFO x] other\n");
        assert_eq!(closed_after(&log), Some(Duration::from_millis(8013)));
        assert_eq!(closed_after("nothing"), None);
    }

    /// The registry's GPUI list is what this file runs, in its order (design section 7.2's guard).
    #[test]
    fn checks_are_the_ones_the_run_makes_in_its_order() {
        let source = include_str!("foundation.rs");
        let run = &source[source.find("pub async fn run(").unwrap()..source.find("async fn profile_open").unwrap()];
        let names: Vec<&str> = run.split("run.check(\"").skip(1).map(|rest| &rest[..rest.find('"').unwrap()]).collect();
        let expected = vsesvit_core::testkit::checks::expected(vsesvit_core::testkit::checks::Flavour::Gpui, false);
        assert_eq!(names, expected);
    }
}
