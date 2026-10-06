//! Browser windows as the runtime sees them. The shell owns the real windows and answers
//! through [`crate::TabHost`]; these types are what crosses that boundary, what
//! `chrome.windows` shows to extensions, and the events a change between two looks at the
//! windows fires ([`changes`]).

use serde::Serialize;
use serde_json::{Value, json};
use vsesvit_core::private::Browsing;

use crate::tabs::TabId;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WindowId(pub u32);

/// `chrome.windows.WINDOW_ID_NONE`: what `onFocusChanged` reports when no browser window
/// has the focus.
pub const WINDOW_ID_NONE: i64 = -1;
/// `chrome.windows.WINDOW_ID_CURRENT`.
pub const WINDOW_ID_CURRENT: i64 = -2;

/// Every window the shell has is a tabbed browser window, Chrome's `normal` type.
pub const WINDOW_TYPE: &str = "normal";

/// Chrome's error for a state that contradicts the focus or bounds asked for with it.
pub const INVALID_STATE: &str = "Invalid value for state";
pub const NO_CURRENT_WINDOW: &str = "No current window";
pub const NO_LAST_FOCUSED_WINDOW: &str = "No last-focused window";
/// Chrome's error for moving a tab into a window of the other kind, normal or private.
pub const ONLY_SAME_PROFILE: &str = "Tabs can only be moved between windows in the same profile.";

/// Chrome's error for a private window whose every URL is one a private window may not open.
pub fn not_in_private(url: &str) -> String {
    format!("Cannot open URL \"{url}\" in an incognito window.")
}

pub fn not_found(id: i64) -> String {
    format!("No window with id: {id}.")
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

impl WindowState {
    pub const fn name(self) -> &'static str {
        match self {
            WindowState::Normal => "normal",
            WindowState::Minimized => "minimized",
            WindowState::Maximized => "maximized",
            WindowState::Fullscreen => "fullscreen",
        }
    }

    /// `None` for an absent state; `locked-fullscreen` is ChromeOS's, for its own apps.
    fn parse(v: &Value) -> Result<Option<WindowState>, String> {
        let state = match v {
            Value::Null => return Ok(None),
            Value::String(s) => s.as_str(),
            _ => return Err("state must be a string".into()),
        };
        Ok(Some(match state {
            "normal" => WindowState::Normal,
            "minimized" => WindowState::Minimized,
            "maximized" => WindowState::Maximized,
            "fullscreen" => WindowState::Fullscreen,
            other => return Err(format!("Invalid enumeration value \"{other}\" for state")),
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub focused: bool,
    /// Its kind, for its whole life: `incognito` when private.
    pub browsing: Browsing,
    pub state: WindowState,
    pub width: u32,
    pub height: u32,
}

impl WindowInfo {
    /// `chrome.windows.Window`, with `tabs` when the caller asked to populate it. Wayland
    /// does not tell a window where it is, so it is always at the origin.
    pub fn to_json(&self, tabs: Option<Vec<Value>>) -> Value {
        let mut window = json!({
            "id": self.id,
            "focused": self.focused,
            "incognito": self.browsing == Browsing::Private,
            "type": WINDOW_TYPE,
            "state": self.state.name(),
            "alwaysOnTop": false,
            "left": 0,
            "top": 0,
            "width": self.width,
            "height": self.height,
        });
        if let Some(tabs) = tabs {
            window["tabs"] = Value::Array(tabs);
        }
        window
    }
}

/// Which window `WINDOW_ID_CURRENT`, `currentWindow` and `lastFocusedWindow` mean for a
/// caller: the one holding its tab, or else the last focused, as in Chrome.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowScope {
    pub current: Option<WindowId>,
    pub last_focused: Option<WindowId>,
}

impl WindowScope {
    /// A `windowId` argument: `WINDOW_ID_CURRENT` resolved, `None` for no such window.
    pub fn resolve(&self, id: i64) -> Option<WindowId> {
        if id == WINDOW_ID_CURRENT {
            return self.current;
        }
        u32::try_from(id).ok().map(WindowId)
    }
}

/// `windows.get/getCurrent/getLastFocused/getAll`'s `queryOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowQuery {
    pub populate: bool,
    types: Option<Vec<String>>,
}

impl WindowQuery {
    pub fn parse(v: &Value) -> WindowQuery {
        WindowQuery {
            populate: v["populate"].as_bool().unwrap_or(false),
            types: v["windowTypes"].as_array().map(|types| types.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
        }
    }

    /// Chrome's default filter takes `normal` windows, as does any list naming them.
    pub fn admits(&self, _window: &WindowInfo) -> bool {
        self.types.as_ref().is_none_or(|types| types.iter().any(|t| t == WINDOW_TYPE))
    }
}

/// `windows.create(createData)`, checked as Chrome checks it. A `popup` or `panel` type
/// opens a normal window, the only kind the shell has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewWindow {
    /// Tabs to open, the first one selected. As the extension wrote them until the
    /// runtime resolves them against the calling page.
    pub urls: Vec<String>,
    /// A tab to move into the window, ahead of `urls`.
    pub tab: Option<TabId>,
    pub focused: bool,
    /// Private for `incognito: true`.
    pub browsing: Browsing,
    pub state: WindowState,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl NewWindow {
    pub fn parse(v: &Value) -> Result<NewWindow, String> {
        let urls = match &v["url"] {
            Value::Null => Vec::new(),
            Value::String(url) => vec![url.clone()],
            Value::Array(urls) => urls.iter().map(|u| u.as_str().map(str::to_owned).ok_or("url must be a string or an array of strings")).collect::<Result<_, _>>()?,
            _ => return Err("url must be a string or an array of strings".into()),
        };
        let tab = match &v["tabId"] {
            Value::Null => None,
            id => Some(TabId::from_json(id).ok_or("tabId must be an integer")?),
        };
        if let Some(kind) = v["type"].as_str()
            && !["normal", "popup", "panel"].contains(&kind)
        {
            return Err(format!("Invalid enumeration value \"{kind}\" for type"));
        }
        let state = WindowState::parse(&v["state"])?;
        let focused = optional_bool(v, "focused")?;
        check_state(state, focused, has_bounds(v))?;
        let state = state.unwrap_or_default();
        Ok(NewWindow {
            urls,
            tab,
            // Minimized starts unfocused, as in Chrome; everything else focused.
            focused: focused.unwrap_or(state != WindowState::Minimized),
            browsing: if optional_bool(v, "incognito")? == Some(true) { Browsing::Private } else { Browsing::Normal },
            state,
            width: dimension(v, "width")?,
            height: dimension(v, "height")?,
        })
    }
}

/// `windows.update(windowId, updateInfo)`, checked as Chrome checks it. Unfocusing a window
/// is left to the user (Wayland has no way to), and `drawAttention` does nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowUpdate {
    pub focused: Option<bool>,
    pub state: Option<WindowState>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl WindowUpdate {
    pub fn parse(v: &Value) -> Result<WindowUpdate, String> {
        let state = WindowState::parse(&v["state"])?;
        let focused = optional_bool(v, "focused")?;
        check_state(state, focused, has_bounds(v))?;
        Ok(WindowUpdate { focused, state, width: dimension(v, "width")?, height: dimension(v, "height")? })
    }
}

/// A window cannot be both focused and minimized, nor unfocused and maximized or full
/// screen, nor sized while in any of those states.
fn check_state(state: Option<WindowState>, focused: Option<bool>, bounds: bool) -> Result<(), String> {
    let valid = match state {
        Some(WindowState::Minimized) => focused != Some(true) && !bounds,
        Some(WindowState::Maximized | WindowState::Fullscreen) => focused != Some(false) && !bounds,
        Some(WindowState::Normal) | None => true,
    };
    if valid { Ok(()) } else { Err(INVALID_STATE.into()) }
}

fn has_bounds(v: &Value) -> bool {
    ["left", "top", "width", "height"].iter().any(|k| !v[*k].is_null())
}

fn optional_bool(v: &Value, key: &str) -> Result<Option<bool>, String> {
    match &v[key] {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        _ => Err(format!("{key} must be a boolean")),
    }
}

fn dimension(v: &Value, key: &str) -> Result<Option<u32>, String> {
    match &v[key] {
        Value::Null => Ok(None),
        n => n.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| format!("{key} must be a non-negative integer")),
    }
}

/// A change `chrome.windows` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Created(WindowInfo),
    Removed(WindowId),
    BoundsChanged(WindowInfo),
    /// `None` when focus left every browser window.
    FocusChanged(Option<WindowId>),
}

impl WindowEvent {
    pub const fn name(&self) -> &'static str {
        match self {
            WindowEvent::Created(_) => "windows.onCreated",
            WindowEvent::Removed(_) => "windows.onRemoved",
            WindowEvent::BoundsChanged(_) => "windows.onBoundsChanged",
            WindowEvent::FocusChanged(_) => "windows.onFocusChanged",
        }
    }

    pub fn args(&self) -> Vec<Value> {
        vec![match self {
            WindowEvent::Created(window) | WindowEvent::BoundsChanged(window) => window.to_json(None),
            WindowEvent::Removed(id) => json!(id),
            WindowEvent::FocusChanged(id) => id.map_or(json!(WINDOW_ID_NONE), |id| json!(id)),
        }]
    }
}

/// What extensions hear about the windows going from `before` to `after`: windows opened,
/// then closed, then resized, then where the focus went. Focus moving straight from one
/// window to another may show as two changes, through `WINDOW_ID_NONE`, as Chrome warns it
/// does on Linux.
pub fn changes(before: &[WindowInfo], after: &[WindowInfo]) -> Vec<WindowEvent> {
    let find = |windows: &[WindowInfo], id: WindowId| windows.iter().find(|w| w.id == id).cloned();
    let mut events: Vec<WindowEvent> = after.iter().filter(|w| find(before, w.id).is_none()).cloned().map(WindowEvent::Created).collect();
    events.extend(before.iter().filter(|w| find(after, w.id).is_none()).map(|w| WindowEvent::Removed(w.id)));
    events.extend(
        after
            .iter()
            .filter(|w| find(before, w.id).is_some_and(|old| (old.width, old.height) != (w.width, w.height)))
            .cloned()
            .map(WindowEvent::BoundsChanged),
    );
    let focused = |windows: &[WindowInfo]| windows.iter().find(|w| w.focused).map(|w| w.id);
    if focused(before) != focused(after) {
        events.push(WindowEvent::FocusChanged(focused(after)));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, focused: bool) -> WindowInfo {
        WindowInfo { id: WindowId(id), focused, browsing: Browsing::Normal, state: WindowState::Normal, width: 1280, height: 820 }
    }

    #[test]
    fn window_json_is_chromes() {
        let v = window(3, true).to_json(Some(vec![json!({"id": 7})]));
        assert_eq!(
            v,
            json!({
                "id": 3, "focused": true, "incognito": false, "type": "normal", "state": "normal",
                "alwaysOnTop": false, "left": 0, "top": 0, "width": 1280, "height": 820, "tabs": [{"id": 7}],
            })
        );
        assert!(window(3, false).to_json(None).get("tabs").is_none());
        assert_eq!(WindowInfo { browsing: Browsing::Private, ..window(3, true) }.to_json(None)["incognito"], true);
    }

    #[test]
    fn window_ids_resolve_against_the_callers_scope() {
        let scope = WindowScope { current: Some(WindowId(2)), last_focused: Some(WindowId(1)) };
        assert_eq!(scope.resolve(WINDOW_ID_CURRENT), Some(WindowId(2)));
        assert_eq!(scope.resolve(5), Some(WindowId(5)));
        assert_eq!(scope.resolve(WINDOW_ID_NONE), None);
        assert_eq!(WindowScope::default().resolve(WINDOW_ID_CURRENT), None);
        assert_eq!(not_found(-1), "No window with id: -1.");
    }

    #[test]
    fn queries_admit_normal_windows() {
        let w = window(1, true);
        assert!(WindowQuery::parse(&Value::Null).admits(&w));
        assert!(WindowQuery::parse(&json!({"windowTypes": ["popup", "normal"]})).admits(&w));
        assert!(!WindowQuery::parse(&json!({"windowTypes": ["popup", "devtools"]})).admits(&w));
        assert!(WindowQuery::parse(&json!({"populate": true})).populate);
        assert!(!WindowQuery::parse(&json!({})).populate);
    }

    #[test]
    fn create_data_defaults_and_parses() {
        assert_eq!(NewWindow::parse(&json!({})).unwrap(), NewWindow { focused: true, ..NewWindow::default() });
        let w = NewWindow::parse(&json!({"url": ["a.html", "https://x.test/"], "tabId": 4, "type": "popup", "width": 400, "height": 300, "focused": false})).unwrap();
        assert_eq!(w.urls, ["a.html", "https://x.test/"]);
        assert_eq!((w.tab, w.focused, w.width, w.height), (Some(TabId(4)), false, Some(400), Some(300)));
        assert_eq!(NewWindow::parse(&json!({"url": "a.html"})).unwrap().urls, ["a.html"]);
        assert_eq!(NewWindow::parse(&json!({"incognito": true})).unwrap().browsing, Browsing::Private);
        assert_eq!(NewWindow::parse(&json!({"incognito": false})).unwrap().browsing, Browsing::Normal);
        let minimized = NewWindow::parse(&json!({"state": "minimized"})).unwrap();
        assert_eq!((minimized.state, minimized.focused), (WindowState::Minimized, false));
        assert_eq!(NewWindow::parse(&json!({"type": "devtools"})).unwrap_err(), "Invalid enumeration value \"devtools\" for type");
        assert_eq!(NewWindow::parse(&json!({"state": "locked-fullscreen"})).unwrap_err(), "Invalid enumeration value \"locked-fullscreen\" for state");
        assert!(NewWindow::parse(&json!({"url": 3})).is_err());
        assert!(NewWindow::parse(&json!({"width": -1})).is_err());
    }

    /// The state checks of Chrome's `IsValidStateForWindowsCreateFunction` and
    /// `WindowsUpdateFunction`.
    #[test]
    fn states_that_contradict_focus_or_bounds_are_refused() {
        for bad in [
            json!({"state": "minimized", "focused": true}),
            json!({"state": "maximized", "focused": false}),
            json!({"state": "fullscreen", "focused": false}),
            json!({"state": "maximized", "width": 300}),
            json!({"state": "minimized", "left": 0}),
        ] {
            assert_eq!(NewWindow::parse(&bad).unwrap_err(), INVALID_STATE, "{bad}");
            assert_eq!(WindowUpdate::parse(&bad).unwrap_err(), INVALID_STATE, "{bad}");
        }
        for good in [json!({"state": "normal", "width": 300, "focused": false}), json!({"state": "maximized", "focused": true}), json!({"state": "minimized"})] {
            assert!(NewWindow::parse(&good).is_ok(), "{good}");
            assert!(WindowUpdate::parse(&good).is_ok(), "{good}");
        }
        assert_eq!(
            WindowUpdate::parse(&json!({"state": "fullscreen", "drawAttention": true})).unwrap(),
            WindowUpdate { state: Some(WindowState::Fullscreen), ..WindowUpdate::default() }
        );
    }

    #[test]
    fn changes_name_what_opened_closed_resized_and_took_the_focus() {
        let (one, unfocused, two, opened) = (window(1, true), window(1, false), window(2, false), window(2, true));
        let alone = std::slice::from_ref(&one);
        assert_eq!(changes(alone, alone), []);
        assert_eq!(
            changes(alone, &[unfocused.clone(), opened.clone()]),
            [WindowEvent::Created(opened.clone()), WindowEvent::FocusChanged(Some(WindowId(2)))]
        );
        assert_eq!(changes(&[one.clone(), two.clone()], &[unfocused.clone(), two]), [WindowEvent::FocusChanged(None)]);
        assert_eq!(changes(&[one.clone(), opened], std::slice::from_ref(&unfocused)), [WindowEvent::Removed(WindowId(2)), WindowEvent::FocusChanged(None)]);
        let resized = WindowInfo { width: 640, ..one.clone() };
        assert_eq!(changes(alone, std::slice::from_ref(&resized)), [WindowEvent::BoundsChanged(resized.clone())]);
        let maximized = WindowInfo { state: WindowState::Maximized, ..one.clone() };
        assert_eq!(changes(alone, &[maximized]), []);
    }

    #[test]
    fn events_carry_chromes_arguments() {
        assert_eq!(WindowEvent::Removed(WindowId(4)).args(), [json!(4)]);
        assert_eq!(WindowEvent::FocusChanged(None).args(), [json!(-1)]);
        assert_eq!(WindowEvent::FocusChanged(Some(WindowId(2))).args(), [json!(2)]);
        assert_eq!(WindowEvent::Created(window(2, false)).args()[0]["id"], 2);
        assert_eq!(WindowEvent::BoundsChanged(window(2, false)).name(), "windows.onBoundsChanged");
    }
}
