//! Owned by W1.1.
//! One scale for GPUI and Chromium (spike-core 5.4). GPUI reads `GPUI_X11_SCALE_FACTOR`, else
//! `Xft.dpi` / 96, else RandR; Chromium never reads RandR. So boot computes this rule (GPUI's minus
//! RandR) before anything else and hands the result to both: `GPUI_X11_SCALE_FACTOR` in the
//! environment and `--force-device-scale-factor` on Chromium's command line.

/// GPUI's variable; boot sets it to `scale_factor()` before GPUI starts.
pub const GPUI_SCALE_VAR: &str = "GPUI_X11_SCALE_FACTOR";
const BASE_DPI: f32 = 96.0;

/// The scale for this session, from the environment and the X resource database.
pub fn scale_factor() -> f32 {
    let env = std::env::var(GPUI_SCALE_VAR).ok();
    let xft = super::conn().ok().and_then(|x| {
        let db = x11rb::resource_manager::new_from_default(&x.conn).ok()?;
        db.get_value::<f32>("Xft.dpi", "Xft.dpi").ok().flatten()
    });
    rule(env.as_deref(), xft)
}

/// GPUI's rule without RandR. A value GPUI would reject (not a positive normal number, or `randr`)
/// counts as unset, because boot replaces it with the result.
fn rule(env: Option<&str>, xft_dpi: Option<f32>) -> f32 {
    let valid = |s: f32| s.is_normal() && s > 0.0;
    env.and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|&s| valid(s))
        .or(xft_dpi.map(|dpi| dpi / BASE_DPI).filter(|&s| valid(s)))
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::rule;

    #[test]
    fn the_variable_wins_then_xft_dpi_then_one() {
        assert_eq!(rule(Some("1.5"), Some(192.0)), 1.5);
        assert_eq!(rule(None, Some(192.0)), 2.0);
        assert_eq!(rule(None, None), 1.0);
    }

    #[test]
    fn values_gpui_would_reject_count_as_unset() {
        for bad in ["randr", "", "0", "-2", "NaN", "inf"] {
            assert_eq!(rule(Some(bad), Some(144.0)), 1.5, "{bad:?}");
        }
        assert_eq!(rule(None, Some(0.0)), 1.0);
    }
}
