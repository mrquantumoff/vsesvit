//! Owned by W0.2.
//! The page-focus policy (design section 1.5). Chromium focuses new browsers and navigations itself;
//! the glue refuses every such request the shell's policy for that tab does not allow.

/// Who has the keyboard in a window.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyboardOwner {
    /// The shell's chrome (omnibox, tab strip).
    Chrome,
    /// The selected tab's page.
    Page,
    /// An interactive overlay or an extension popup.
    Overlay,
}

/// The facts the focus decision needs, published per tab with `Engine::set_page_focus`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PageFocus {
    /// The tab's window is the active window.
    pub window_active: bool,
    /// The tab is its window's selected tab.
    pub tab_selected: bool,
    /// Who has the keyboard in that window.
    pub keyboard: KeyboardOwner,
}

impl PageFocus {
    /// The policy of a browser not yet shown: refuses every focus request.
    pub const NEW_BROWSER: PageFocus =
        PageFocus { window_active: false, tab_selected: false, keyboard: KeyboardOwner::Chrome };

    /// Whether the page may take focus: only the selected tab of the active window, while it has the
    /// keyboard. Every other request is refused, whatever its source.
    pub fn allows(self) -> bool {
        self.window_active && self.tab_selected && self.keyboard == KeyboardOwner::Page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_the_selected_page_of_the_active_window() {
        let owners = [KeyboardOwner::Chrome, KeyboardOwner::Page, KeyboardOwner::Overlay];
        let mut allowed = Vec::new();
        for window_active in [false, true] {
            for tab_selected in [false, true] {
                for keyboard in owners {
                    let policy = PageFocus { window_active, tab_selected, keyboard };
                    if policy.allows() {
                        allowed.push(policy);
                    }
                }
            }
        }
        assert_eq!(allowed, [PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page }]);
    }

    #[test]
    fn a_new_browser_is_refused() {
        assert!(!PageFocus::NEW_BROWSER.allows());
    }
}
