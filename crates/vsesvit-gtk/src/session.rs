//! The open windows and tabs as core's session record, and the way back.

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::Url;
use vsesvit_core::prefs::keys;
use vsesvit_core::session::{SessionSnapshot, TabSnapshot, WindowSnapshot};

use crate::browser::Browser;
use crate::window::{BrowserWindow, Focus};

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Every window with at least one committed page, most recently focused first, so the
/// active window is index 0.
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
    for tab in window.tabs() {
        let Some(url) = tab.committed_uri().and_then(|u| Url::parse(&u).ok()) else {
            continue;
        };
        if selected.as_ref() == Some(&tab) {
            active_tab = tabs.len();
        }
        tabs.push(TabSnapshot {
            id: tab.session_id(),
            url,
            title: tab.display_title(),
            pinned: false,
            last_active_ms: tab.last_active_ms(),
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
pub(crate) fn restore(browser: &Browser, snapshot: SessionSnapshot) -> usize {
    let mut opened = 0;
    for saved in snapshot.windows {
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
        for (index, tab_state) in saved.tabs.into_iter().enumerate() {
            let tab = window.open_tab(None, None, Focus::Background);
            tab.set_session_id(tab_state.id);
            tab.mark_active(tab_state.last_active_ms);
            tab.restore_saved(tab_state.restore_state.as_deref(), tab_state.url.as_str());
            if index == saved.active_tab {
                selected = Some(tab);
            }
        }
        if let Some(tab) = selected.or_else(|| window.tabs().into_iter().next()) {
            window.select_tab(&tab);
        }
        window.present();
        opened += 1;
    }
    opened
}
