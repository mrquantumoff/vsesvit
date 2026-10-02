//! Page zoom. WebView2 zooms a page itself (Ctrl+plus, Ctrl+minus, Ctrl+0, Ctrl+wheel) and
//! remembers the level per site while it runs, but the XAML `WebView2` exposes neither the level
//! nor a way to set it. The page script reports the page's `devicePixelRatio`, which is the
//! window's rasterization scale times the zoom; the zoom bubble's buttons press the same
//! shortcuts on the page a person would.
//!
//! Core remembers a level per site across restarts (`Profile::site_zoom`), as on Linux. A level
//! that changes in a loaded page is remembered for its site once the window's scale is known to
//! have stayed ([`Unsettled`]); a new document that starts at another level than its site's is
//! taken there with the same key presses ([`Memory`]). Those presses are the only way to set
//! the level: key events sent through DevTools' `Input.dispatchKeyEvent` do not zoom.
//!
//! Key presses are real input, sent to whatever has the keyboard focus, so they are sent only
//! for a click on a zoom bubble button, or in an interactive run to take the selected tab's page
//! to its site's level when the tab is selected, its page focused or a new document starts in it
//! (`BrowserWindow::take_to_site_zoom`). Either way the page is focused first, and a moment
//! later [`press`] sends them only if, checked just before sending, the window is still the
//! foreground one, the page still has the keyboard focus and no modifier or mouse button is
//! held ([`may_press`]); otherwise nothing is sent, and the site's level waits for the next
//! focus or selection. Scripted runs send none and only record the level the page wants.

use std::time::Duration;

use crate::bindings::*;

/// A page's zoom, in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Level(pub u32);

impl Default for Level {
    fn default() -> Self {
        Level(100)
    }
}

impl Level {
    pub fn is_default(self) -> bool {
        self.0 == 100
    }

    pub fn label(self) -> String {
        format!("{}%", self.0)
    }

    /// Core's level, a factor (1.0 = 100%).
    pub fn of_factor(factor: f64) -> Self {
        Level((factor * 100.0).round().max(1.0) as u32)
    }

    pub fn factor(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

/// A zoom the page script reported. Only the top document reports zoom: the script stops
/// before it in frames, and a frame's own DevTools session reports nothing but keys
/// (`shortcuts::parse_binding_call`), so a frame starting cannot take the page anywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Report {
    /// The level a new document started at.
    Start(Level),
    /// The level changed in a loaded document: the user zoomed, or the shell's key presses
    /// took the page to its site's level.
    Change(Level),
}

/// What a tab knows of its page's zoom beyond the level: the level remembered for its site
/// that the page has yet to reach, and the changes it reported.
#[derive(Debug, Default)]
pub(crate) struct Memory {
    wanted: Option<Level>,
    /// The level [`Memory::take_wanted`] took, until its presses are sent or a report comes.
    taken: Option<Level>,
    /// How many changes the page reported, which tells the latest [`Unsettled`] change.
    changes: u64,
}

/// How long a reported change waits before it is remembered, for the window's scale to follow
/// a move to a monitor of another scale.
pub(crate) const SETTLE: Duration = Duration::from_secs(1);

/// A level the page changed to, read with the window drawn at `scale`, to remember for its site
/// once [`SETTLE`] has passed ([`Memory::settled`]). When the window moves to a monitor of
/// another scale the page's ratio can change before the window's scale does, which reads as a
/// zoom the user never made; the scale having changed meanwhile tells it apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Unsettled {
    level: Level,
    scale: f64,
    seq: u64,
}

impl Memory {
    /// Takes `report`, read with the window drawn at `scale`, about a page whose site is
    /// remembered at `remembered`, and returns the change to remember for the site once it
    /// settles, if any. Only a change is remembered: a new document starting at another level
    /// is taken to its site's instead.
    pub fn reported(&mut self, report: Report, remembered: Level, scale: f64) -> Option<Unsettled> {
        match report {
            Report::Start(level) => {
                self.wanted = (level != remembered).then_some(remembered);
                self.taken = None;
                None
            }
            Report::Change(level) => {
                self.wanted = None;
                self.taken = None;
                self.changes += 1;
                Some(Unsettled {
                    level,
                    scale,
                    seq: self.changes,
                })
            }
        }
    }

    /// The level to remember for `change` once it has settled, with the window now drawn at
    /// `scale`: none when the scale moved since, which the monitor did rather than the user,
    /// or when a later change came, which is remembered in its place.
    pub fn settled(&self, change: Unsettled, scale: f64) -> Option<Level> {
        (change.seq == self.changes && change.scale == scale).then_some(change.level)
    }

    /// The site's level, while the page is not at it.
    pub fn wanted(&self) -> Option<Level> {
        self.wanted
    }

    pub fn take_wanted(&mut self) -> Option<Level> {
        self.taken = self.wanted.take();
        self.taken
    }

    /// Whether the level [`Memory::take_wanted`] took still waits for its presses: the page
    /// has reported no level since, so the steps to it still hold.
    pub fn awaits_presses(&self) -> bool {
        self.taken.is_some()
    }

    /// The presses for the level [`Memory::take_wanted`] took were not sent: it is wanted
    /// again, unless the page reported a level since.
    pub fn put_back(&mut self) {
        if self.wanted.is_none() {
            self.wanted = self.taken.take();
        }
    }
}

/// The key presses that take a page at `from` to `to`: Ctrl+0 for 100%, else one Ctrl+plus or
/// Ctrl+minus for each of Chrome's levels on the way.
pub(crate) fn steps(from: Level, to: Level) -> Vec<Step> {
    if from == to {
        return Vec::new();
    }
    if to.is_default() {
        return vec![Step::Reset];
    }
    let (from, to) = (from.0, to.0);
    if to > from {
        let count = LEVELS.iter().filter(|&&l| from < l && l <= to).count();
        vec![Step::In; count.max(1)]
    } else {
        let count = LEVELS.iter().filter(|&&l| to <= l && l < from).count();
        vec![Step::Out; count.max(1)]
    }
}

/// Chrome's zoom levels, in percent; a reported level is snapped to the nearest.
const LEVELS: [u32; 17] = [
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];

/// The zoom, in percent, of a page whose `devicePixelRatio` is `ratio` in a window drawn at
/// `scale`.
pub(crate) fn percent(ratio: f64, scale: f64) -> u32 {
    if !(ratio.is_finite() && scale.is_finite() && ratio > 0.0 && scale > 0.0) {
        return 100;
    }
    let exact = ratio / scale * 100.0;
    let nearest = LEVELS
        .iter()
        .copied()
        .min_by(|a, b| {
            (f64::from(*a) - exact)
                .abs()
                .total_cmp(&(f64::from(*b) - exact).abs())
        })
        .unwrap_or(100);
    if (f64::from(nearest) - exact).abs() <= f64::from(nearest) * 0.02 {
        nearest
    } else {
        exact.round() as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    In,
    Out,
    Reset,
}

impl Step {
    /// The key that, with Ctrl, zooms the page this way.
    fn key(self) -> u16 {
        match self {
            Step::In => 0xBB,    // VK_OEM_PLUS
            Step::Out => 0xBD,   // VK_OEM_MINUS
            Step::Reset => 0x30, // 0
        }
    }
}

const VK_CONTROL: u16 = 0x11;

/// The keys whose being held stops presses ([`may_press`]): Shift, Ctrl, Alt, the Windows
/// keys, and the mouse buttons.
const HELD: [u16; 10] = [
    0x10, VK_CONTROL, 0x12, 0x5B, 0x5C, 0x01, 0x02, 0x04, 0x05, 0x06,
];

/// Why key presses were not sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotSent {
    /// Another window, maybe another app's, is in the foreground and would get them.
    Background,
    /// The page does not have the keyboard focus; something else in the window would get them.
    PageUnfocused,
    /// This key (a virtual-key code) is down: the presses would combine with it, and their
    /// Ctrl release would let go of a Ctrl the user holds.
    Held(u16),
}

/// Whether presses may be sent now: only to a page that has the keyboard focus in the foreground
/// window, while no modifier or mouse button is down (`held`, by virtual-key code).
pub(crate) fn may_press(
    foreground: bool,
    page_focused: bool,
    held: impl Fn(u16) -> bool,
) -> Result<(), NotSent> {
    if !foreground {
        return Err(NotSent::Background);
    }
    if !page_focused {
        return Err(NotSent::PageUnfocused);
    }
    match HELD.into_iter().find(|&vk| held(vk)) {
        Some(vk) => Err(NotSent::Held(vk)),
        None => Ok(()),
    }
}

/// Presses Ctrl and the key of each of `steps`, as one batch of input, for the page in window
/// `hwnd`, if [`may_press`] allows it. That is checked right before the batch goes, in the same
/// call, with `page_focused` asked first, so the foreground window and the keys held are read
/// last.
pub(crate) fn press(
    hwnd: HWND,
    page_focused: impl FnOnce() -> bool,
    steps: &[Step],
) -> Result<(), NotSent> {
    let page_focused = page_focused();
    let foreground = unsafe { GetForegroundWindow() } == hwnd;
    // The high bit: the key is down now.
    let down = |vk: u16| unsafe { GetAsyncKeyState(i32::from(vk)) } < 0;
    may_press(foreground, page_focused, down)?;
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD as u32,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP as u32 } else { 0 },
                ..KEYBDINPUT::default()
            },
        },
    };
    let mut inputs = vec![key(VK_CONTROL, false)];
    for step in steps {
        inputs.extend([key(step.key(), false), key(step.key(), true)]);
    }
    inputs.push(key(VK_CONTROL, true));
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        log::warn!(
            "zoom: only {sent} of {} key events went through",
            inputs.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_is_the_ratio_over_the_window_scale() {
        assert_eq!(percent(1.75, 1.75), 100);
        assert_eq!(percent(1.925, 1.75), 110);
        assert_eq!(percent(1.1, 1.0), 110);
        assert_eq!(percent(0.6666, 1.0), 67);
        assert_eq!(percent(2.1875, 1.75), 125);
        assert_eq!(percent(1.5, 1.5), 100);
    }

    /// The level `memory` remembers for a change reported at scale 1, settled at scale 1.
    fn remembered(memory: &mut Memory, report: Report, site: Level) -> Option<Level> {
        let change = memory.reported(report, site, 1.0)?;
        memory.settled(change, 1.0)
    }

    #[test]
    fn a_new_document_is_taken_to_its_sites_level_and_only_changes_are_remembered() {
        let mut memory = Memory::default();
        assert_eq!(
            remembered(&mut memory, Report::Start(Level(100)), Level(125)),
            None
        );
        assert_eq!(memory.wanted(), Some(Level(125)));
        // The shell's steps, or the user's, change the level; that is the site's now.
        assert_eq!(
            remembered(&mut memory, Report::Change(Level(110)), Level(125)),
            Some(Level(110))
        );
        assert_eq!(memory.wanted(), None);
        assert_eq!(
            remembered(&mut memory, Report::Start(Level(125)), Level(125)),
            None
        );
        assert_eq!(memory.wanted(), None, "already at the site's level");
        assert_eq!(
            remembered(&mut memory, Report::Start(Level(150)), Level(100)),
            None
        );
        assert_eq!(memory.wanted(), Some(Level(100)));
        assert_eq!(
            remembered(&mut memory, Report::Change(Level(100)), Level(150)),
            Some(Level(100)),
            "100% is remembered too, which forgets the site"
        );
    }

    #[test]
    fn a_change_is_remembered_once_settled_and_not_when_the_window_scale_moved() {
        let mut memory = Memory::default();
        // The window went to a 150% monitor: the page's ratio moved before the window's scale,
        // so 100% read as 150%.
        let moved = memory.reported(Report::Change(Level(150)), Level(100), 1.0).unwrap();
        assert_eq!(memory.settled(moved, 1.5), None);
        let zoomed = memory.reported(Report::Change(Level(110)), Level(100), 1.5).unwrap();
        assert_eq!(memory.settled(zoomed, 1.5), Some(Level(110)));
        // Several presses in a row: only the last is remembered.
        let first = memory.reported(Report::Change(Level(125)), Level(110), 1.5).unwrap();
        let last = memory.reported(Report::Change(Level(150)), Level(110), 1.5).unwrap();
        assert_eq!(memory.settled(first, 1.5), None);
        assert_eq!(memory.settled(last, 1.5), Some(Level(150)));
        // A new document after a change leaves it to be remembered for its own page's site.
        let before_leaving = memory.reported(Report::Change(Level(90)), Level(150), 1.5).unwrap();
        memory.reported(Report::Start(Level(100)), Level(100), 1.5);
        assert_eq!(memory.settled(before_leaving, 1.5), Some(Level(90)));
    }

    #[test]
    fn a_site_level_whose_presses_were_not_sent_is_wanted_again_until_a_report_comes() {
        let mut memory = Memory::default();
        memory.reported(Report::Start(Level(100)), Level(125), 1.0);
        assert_eq!(memory.take_wanted(), Some(Level(125)));
        assert_eq!(memory.wanted(), None, "not pressed for twice");
        assert!(memory.awaits_presses());
        memory.put_back();
        assert_eq!(memory.wanted(), Some(Level(125)), "for the next focus or selection");
        assert_eq!(memory.take_wanted(), Some(Level(125)));
        memory.reported(Report::Change(Level(110)), Level(125), 1.0);
        assert!(!memory.awaits_presses(), "the steps from the old level are stale");
        memory.put_back();
        assert_eq!(memory.wanted(), None, "the user zoomed meanwhile");
        memory.reported(Report::Start(Level(100)), Level(150), 1.0);
        assert_eq!(memory.take_wanted(), Some(Level(150)));
        memory.reported(Report::Start(Level(100)), Level(125), 1.0);
        memory.put_back();
        assert_eq!(
            memory.wanted(),
            Some(Level(125)),
            "the new document's level, not the old one's"
        );
    }

    #[test]
    fn presses_go_only_to_the_focused_page_of_the_foreground_window_with_nothing_held() {
        let none = |_: u16| false;
        assert_eq!(may_press(true, true, none), Ok(()));
        assert_eq!(may_press(false, true, none), Err(NotSent::Background));
        assert_eq!(may_press(true, false, none), Err(NotSent::PageUnfocused));
        for (vk, name) in [
            (0x10, "Shift"),
            (0x11, "Ctrl"),
            (0x12, "Alt"),
            (0x5B, "left Windows"),
            (0x5C, "right Windows"),
            (0x01, "left button"),
            (0x02, "right button"),
            (0x04, "middle button"),
            (0x05, "X1 button"),
            (0x06, "X2 button"),
        ] {
            assert_eq!(
                may_press(true, true, |k| k == vk),
                Err(NotSent::Held(vk)),
                "{name}"
            );
        }
        assert_eq!(
            may_press(true, true, |k| k == 0x41),
            Ok(()),
            "a letter held is no modifier"
        );
    }

    #[test]
    fn steps_go_level_by_level_and_back_to_100_at_once() {
        use Step::{In, Out, Reset};
        assert_eq!(steps(Level(100), Level(125)), [In, In]);
        assert_eq!(steps(Level(150), Level(110)), [Out, Out]);
        assert_eq!(steps(Level(67), Level(50)), [Out]);
        assert_eq!(steps(Level(175), Level(100)), [Reset]);
        assert_eq!(steps(Level(125), Level(125)), []);
        assert_eq!(steps(Level(100), Level(500)).len(), 9);
        assert_eq!(steps(Level(137), Level(150)), [In], "from a level between two");
    }

    #[test]
    fn levels_are_factors_in_core() {
        assert_eq!(Level::of_factor(1.25), Level(125));
        assert_eq!(Level::of_factor(0.333), Level(33));
        assert!((Level(110).factor() - 1.1).abs() < 1e-9);
    }

    #[test]
    fn odd_ratios_keep_their_own_level_and_garbage_reads_as_100() {
        assert_eq!(percent(1.37, 1.0), 137);
        assert_eq!(percent(f64::NAN, 1.0), 100);
        assert_eq!(percent(1.0, 0.0), 100);
    }
}
