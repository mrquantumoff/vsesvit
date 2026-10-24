//! Owned by W1.6.
//! The shell's synchronous answers to the engine (design section 1.5, Hooks). The engine calls them
//! inside its own callbacks, which can run inside a GPUI update, so they read only the snapshot the
//! windows publish and never reach `gpui::App` or the core `Profile`. In debug builds, any bridge
//! entry point that takes the App panics when it runs inside a hook.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use vsesvit_engine::{
    Browsing, NativeHandle, NavigationDecision, NavigationRequest, NeutralizeFailed, PhysRect, PopupDecision, PopupRequest,
    ShellHooks, TabId,
};

use crate::platform::PlatformError;

/// What a hook needs to know about one tab, as its window last laid it out.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TabHook {
    pub browsing: Browsing,
    /// The window's content host.
    pub parent: NativeHandle,
    /// Where the window's pages sit in the host.
    pub bounds: PhysRect,
    pub scale_factor: f32,
}

/// The windows' published state. `BrowserWindow`s refresh it whenever layout or their tabs change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HookSnapshot {
    pub tabs: HashMap<TabId, TabHook>,
    /// The popup-blocking pref: block popups no user gesture opened.
    pub block_popups: bool,
}

pub type SharedSnapshot = Rc<RefCell<HookSnapshot>>;

/// The snapshot the windows write.
pub fn snapshot(cx: &gpui::App) -> SharedSnapshot {
    super::bridge(cx).hooks.clone()
}

thread_local! {
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as inside a hook until dropped.
pub(crate) struct InHook(bool);

impl InHook {
    pub fn enter() -> InHook {
        InHook(IN_HOOK.replace(true))
    }
}

impl Drop for InHook {
    fn drop(&mut self) {
        IN_HOOK.set(self.0);
    }
}

/// The debug guard: the bridge calls it wherever it takes the App.
pub(crate) fn assert_outside_hook() {
    debug_assert!(!IN_HOOK.get(), "a shell hook reached gpui::App");
}

pub(crate) struct Hooks {
    snapshot: SharedSnapshot,
    neutralize: Box<dyn Fn(NativeHandle) -> Result<(), PlatformError>>,
}

impl Hooks {
    pub fn new(snapshot: SharedSnapshot, neutralize: impl Fn(NativeHandle) -> Result<(), PlatformError> + 'static) -> Hooks {
        Hooks { snapshot, neutralize: Box::new(neutralize) }
    }
}

impl ShellHooks for Hooks {
    /// Every navigation proceeds until HTTPS-only reads the privacy snapshot (C.2, W3.1).
    fn navigation_policy(&self, _request: &NavigationRequest) -> NavigationDecision {
        let _hook = InHook::enter();
        NavigationDecision::Allow
    }

    fn popup_policy(&self, request: &PopupRequest) -> PopupDecision {
        let _hook = InHook::enter();
        match self.snapshot.try_borrow() {
            Ok(snapshot) => popup_decision(&snapshot, request, TabId::next),
            Err(_) => {
                log::error!("hooks: the snapshot is being written; popup from tab {} blocked", request.opener.get());
                PopupDecision::Block
            }
        }
    }

    fn neutralize_foreign(&self, window: NativeHandle) -> Result<(), NeutralizeFailed> {
        let _hook = InHook::enter();
        (self.neutralize)(window).map_err(|e| NeutralizeFailed { reason: e.to_string() })
    }
}

/// A popup opens as a new tab in its opener's window, unless popups are blocked and no user gesture
/// opened it, or its opener is unknown.
fn popup_decision(snapshot: &HookSnapshot, request: &PopupRequest, new_tab: impl FnOnce() -> TabId) -> PopupDecision {
    let Some(opener) = snapshot.tabs.get(&request.opener) else { return PopupDecision::Block };
    if snapshot.block_popups && !request.user_gesture {
        return PopupDecision::Block;
    }
    PopupDecision::AllowAsTab { tab: new_tab(), parent: opener.parent, bounds: opener.bounds, scale_factor: opener.scale_factor }
}

#[cfg(test)]
mod tests {
    use vsesvit_engine::{OpenDisposition, Url};

    use super::*;

    fn request(opener: TabId, user_gesture: bool) -> PopupRequest {
        PopupRequest { opener, url: Url::parse("https://example.com/").unwrap(), disposition: OpenDisposition::NewForegroundTab, user_gesture }
    }

    #[test]
    fn a_popup_opens_beside_its_opener_unless_blocked_without_a_gesture() {
        let (opener, popup) = (TabId::next(), TabId::next());
        let hook = TabHook {
            browsing: Browsing::Normal,
            parent: NativeHandle::from_raw(7),
            bounds: PhysRect { x: 0, y: 0, width: 800, height: 600 },
            scale_factor: 1.5,
        };
        let mut snapshot = HookSnapshot { tabs: HashMap::from([(opener, hook)]), block_popups: true };
        let allowed = PopupDecision::AllowAsTab { tab: popup, parent: hook.parent, bounds: hook.bounds, scale_factor: 1.5 };
        assert_eq!(popup_decision(&snapshot, &request(opener, true), || popup), allowed);
        assert_eq!(popup_decision(&snapshot, &request(opener, false), || popup), PopupDecision::Block);
        assert_eq!(popup_decision(&snapshot, &request(popup, true), || popup), PopupDecision::Block, "unknown opener");
        snapshot.block_popups = false;
        assert_eq!(popup_decision(&snapshot, &request(opener, false), || popup), allowed);
    }

    #[test]
    fn hooks_answer_from_the_snapshot_without_the_app() {
        let snapshot = SharedSnapshot::default();
        let hooks = Hooks::new(snapshot.clone(), |w| if w.raw() == 1 { Ok(()) } else { Err(PlatformError::new("gone")) });
        let held = snapshot.borrow_mut();
        assert_eq!(hooks.popup_policy(&request(TabId::next(), true)), PopupDecision::Block, "a held snapshot blocks, never panics");
        drop(held);
        assert!(hooks.neutralize_foreign(NativeHandle::from_raw(1)).is_ok());
        assert_eq!(hooks.neutralize_foreign(NativeHandle::from_raw(2)), Err(NeutralizeFailed { reason: "gone".into() }));
        assert!(!IN_HOOK.get(), "the guard is lifted after each hook");
    }
}
