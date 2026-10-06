//! The `tab_search` check: the popup Ctrl+Shift+A opens lists the open tabs of both windows and
//! the closed ones for what is typed, ranked as core ranks them; it switches to a tab in the
//! other window and reopens a closed tab, which leaves the closed ones. The box is filled and the
//! keys are given to the popup directly, without OS input.

use std::rc::Rc;

use vsesvit_core::private::Browsing;
use vsesvit_core::testkit::FixtureServer;

use super::{FIXTURE_TITLE, PAGE2_TITLE, Probe, load, until};
use crate::browser::Browser;
use crate::session::{TabPlan, WindowPlan};
use crate::shortcuts::Command;
use crate::tab::Tab;
use crate::window::{BrowserWindow, TabSearch};

/// A page whose title starts with the query the check types, which ranks it first.
const LISTED: &str = "data:text/html,<title>Fixture for tab search</title>";
const LISTED_TITLE: &str = "Fixture for tab search";

const VK_RETURN: i32 = 0x0D;
const VK_ESCAPE: i32 = 0x1B;
const VK_UP: i32 = 0x26;
const VK_DOWN: i32 = 0x28;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Waits until `tab` has `title` and is done loading.
async fn titled(tab: &Tab, title: &str, p: &Probe) {
    until(p, |p| {
        let s = tab.state();
        p.observe(format!(
            "tab {} titled {:?} at {}, loading={}; want {title:?}",
            tab.id,
            s.title,
            s.url,
            s.loading()
        ));
        (s.title == title && !s.loading()).then_some(())
    })
    .await;
}

/// Runs the Search tabs command in `window` and waits for its popup.
async fn open(window: &BrowserWindow, p: &Probe) -> Rc<TabSearch> {
    window.run(Command::SearchTabs);
    until(p, |p| {
        p.observe("Search tabs: no popup yet");
        window.tab_search()
    })
    .await
}

/// Waits until `window`'s tab search has closed.
async fn closed(window: &BrowserWindow, p: &Probe) {
    until(p, |p| {
        p.observe("tab search still open");
        window.tab_search().is_none().then_some(())
    })
    .await;
}

pub(super) async fn tab_search(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    first: &Rc<Tab>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let mut detail = Vec::new();
    let index = server.url("/index.html");
    let page2 = server.url("/page2.html");
    let host = index.host_str().unwrap_or_default().to_owned();
    load(first, index.as_str(), p).await;
    let plan = WindowPlan::with_tabs(vec![TabPlan::url(page2.to_string()), TabPlan::url(LISTED.to_owned())]);
    let other = browser.open_window(Browsing::Normal, &plan, browser.show_mode()).map_err(err)?;
    let [second, listed] = until(p, |p| {
        let tabs = other.tabs_in_order();
        p.observe(format!("the other window has {} tabs", tabs.len()));
        <[Rc<Tab>; 2]>::try_from(tabs).ok()
    })
    .await;
    titled(&second, PAGE2_TITLE, p).await;
    titled(&listed, LISTED_TITLE, p).await;

    let search = open(window, p).await;
    search.set_query("fixture").map_err(err)?;
    let seen = search.lines();
    let listed_url = listed.session_url();
    let want = [
        "Open tabs".to_owned(),
        format!("> {LISTED_TITLE} | {listed_url}"),
        format!("- {PAGE2_TITLE} | {host}"),
        format!("- {FIXTURE_TITLE} | {host}"),
    ];
    detail.push(format!("\"fixture\" lists {seen:?}"));
    let closed_after = seen.get(want.len()).is_none_or(|l| l == "Recently closed");
    if !seen.starts_with(&want) || !closed_after {
        search.close();
        return Err(detail.join("; "));
    }

    search.key(VK_DOWN);
    let down = search.lines().get(2).cloned().unwrap_or_default();
    search.key(VK_UP);
    let up = search.lines().get(1).cloned().unwrap_or_default();
    detail.push(format!("Down selects {down:?}, Up {up:?}"));
    if !down.starts_with("> ") || !up.starts_with("> ") {
        search.close();
        return Err(detail.join("; "));
    }
    search.key(VK_RETURN);
    until(p, |p| {
        let selected = other.active_tab().map(|t| t.id);
        p.observe(format!(
            "Enter on {LISTED_TITLE:?}: the other window selects {selected:?}, want {}",
            listed.id
        ));
        (selected == Some(listed.id)).then_some(())
    })
    .await;
    closed(window, p).await;
    detail.push("Enter switched to it in the other window and closed the popup".to_owned());

    other.close_tab(listed.id);
    let search = open(window, p).await;
    search.set_query("for tab search").map_err(err)?;
    let seen = search.lines();
    detail.push(format!("closed, \"for tab search\" lists {seen:?}"));
    if seen != ["Recently closed".to_owned(), format!("> {LISTED_TITLE} | {listed_url}")] {
        search.close();
        return Err(detail.join("; "));
    }
    let before: Vec<u64> = window.tabs_in_order().iter().map(|t| t.id).collect();
    search.key(VK_RETURN);
    let reopened = until(p, |p| {
        p.observe("Enter on the closed tab: nothing reopened yet");
        window.tabs_in_order().into_iter().find(|t| !before.contains(&t.id))
    })
    .await;
    titled(&reopened, LISTED_TITLE, p).await;
    closed(window, p).await;
    let selected = window.active_tab().is_some_and(|t| t.id == reopened.id);

    let search = open(window, p).await;
    search.set_query("for tab search").map_err(err)?;
    let again = search.lines();
    search.set_query("no tab is called this").map_err(err)?;
    let none = search.lines();
    search.key(VK_ESCAPE);
    closed(window, p).await;
    detail.push(format!(
        "Enter reopened it here, selected {selected}; then it lists {again:?}, and an unmatched query {none:?}; Escape closed the popup"
    ));
    let open_now = [
        "Open tabs".to_owned(),
        format!("> {LISTED_TITLE} | {}", reopened.session_url()),
    ];
    if !selected || again != open_now || none != ["No results found"] {
        return Err(detail.join("; "));
    }
    Ok(detail.join("; "))
}
