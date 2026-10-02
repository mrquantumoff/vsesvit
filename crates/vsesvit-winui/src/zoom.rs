//! Page zoom. WebView2 zooms a page itself (Ctrl+plus, Ctrl+minus, Ctrl+0, Ctrl+wheel) and
//! remembers the level per site while it runs, but the XAML `WebView2` exposes neither the level
//! nor a way to set it. The page script reports the page's `devicePixelRatio`, which is the
//! window's rasterization scale times the zoom; the zoom bubble's buttons press the same
//! shortcuts on the page a person would.
//!
//! Core remembers a level per site across restarts (`Profile::site_zoom`), as on Linux. A level
//! that changes in a loaded page is remembered for its site; a new document that starts at
//! another level than its site's is taken there with the same key presses, once the tab is
//! selected in the foreground window ([`Memory`]). Those presses are the only way to set the
//! level, so scripted runs, which send no OS input, only record the level the page wants.

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
/// that the page has yet to reach.
#[derive(Debug, Default)]
pub(crate) struct Memory {
    wanted: Option<Level>,
}

impl Memory {
    /// Takes `report` about a page whose site is remembered at `remembered`, and returns the
    /// level to remember for the site, if any. Only a change is remembered: a new document
    /// starting at another level is taken to its site's instead.
    pub fn reported(&mut self, report: Report, remembered: Level) -> Option<Level> {
        match report {
            Report::Start(level) => {
                self.wanted = (level != remembered).then_some(remembered);
                None
            }
            Report::Change(level) => {
                self.wanted = None;
                Some(level)
            }
        }
    }

    /// The site's level, while the page is not at it.
    pub fn wanted(&self) -> Option<Level> {
        self.wanted
    }

    pub fn take_wanted(&mut self) -> Option<Level> {
        self.wanted.take()
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

/// Presses Ctrl and the key of each of `steps` for the page that has the keyboard focus. Only
/// for a window in the foreground.
pub(crate) fn press(steps: &[Step]) {
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

    #[test]
    fn a_new_document_is_taken_to_its_sites_level_and_only_changes_are_remembered() {
        let mut memory = Memory::default();
        assert_eq!(
            memory.reported(Report::Start(Level(100)), Level(125)),
            None
        );
        assert_eq!(memory.wanted(), Some(Level(125)));
        // The shell's steps, or the user's, change the level; that is the site's now.
        assert_eq!(
            memory.reported(Report::Change(Level(110)), Level(125)),
            Some(Level(110))
        );
        assert_eq!(memory.wanted(), None);
        assert_eq!(
            memory.reported(Report::Start(Level(125)), Level(125)),
            None
        );
        assert_eq!(memory.wanted(), None, "already at the site's level");
        assert_eq!(
            memory.reported(Report::Start(Level(150)), Level(100)),
            None
        );
        assert_eq!(memory.wanted(), Some(Level(100)));
        assert_eq!(
            memory.reported(Report::Change(Level(100)), Level(150)),
            Some(Level(100)),
            "100% is remembered too, which forgets the site"
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
