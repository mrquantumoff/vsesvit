//! The open windows and tabs as core's session record, and the way back.

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::Url;
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::session::{SessionSnapshot, TabSnapshot, WindowSnapshot};

use crate::browser::Browser;
use crate::window::{BrowserWindow, Focus};

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Every window with at least one page, committed or still loading, most recently focused
/// first, so the active window is index 0. Private tabs, and so private windows, are never
/// saved.
pub(crate) fn snapshot(browser: &Browser) -> SessionSnapshot {
    let windows = browser
        .windows()
        .iter()
        .filter_map(window_snapshot)
        .collect();
    SessionSnapshot {
        device_name: device_name(browser),
        windows,
        active_window: 0,
    }
}

fn window_snapshot(window: &BrowserWindow) -> Option<WindowSnapshot> {
    let selected = window.selected_tab();
    let mut tabs = Vec::new();
    let mut active_tab = 0;
    for tab in window.tabs().into_iter().filter(|tab| tab.browsing() == Browsing::Normal) {
        let Some(url) = tab.session_uri().and_then(|u| Url::parse(&u).ok()) else {
            continue;
        };
        if selected.as_ref() == Some(&tab) {
            active_tab = tabs.len();
        }
        tabs.push(TabSnapshot {
            id: tab.session_id(),
            url,
            title: tab.display_title(),
            pinned: window.is_pinned(&tab),
            last_active_ms: tab.last_active_ms(),
            group: window.group_of(&tab),
            restore_state: tab.session_state_bytes(),
        });
    }
    if tabs.is_empty() {
        return None;
    }
    let (width, height) = window.default_size();
    Some(WindowSnapshot {
        tabs,
        active_tab,
        bounds: Some((0, 0, width.max(0).cast_unsigned(), height.max(0).cast_unsigned())),
        maximized: window.is_maximized(),
    })
}

fn device_name(browser: &Browser) -> String {
    let configured = browser.core().borrow_mut().prefs().get(&keys::DEVICE_NAME);
    if configured.trim().is_empty() {
        glib::host_name().to_string()
    } else {
        configured
    }
}

/// Recreates the snapshot's windows and tabs. Returns how many windows were opened.
///
/// The active window is created and presented last, so that it is on top and first in
/// [`Browser::windows`], where command-line URLs go and the next save puts it.
pub(crate) fn restore(browser: &Browser, snapshot: SessionSnapshot) -> usize {
    let mut windows = snapshot.windows;
    if snapshot.active_window < windows.len() {
        let active = windows.remove(snapshot.active_window);
        windows.insert(0, active);
    }
    let mut opened = 0;
    for saved in windows.into_iter().rev() {
        if saved.tabs.is_empty() {
            continue;
        }
        let window = BrowserWindow::new(browser);
        if let Some((_, _, width, height)) = saved.bounds
            && width > 0
            && height > 0
        {
            window.set_default_size(width.cast_signed(), height.cast_signed());
        }
        if saved.maximized {
            window.maximize();
        }
        let mut selected = None;
        let mut groups = Vec::with_capacity(saved.tabs.len());
        for (index, tab_state) in saved.tabs.into_iter().enumerate() {
            let tab = window.open_tab(None, None, Focus::Background);
            window.set_pinned(&tab, tab_state.pinned);
            tab.set_session_id(tab_state.id);
            tab.mark_active(tab_state.last_active_ms);
            tab.restore_saved(tab_state.restore_state.as_deref(), tab_state.url.as_str());
            groups.push((tab.id(), tab_state.group));
            if index == saved.active_tab {
                selected = Some(tab);
            }
        }
        if let Some(tab) = selected.or_else(|| window.tabs().into_iter().next()) {
            window.select_tab(&tab);
        }
        // After the selection: the view selected the first tab meanwhile, and selecting a tab
        // expands its group.
        window.restore_groups(groups);
        window.present();
        opened += 1;
    }
    opened
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use webkit::prelude::*;

    use super::*;
    use crate::test_support::{Reply, Server, browser, wait_until};

    #[gtk::test]
    fn a_tab_still_loading_its_first_page_is_saved() {
        let server = Server::start("127.0.0.1", |_| Reply::Hang);
        let window = BrowserWindow::new(&browser());
        let url = server.url("/slow");
        let tab = window.open_tab(Some(&url), None, Focus::Background);
        wait_until("the load to start", || tab.web_view().is_loading());
        let saved = window_snapshot(&window);
        window.destroy();
        let urls: Vec<String> = saved
            .into_iter()
            .flat_map(|w| w.tabs)
            .map(|t| t.url.to_string())
            .collect();
        assert_eq!(urls, [url]);
    }

    #[gtk::test]
    fn a_restored_tab_keeps_its_back_forward_state_until_it_commits() {
        let stalled = Arc::new(AtomicBool::new(false));
        let server = Server::start("127.0.0.1", {
            let stalled = stalled.clone();
            move |path| match path {
                "/a" => Reply::Page("A"),
                "/b" if stalled.load(Ordering::SeqCst) => Reply::Hang,
                "/b" => Reply::Page("B"),
                _ => Reply::NotFound,
            }
        });
        let (a, b) = (server.url("/a"), server.url("/b"));
        let window = BrowserWindow::new(&browser());
        let visited = window.open_tab(Some(&a), None, Focus::Background);
        wait_until("A to commit", || visited.committed_uri().as_deref() == Some(a.as_str()));
        visited.load(&b);
        wait_until("B to commit", || visited.committed_uri().as_deref() == Some(b.as_str()));
        let state = visited.session_state_bytes().expect("the engine's back/forward state");

        stalled.store(true, Ordering::SeqCst);
        let restored = window.open_tab(None, None, Focus::Background);
        restored.restore_saved(Some(&state), &b);
        wait_until("the restore to start", || restored.web_view().is_loading());
        let saved = window_snapshot(&window);
        window.destroy();
        let saved = saved.expect("the window is saved");
        let tab = saved
            .tabs
            .iter()
            .find(|t| t.id == restored.session_id())
            .expect("the restored tab is saved");
        assert_eq!(tab.url.as_str(), b);
        assert!(tab.restore_state.is_some(), "its back/forward state was dropped");
    }

    #[gtk::test]
    fn pinned_tabs_are_restored_pinned_and_saved_again() {
        let server = Server::start("127.0.0.1", |_| Reply::Page("Pinned"));
        let browser = browser();
        let saved = |path: &str, pinned| TabSnapshot {
            id: vsesvit_core::session::TabId::new(),
            url: Url::parse(&server.url(path)).unwrap(),
            title: String::new(),
            pinned,
            last_active_ms: 0,
            group: None,
            restore_state: None,
        };
        restore(
            &browser,
            SessionSnapshot {
                device_name: String::new(),
                windows: vec![WindowSnapshot {
                    tabs: vec![saved("/a", true), saved("/b", false)],
                    active_tab: 1,
                    bounds: None,
                    maximized: false,
                }],
                active_window: 0,
            },
        );
        let window = browser.windows()[0].clone();
        let shown: Vec<bool> = window.tabs().iter().map(|tab| window.is_pinned(tab)).collect();
        let resaved = window_snapshot(&window).expect("the window is saved");
        window.destroy();
        assert_eq!(shown, [true, false]);
        let pins: Vec<(String, bool)> = resaved.tabs.iter().map(|t| (t.url.path().to_owned(), t.pinned)).collect();
        assert_eq!(pins, [("/a".to_owned(), true), ("/b".to_owned(), false)]);
    }

    #[gtk::test]
    fn grouped_tabs_are_restored_in_their_groups_and_saved_again() {
        use vsesvit_core::tab_groups::{GroupColor, GroupId, TabGroup};

        let server = Server::start("127.0.0.1", |_| Reply::Page("Grouped"));
        let browser = browser();
        let work = TabGroup { id: GroupId::new(), title: "Work".into(), color: GroupColor::Blue, collapsed: true };
        let saved = |path: &str, group: Option<&TabGroup>| TabSnapshot {
            id: vsesvit_core::session::TabId::new(),
            url: Url::parse(&server.url(path)).unwrap(),
            title: String::new(),
            pinned: false,
            last_active_ms: 0,
            group: group.cloned(),
            restore_state: None,
        };
        restore(
            &browser,
            SessionSnapshot {
                device_name: String::new(),
                windows: vec![WindowSnapshot {
                    tabs: vec![saved("/a", Some(&work)), saved("/b", Some(&work)), saved("/c", None)],
                    active_tab: 2,
                    bounds: None,
                    maximized: false,
                }],
                active_window: 0,
            },
        );
        let window = browser.windows()[0].clone();
        let shown: Vec<(Option<TabGroup>, Option<bool>)> =
            window.tabs().iter().map(|tab| (window.group_of(tab), window.tab_row_hidden(tab))).collect();
        let resaved = window_snapshot(&window).expect("the window is saved");
        window.destroy();
        assert_eq!(shown, [(Some(work.clone()), Some(true)), (Some(work.clone()), Some(true)), (None, Some(false))]);
        let groups: Vec<Option<TabGroup>> = resaved.tabs.into_iter().map(|t| t.group).collect();
        assert_eq!(groups, [Some(work.clone()), Some(work), None]);
    }

    #[gtk::test]
    fn a_restored_session_keeps_its_active_window_first() {
        let server = Server::start("127.0.0.1", |_| Reply::Page("Restored"));
        let browser = browser();
        let saved = |path: &str| WindowSnapshot {
            tabs: vec![TabSnapshot {
                id: vsesvit_core::session::TabId::new(),
                url: Url::parse(&server.url(path)).unwrap(),
                title: String::new(),
                pinned: false,
                last_active_ms: 0,
                group: None,
                restore_state: None,
            }],
            active_tab: 0,
            bounds: None,
            maximized: false,
        };
        let opened = restore(
            &browser,
            SessionSnapshot {
                device_name: String::new(),
                windows: vec![saved("/a"), saved("/b")],
                active_window: 0,
            },
        );
        let first = browser.windows()[0].tabs()[0].session_uri();
        let resaved = snapshot(&browser);
        for window in browser.windows() {
            window.destroy();
        }
        assert_eq!(opened, 2);
        assert_eq!(first, Some(server.url("/a")), "the active window comes first");
        let order: Vec<String> = resaved.windows.iter().map(|w| w.tabs[0].url.to_string()).collect();
        assert_eq!(order, [server.url("/a"), server.url("/b")], "a save keeps the order");
    }
}
