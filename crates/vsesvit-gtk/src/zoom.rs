//! Page zoom steps.

pub(crate) use vsesvit_core::zoom::DEFAULT;

/// Chrome's zoom levels, as the Windows shell's are. Two decimals, not thirds: the per-site store
/// keeps whole percents, so a level read back from it must still be a preset.
const LEVELS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];
const EPSILON: f64 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    In,
    Out,
}

/// The next preset level from `current`, which need not be a preset itself.
pub(crate) fn step(current: f64, step: Step) -> f64 {
    match step {
        Step::In => LEVELS.iter().copied().find(|&l| l > current + EPSILON),
        Step::Out => LEVELS
            .iter()
            .rev()
            .copied()
            .find(|&l| l < current - EPSILON),
    }
    .unwrap_or(current)
}

pub(crate) fn percent(level: f64) -> String {
    format!("{:.0}%", level * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_walk_the_presets_and_stop_at_the_ends() {
        assert_eq!(step(1.0, Step::In), 1.1);
        assert_eq!(step(1.0, Step::Out), 0.9);
        assert_eq!(step(5.0, Step::In), 5.0);
        assert_eq!(step(0.25, Step::Out), 0.25);
    }

    #[test]
    fn zoom_out_follows_chromes_presets_below_half() {
        assert_eq!(step(0.5, Step::Out), 0.33);
        assert_eq!(step(0.33, Step::Out), 0.25);
        assert_eq!(step(0.25, Step::In), 0.33);
        // A level read back from the store's whole percents is still a preset.
        assert_eq!(step(0.67, Step::Out), 0.5);
    }

    #[test]
    fn off_preset_levels_snap_to_the_neighbouring_preset() {
        assert_eq!(step(1.05, Step::In), 1.1);
        assert_eq!(step(1.05, Step::Out), 1.0);
        assert_eq!(step(9.0, Step::Out), 5.0);
    }

    #[test]
    fn percent_is_rounded() {
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.67), "67%");
        assert_eq!(percent(1.25), "125%");
    }
}
