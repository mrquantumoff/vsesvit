//! The `memory_saver` check: a background tab left alone past Memory Saver's delay sleeps in
//! WebView2 and keeps its address, title and history, while a tab with unsaved form input, a
//! pinned tab and the selected tab stay awake; selecting it wakes it, a tab pinned while its page
//! answers the sweep stays awake, and with Memory Saver off nothing sleeps.

use std::rc::Rc;
use std::time::{Duration, Instant};

use vsesvit_core::memory_saver::{UNSAVED_INPUT_SCRIPT, has_unsaved_input};
use vsesvit_core::prefs::keys;
use vsesvit_core::testkit::FixtureServer;

use super::tab_menu_checks::loaded;
use super::{FIXTURE_TITLE, PAGE2_TITLE, Probe, eval, tab_ids, until};
use crate::browser::Browser;
use crate::exec;
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::{BrowserWindow, TabAction};

/// Past every Memory Saver delay.
const LEFT_ALONE: Duration = Duration::from_secs(7 * 60 * 60);
/// Long enough for a sweep's scripts and suspensions to have run.
const SETTLED: Duration = Duration::from_secs(2);
/// Types into a field the page did not load with.
const TYPE_INTO_FIELD: &str = "(() => { const field = document.createElement('input'); \
    document.body.append(field); field.value = 'typed'; return field.value; })()";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Asleep as the shell shows it and as WebView2 has it.
fn asleep(tab: &Tab) -> (bool, bool) {
    (tab.look().asleep, tab.suspended())
}

fn select(window: &BrowserWindow, tab: &Tab) -> Result<(), String> {
    let index = tab_ids(window)
        .iter()
        .position(|id| *id == tab.id)
        .and_then(|i| u8::try_from(i).ok())
        .ok_or_else(|| format!("tab {} is not in the window", tab.id))?;
    window.run(Command::SelectTab(index));
    Ok(())
}

pub(super) async fn memory_saver(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let index = server.url("/index.html");
    let page2 = server.url("/page2.html");

    let idle = window.open_url_tab(index.as_str(), false).map_err(err)?;
    loaded(&idle, index.as_str(), FIXTURE_TITLE, p).await;
    idle.navigate(page2.as_str());
    loaded(&idle, page2.as_str(), PAGE2_TITLE, p).await;
    let typed = window.open_url_tab(index.as_str(), false).map_err(err)?;
    loaded(&typed, index.as_str(), FIXTURE_TITLE, p).await;
    eval(&typed, TYPE_INTO_FIELD).await?;
    let unsaved = has_unsaved_input(&eval(&typed, UNSAVED_INPUT_SCRIPT).await?);
    let pinned = window.open_url_tab(index.as_str(), false).map_err(err)?;
    loaded(&pinned, index.as_str(), FIXTURE_TITLE, p).await;
    window.tab_action(pinned.id, TabAction::Pin(true));
    let selected = window.active_tab().is_some_and(|t| t.id == first.id);
    detail.push(format!(
        "left alone: tab {} at {} (back {}), typed into tab {} (unsaved input {unsaved}), pinned tab {}, the first tab {} still selected {selected}",
        idle.id,
        idle.state().url,
        idle.state().can_go_back,
        typed.id,
        pinned.id,
        first.id
    ));
    if !idle.state().can_go_back || !unsaved || !pinned.is_pinned() || !selected {
        return Err(detail.join("; "));
    }

    browser.sleep_idle_tabs(Instant::now() + LEFT_ALONE);
    until(p, |p| {
        let state = asleep(&idle);
        p.observe(format!(
            "7 h later, tab {} asleep (tab list, engine) {state:?}",
            idle.id
        ));
        (state == (true, true)).then_some(())
    })
    .await;
    exec::sleep(SETTLED).await;
    let awake: Vec<_> = [first, &typed, &pinned]
        .iter()
        .map(|t| (t.id, asleep(t)))
        .collect();
    let look = idle.look();
    let saved = browser.save_session_now();
    let restored = browser
        .core(|c| c.session().restore())
        .map_err(err)?
        .ok_or("the session restored nothing")?;
    let listed = restored
        .windows
        .iter()
        .flat_map(|w| &w.tabs)
        .find(|t| t.id == idle.session_id)
        .map(|t| (t.url.as_str().to_owned(), t.title.clone()));
    detail.push(format!(
        "7 h later tab {} slept, titled {:?}, saved {saved} as {listed:?}; the others (tab list, engine) {awake:?}",
        idle.id, look.title
    ));
    if look.title != PAGE2_TITLE
        || listed != Some((page2.as_str().to_owned(), PAGE2_TITLE.to_owned()))
        || awake.iter().any(|(_, state)| *state != (false, false))
    {
        return Err(detail.join("; "));
    }

    select(window, &idle)?;
    until(p, |p| {
        let state = (asleep(&idle), idle.state());
        p.observe(format!(
            "selected tab {}: asleep (tab list, engine) {:?}, at {} titled {:?}, back {}",
            idle.id, state.0, state.1.url, state.1.title, state.1.can_go_back
        ));
        (state.0 == (false, false)
            && state.1.url == page2.as_str()
            && state.1.title == PAGE2_TITLE
            && state.1.can_go_back)
            .then_some(())
    })
    .await;
    let title = eval(&idle, "document.title").await?;
    detail.push(format!(
        "selected, it woke at {page2}, its page titled {title}, and can go back"
    ));
    if title != format!("{PAGE2_TITLE:?}") {
        return Err(detail.join("; "));
    }

    select(window, first)?;
    browser.sleep_idle_tabs(Instant::now() + LEFT_ALONE);
    window.tab_action(idle.id, TabAction::Pin(true));
    exec::sleep(SETTLED).await;
    let pinned_meanwhile = asleep(&idle);
    window.tab_action(idle.id, TabAction::Pin(false));
    detail.push(format!(
        "pinned while its page answered the sweep, tab {} (tab list, engine) {pinned_meanwhile:?}",
        idle.id
    ));
    if pinned_meanwhile != (false, false) {
        return Err(detail.join("; "));
    }

    browser.write_pref(&keys::MEMORY_SAVER, &false);
    browser.sleep_idle_tabs(Instant::now() + LEFT_ALONE);
    exec::sleep(SETTLED).await;
    let off: Vec<_> = window
        .tabs_in_order()
        .iter()
        .map(|t| (t.id, asleep(t)))
        .collect();
    detail.push(format!(
        "with Memory Saver off, 7 h later (tab list, engine) {off:?}"
    ));
    if off.iter().any(|(_, state)| *state != (false, false)) {
        return Err(detail.join("; "));
    }
    Ok(detail.join("; "))
}
