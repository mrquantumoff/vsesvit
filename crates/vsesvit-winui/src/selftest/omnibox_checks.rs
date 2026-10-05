//! The `address_completion` and `selection_search` checks: Chrome's keys over the address box's list
//! with a visited fixture page to complete inline, and the page context menu's search for the
//! selected text.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use vsesvit_core::search::{EngineForm, SearchEngineId, UrlTemplate};
use vsesvit_core::testkit::FixtureServer;
use windows_core::Interface;

use super::{FIXTURE_TITLE, Probe, eval, tab_ids, until};
use crate::bindings::ICoreWebView2_11;
use crate::browser::Browser;
use crate::exec;
use crate::shortcuts::Mods;
use crate::tab::Tab;
use crate::window::BrowserWindow;

const SETTLE: Duration = Duration::from_millis(300);
const DOWN: u16 = 0x28;
const ESCAPE: u16 = 0x1B;
const ENTER: u16 = 0x0D;
/// Typed into the address box: a prefix of the fixture server's host.
const TYPED: &str = "127.0.0";
/// Selects the fixture page's heading and says where its middle is, in CSS pixels.
const SELECT_HEADING: &str = "(() => { const h = document.querySelector('h1'); \
    const range = document.createRange(); range.selectNodeContents(h); \
    getSelection().removeAllRanges(); getSelection().addRange(range); \
    const r = h.getBoundingClientRect(); \
    return [getSelection().toString(), r.x + r.width / 2, r.y + r.height / 2]; })()";
const SEARCH_ENGINE: &str = "Fixture search";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Runs its closure when dropped: when a check returns, and when its timeout drops it at an
/// `.await` that never completed, where no line after that `.await` runs.
struct OnDrop<F: FnMut()>(F);

impl<F: FnMut()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// What the box and its list show, for the report.
fn describe(window: &BrowserWindow) -> String {
    format!(
        "text {:?} selection {:?} row {:?} open {}",
        window.address_text(),
        window.address_selection(),
        window.highlighted_suggestion(),
        window.suggestions_open()
    )
}

pub(super) async fn address_completion(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    server: &FixtureServer,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let passed = Cell::new(false);
    let _escape = OnDrop(|| {
        if !passed.get() {
            window.address_key_down(ESCAPE, Mods::NONE);
            window.address_key_down(ESCAPE, Mods::NONE);
        }
    });
    let result = keys(window, tab, server, out_dir, p).await;
    passed.set(result.is_ok());
    result
}

async fn keys(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    server: &FixtureServer,
    out_dir: &Path,
    p: &Probe,
) -> Result<String, String> {
    let host = format!("127.0.0.1:{}", server.port());
    let root = server.url("/");
    let mut detail = Vec::new();

    window.type_address(TYPED);
    until(p, |p| {
        p.observe(format!("typed {TYPED:?}: {}", describe(window)));
        (window.highlighted_suggestion() == Some(0)).then_some(())
    })
    .await;
    let fills = window.suggestion_fills();
    let typed = describe(window);
    let completion = Some((TYPED.len(), host.len() - TYPED.len()));
    let shot = window
        .capture()
        .await
        .map_err(|e| format!("capture: {e}"))?;
    let path = out_dir.join("omnibox-inline.png");
    std::fs::write(&path, &shot.png).map_err(|e| format!("{}: {e}", path.display()))?;
    let inline = window.address_text() == host
        && window.address_selection() == completion
        && window.suggestions_open()
        && fills.first() == Some(&host)
        && fills.len() >= 3;
    detail.push(format!("typed {TYPED:?}: {typed}, rows {fills:?}"));
    if !inline {
        return Err(detail.join("; "));
    }

    let handled = window.address_key_down(DOWN, Mods::NONE);
    let down = describe(window);
    let moved = handled
        && window.address_text() == fills[1]
        && window.address_selection() == Some((fills[1].len(), 0))
        && window.highlighted_suggestion() == Some(1);
    detail.push(format!("Down: {down}"));
    if !moved {
        return Err(detail.join("; "));
    }

    // Row 2's text is a URL of its own: had writing it asked for suggestions, the rows and
    // the highlight would be that URL's.
    window.address_key_down(DOWN, Mods::NONE);
    exec::sleep(SETTLE).await;
    let again = describe(window);
    let kept = window.address_text() == fills[2]
        && window.suggestion_fills() == fills
        && window.highlighted_suggestion() == Some(2);
    detail.push(format!("Down again, {SETTLE:?} later: {again}"));
    if !kept {
        return Err(detail.join("; "));
    }

    let handled = window.address_key_down(ESCAPE, Mods::NONE);
    let escape = describe(window);
    let back = handled
        && window.address_text() == host
        && window.address_selection() == completion
        && window.highlighted_suggestion() == Some(0)
        && window.suggestions_open();
    detail.push(format!("Escape: {escape}"));
    if !back {
        return Err(detail.join("; "));
    }

    let handled = window.address_key_down(ENTER, Mods::NONE);
    let opened = until(p, |p| {
        let url = tab.state().url;
        p.observe(format!("after Enter the tab is at {url:?}"));
        (url == root.as_str()).then_some(url)
    })
    .await;
    detail.push(format!(
        "Enter: handled {handled}, tab at {opened}, list open {}",
        window.suggestions_open()
    ));
    let index = server.url("/index.html");
    window.address_submitted(index.as_str());
    until(p, |p| {
        let s = tab.state();
        p.observe(format!(
            "back to the fixture page: at {:?} loading {}",
            s.url,
            s.loading()
        ));
        (s.url == index.as_str() && s.title == FIXTURE_TITLE && !s.loading()).then_some(())
    })
    .await;
    let ok = handled && !window.suggestions_open();
    let detail = detail.join("; ");
    ok.then_some(detail.clone()).ok_or(detail)
}

/// With a default search engine on the fixture server, so nothing leaves the machine. The
/// previous default comes back, and a tab the Search item opened closes, also on a timeout.
pub(super) async fn selection_search(
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let template = UrlTemplate(format!(
        "http://127.0.0.1:{}/search?q={{searchTerms}}",
        server.port()
    ));
    let (engine, previous) = browser
        .core(|c| {
            let previous = c.search_engines().default_engine()?.id;
            let form = EngineForm {
                name: SEARCH_ENGINE.to_owned(),
                keyword: "fixture".to_owned(),
                url: template.0.clone(),
            };
            let engine = c.search_engines().save(None, &form)?;
            c.search_engines().set_default(&engine)?;
            Ok::<_, vsesvit_core::Error>((engine, previous))
        })
        .map_err(err)?;
    let open = tab_ids(window);
    let _restore = OnDrop(|| {
        restore_engine(browser, &engine, &previous);
        for t in window.tabs_in_order() {
            if !open.contains(&t.id) {
                window.close_tab(t.id);
            }
        }
    });
    search_selection(window, tab, &template, server, p).await
}

fn restore_engine(browser: &Browser, engine: &SearchEngineId, previous: &SearchEngineId) {
    let restored = browser.core(|c| {
        c.search_engines().set_default(previous)?;
        c.search_engines().remove(engine)
    });
    if let Err(e) = restored {
        log::warn!("self-test: restoring the default search engine: {e}");
    }
}

/// What the menu held when it opened: its item names, and the label of the item after Copy.
#[derive(Debug, Default)]
struct Menu {
    names: Vec<String>,
    after_copy: Option<String>,
}

async fn search_selection(
    window: &Rc<BrowserWindow>,
    tab: &Rc<Tab>,
    template: &UrlTemplate,
    server: &FixtureServer,
    p: &Probe,
) -> Result<String, String> {
    let selected: serde_json::Value =
        serde_json::from_str(&eval(tab, SELECT_HEADING).await?).map_err(err)?;
    let text = selected[0].as_str().unwrap_or_default().to_owned();
    let (x, y) = (selected[1].as_f64(), selected[2].as_f64());
    let (Some(x), Some(y)) = (x, y) else {
        return Err(format!("selecting the heading gave {selected}"));
    };
    let expected_url = template.expand(&text).ok_or("the engine template")?;
    let expected_label = format!("Search {SEARCH_ENGINE} for \u{201c}{text}\u{201d}");

    // Runs after the tab's own handler, which added the item. Choosing it here is what a
    // click on it does, and keeps WebView2's menu from showing on the user's screen.
    let menu = Rc::new(RefCell::new(None::<Menu>));
    let seen = menu.clone();
    let core = tab.core().ok_or("no engine view")?;
    let _watch = core
        .cast::<ICoreWebView2_11>()
        .and_then(|core| {
            core.ContextMenuRequested(move |_, args| {
                let Some(args) = args.as_ref() else { return };
                let mut menu = Menu::default();
                if let Ok(items) = args.MenuItems() {
                    let items: Vec<_> = items.into_iter().collect();
                    menu.names = items.iter().map(|i| i.Name().unwrap_or_default()).collect();
                    let after_copy = menu
                        .names
                        .iter()
                        .position(|n| n == "copy")
                        .and_then(|i| items.get(i + 1));
                    if let Some(item) = after_copy {
                        menu.after_copy = item.Label().ok();
                        if let Ok(id) = item.CommandId() {
                            let _ = args.SetSelectedCommandId(id);
                        }
                    }
                }
                let _ = args.SetHandled(true);
                *seen.borrow_mut() = Some(menu);
            })
        })
        .map_err(err)?;
    let before = window.tab_count();
    for kind in ["mousePressed", "mouseReleased"] {
        let params = json!({
            "type": kind, "x": x, "y": y, "button": "right", "buttons": 2, "clickCount": 1,
        });
        tab.devtools("Input.dispatchMouseEvent", &params.to_string())
            .await
            .map_err(|e| format!("right click: {e}"))?;
    }
    let menu = until(p, |p| {
        p.observe(format!(
            "right-clicked {text:?} at {x},{y}; no context menu yet"
        ));
        menu.borrow_mut().take()
    })
    .await;
    let opened = until(p, |p| {
        let tabs = window.tabs_in_order();
        p.observe(format!(
            "menu {menu:?}; {} tabs, urls {:?}",
            tabs.len(),
            tabs.iter().map(|t| t.state().url).collect::<Vec<_>>()
        ));
        let index = tabs
            .iter()
            .position(|t| t.state().url == expected_url.as_str())?;
        Some((
            index,
            tabs[index].clone(),
            tabs.iter().position(|t| t.id == tab.id),
        ))
    })
    .await;
    let (index, new_tab, opener_index) = opened;
    let foreground = window.active_tab().is_some_and(|t| t.id == new_tab.id);
    let requested = server.hits().iter().any(|h| h == "/search");
    window.close_tab(new_tab.id);
    let detail = format!(
        "selected {text:?}; menu {:?}; item after copy {:?}; new tab {} at {index} after the \
         page at {opener_index:?}, foreground {foreground}; server saw /search {requested}; \
         tabs {before} -> {}",
        menu.names,
        menu.after_copy,
        expected_url,
        window.tab_count()
    );
    let ok = text == "Vsesvit fixture page"
        && menu.after_copy.as_deref() == Some(expected_label.as_str())
        && opener_index.map(|i| i + 1) == Some(index)
        && foreground
        && requested
        && window.active_tab().is_some_and(|t| t.id == tab.id);
    ok.then_some(detail.clone()).ok_or(detail)
}
