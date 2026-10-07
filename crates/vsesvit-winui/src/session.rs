//! Session restore and save, as pure functions over vsesvit-core's snapshot types: what to open
//! at startup, and what a window looks like in a snapshot.

use std::time::{SystemTime, UNIX_EPOCH};

use vsesvit_core::Url;
use vsesvit_core::prefs::Startup;
use vsesvit_core::session::{SessionSnapshot, TabId, TabSnapshot, WindowSnapshot};
use vsesvit_core::tab_groups::TabGroup;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A window to open at startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowPlan {
    pub tabs: Vec<TabPlan>,
    pub active: usize,
    pub bounds: Option<(i32, i32, u32, u32)>,
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TabPlan {
    /// `None` opens a blank tab.
    pub url: Option<String>,
    /// Kept for restored tabs, so the saved session keeps naming the same tabs.
    pub id: Option<TabId>,
    /// A restored tab's title, shown until its page reports one; empty otherwise.
    pub title: String,
    pub pinned: bool,
    /// A restored tab's group, as its record carries it.
    pub group: Option<TabGroup>,
}

impl TabPlan {
    pub fn url(url: String) -> Self {
        Self {
            url: Some(url),
            id: None,
            title: String::new(),
            pinned: false,
            group: None,
        }
    }

    pub fn blank() -> Self {
        Self {
            url: None,
            id: None,
            title: String::new(),
            pinned: false,
            group: None,
        }
    }
}

impl WindowPlan {
    pub fn with_tabs(tabs: Vec<TabPlan>) -> Self {
        Self {
            tabs,
            active: 0,
            bounds: None,
            maximized: false,
        }
    }
}

/// What to open at startup: the `startup` preference's choice, with `urls` from the command line
/// added as tabs of the first window (and the first of them selected). Never empty.
pub(crate) fn startup_plan(
    startup: Startup,
    restored: Option<SessionSnapshot>,
    homepage: Option<String>,
    urls: Vec<String>,
) -> Vec<WindowPlan> {
    let mut plan: Vec<WindowPlan> = match startup {
        Startup::RestoreSession => restored
            .map(|s| s.windows.into_iter().filter_map(restore_window).collect())
            .unwrap_or_default(),
        Startup::Homepage => vec![WindowPlan::with_tabs(vec![match homepage {
            Some(url) => TabPlan::url(url),
            None => TabPlan::blank(),
        }])],
        Startup::NewTab => Vec::new(),
    };
    if !urls.is_empty() {
        let tabs = urls.into_iter().map(TabPlan::url);
        match plan.first_mut() {
            Some(first) => {
                first.active = first.tabs.len();
                first.tabs.extend(tabs);
            }
            None => plan.push(WindowPlan::with_tabs(tabs.collect())),
        }
    }
    if plan.is_empty() {
        plan.push(WindowPlan::with_tabs(vec![TabPlan::blank()]));
    }
    plan
}

fn restore_window(window: WindowSnapshot) -> Option<WindowPlan> {
    if window.tabs.is_empty() {
        return None;
    }
    let active = window.active_tab.min(window.tabs.len() - 1);
    let tabs = window
        .tabs
        .into_iter()
        .map(|tab| TabPlan {
            url: (tab.url.as_str() != "about:blank").then(|| tab.url.to_string()),
            id: Some(tab.id),
            title: tab.title,
            pinned: tab.pinned,
            group: tab.group,
        })
        .collect();
    Some(WindowPlan {
        tabs,
        active,
        bounds: window.bounds,
        maximized: window.maximized,
    })
}

/// One tab in a snapshot, with its group if it is in one. A tab that has not committed a URL
/// yet is saved as the blank page.
pub(crate) fn tab_snapshot(
    id: TabId,
    url: &str,
    title: &str,
    pinned: bool,
    last_active_ms: i64,
    group: Option<TabGroup>,
) -> TabSnapshot {
    let url = Url::parse(url).unwrap_or_else(|_| Url::parse("about:blank").expect("a valid URL"));
    TabSnapshot {
        id,
        url,
        title: title.to_owned(),
        pinned,
        last_active_ms,
        group,
        restore_state: None,
    }
}

/// A window with no tabs is not part of a session.
pub(crate) fn window_snapshot(
    tabs: Vec<TabSnapshot>,
    active: Option<usize>,
    bounds: Option<(i32, i32, u32, u32)>,
    maximized: bool,
) -> Option<WindowSnapshot> {
    if tabs.is_empty() {
        return None;
    }
    let active_tab = active.filter(|&i| i < tabs.len()).unwrap_or(0);
    Some(WindowSnapshot {
        tabs,
        active_tab,
        bounds,
        maximized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(windows: Vec<WindowSnapshot>) -> SessionSnapshot {
        SessionSnapshot {
            device_name: "pc".into(),
            windows,
            active_window: 0,
        }
    }

    fn tab(url: &str) -> TabSnapshot {
        tab_snapshot(TabId::new(), url, "t", false, 1, None)
    }

    #[test]
    fn restore_keeps_windows_tabs_ids_and_selection() {
        let a = TabSnapshot {
            pinned: true,
            ..tab("https://a.test/")
        };
        let blank = tab("about:blank");
        let window = window_snapshot(
            vec![a.clone(), blank.clone()],
            Some(1),
            Some((1, 2, 3, 4)),
            true,
        )
        .unwrap();
        let empty = WindowSnapshot {
            tabs: vec![],
            active_tab: 0,
            bounds: None,
            maximized: false,
        };
        let plan = startup_plan(
            Startup::RestoreSession,
            Some(snapshot(vec![window, empty])),
            None,
            vec![],
        );
        assert_eq!(
            plan,
            [WindowPlan {
                tabs: vec![
                    TabPlan {
                        url: Some("https://a.test/".into()),
                        id: Some(a.id),
                        title: "t".into(),
                        pinned: true,
                        group: None,
                    },
                    TabPlan {
                        url: None,
                        id: Some(blank.id),
                        title: "t".into(),
                        pinned: false,
                        group: None,
                    },
                ],
                active: 1,
                bounds: Some((1, 2, 3, 4)),
                maximized: true,
            }]
        );
    }

    #[test]
    fn a_tabs_group_is_saved_with_it_and_restored_into_its_plan() {
        use vsesvit_core::tab_groups::{GroupColor, GroupId};

        let group = TabGroup {
            id: GroupId::new(),
            title: "Work".into(),
            color: GroupColor::Green,
            collapsed: true,
        };
        let grouped = TabSnapshot {
            group: Some(group.clone()),
            ..tab("https://a.test/")
        };
        let window =
            window_snapshot(vec![grouped, tab("https://b.test/")], None, None, false).unwrap();
        let plan = startup_plan(
            Startup::RestoreSession,
            Some(snapshot(vec![window])),
            None,
            vec![],
        );
        assert_eq!(plan[0].tabs[0].group, Some(group));
        assert_eq!(plan[0].tabs[1].group, None);
    }

    #[test]
    fn command_line_urls_join_the_first_window() {
        let window = window_snapshot(vec![tab("https://a.test/")], None, None, false).unwrap();
        let plan = startup_plan(
            Startup::RestoreSession,
            Some(snapshot(vec![window])),
            None,
            vec!["https://b.test/".into()],
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].tabs.len(), 2);
        assert_eq!(plan[0].active, 1);
        assert_eq!(plan[0].tabs[1], TabPlan::url("https://b.test/".into()));
    }

    #[test]
    fn homepage_new_tab_and_nothing_to_restore() {
        let home = startup_plan(
            Startup::Homepage,
            None,
            Some("https://home.test/".into()),
            vec![],
        );
        assert_eq!(home[0].tabs, [TabPlan::url("https://home.test/".into())]);
        let blank_home = startup_plan(Startup::Homepage, None, None, vec![]);
        assert_eq!(blank_home[0].tabs, [TabPlan::blank()]);
        let window = window_snapshot(vec![tab("https://a.test/")], None, None, false).unwrap();
        let new_tab = startup_plan(Startup::NewTab, Some(snapshot(vec![window])), None, vec![]);
        assert_eq!(new_tab, [WindowPlan::with_tabs(vec![TabPlan::blank()])]);
        let first_run = startup_plan(
            Startup::RestoreSession,
            None,
            None,
            vec!["https://c.test/".into()],
        );
        assert_eq!(first_run[0].tabs, [TabPlan::url("https://c.test/".into())]);
    }

    #[test]
    fn snapshots_clamp_selection_and_skip_empty_windows() {
        assert!(window_snapshot(vec![], Some(0), None, false).is_none());
        let w = window_snapshot(vec![tab("https://a.test/")], Some(7), None, false).unwrap();
        assert_eq!(w.active_tab, 0);
        let blank = tab_snapshot(TabId::new(), "", "", false, 0, None);
        assert_eq!(blank.url.as_str(), "about:blank");
    }
}
