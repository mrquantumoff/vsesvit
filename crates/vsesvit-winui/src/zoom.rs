//! Page zoom. WebView2 zooms a page itself (Ctrl+plus, Ctrl+minus, Ctrl+0, Ctrl+wheel) and
//! remembers the level per site, but the XAML `WebView2` exposes neither the level nor a way to
//! set it. The page script reports the page's `devicePixelRatio`, which is the window's
//! rasterization scale times the zoom; the zoom bubble's buttons press the same shortcuts on
//! the page a person would.

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

/// Presses Ctrl and the key of `step` for the page that has the keyboard focus. Only for a
/// window in the foreground, where the user just clicked.
pub(crate) fn press(step: Step) {
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
    let inputs = [
        key(VK_CONTROL, false),
        key(step.key(), false),
        key(step.key(), true),
        key(VK_CONTROL, true),
    ];
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
    fn odd_ratios_keep_their_own_level_and_garbage_reads_as_100() {
        assert_eq!(percent(1.37, 1.0), 137);
        assert_eq!(percent(f64::NAN, 1.0), 100);
        assert_eq!(percent(1.0, 0.0), 100);
    }
}
