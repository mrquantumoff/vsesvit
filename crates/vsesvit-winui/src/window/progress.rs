//! The load progress along the bottom of the address pill. WebView2 reports no fraction, so the
//! bar eases toward a ceiling for each stage of the navigation, slowing as it nears it, and on
//! completion runs to the end and goes.

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::BrowserWindow;
use crate::exec;
use crate::tab::{Load, TabId};
use crate::xaml;

const FRAME: Duration = Duration::from_millis(16);
/// Where the bar starts, so a navigation shows at once.
const START: f64 = 0.08;
/// Seconds for the bar to cover about two thirds of what remains to its ceiling while loading.
const CRAWL: f64 = 1.2;
/// The same, when the load completed and the bar runs to the end.
const FINISH: f64 = 0.05;
/// How long the full bar stays before it goes.
const HOLD: f64 = 0.15;

fn ceiling(load: Load) -> f64 {
    match load {
        Load::Idle => 1.0,
        Load::Started => 0.3,
        Load::Committed => 0.85,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Bar {
    #[default]
    Hidden,
    Loading { tab: TabId, fraction: f64 },
    /// The load completed; `held` counts the seconds the bar has been full.
    Finishing { tab: TabId, fraction: f64, held: f64 },
}

impl Bar {
    /// The bar `dt` seconds on, showing `tab`, whose navigation is at `load`.
    fn step(self, tab: TabId, load: Load, dt: f64) -> Bar {
        let approach = |from: f64, to: f64, tau: f64| to - (to - from) * (-dt / tau).exp();
        match (self, load) {
            (Bar::Loading { tab: t, fraction }, Load::Started | Load::Committed) if t == tab => {
                Bar::Loading {
                    tab,
                    fraction: fraction.max(approach(fraction, ceiling(load), CRAWL)),
                }
            }
            (_, Load::Started | Load::Committed) => Bar::Loading { tab, fraction: START },
            (Bar::Loading { tab: t, fraction }, Load::Idle) if t == tab => {
                Bar::Finishing { tab, fraction, held: 0.0 }.step(tab, load, dt)
            }
            (Bar::Finishing { tab: t, fraction, held }, Load::Idle) if t == tab => {
                if fraction >= 0.995 {
                    let held = held + dt;
                    if held >= HOLD {
                        Bar::Hidden
                    } else {
                        Bar::Finishing { tab, fraction: 1.0, held }
                    }
                } else {
                    Bar::Finishing { tab, fraction: approach(fraction, 1.0, FINISH), held }
                }
            }
            (_, Load::Idle) => Bar::Hidden,
        }
    }

    fn fraction(self) -> Option<f64> {
        match self {
            Bar::Hidden => None,
            Bar::Loading { fraction, .. } | Bar::Finishing { fraction, .. } => Some(fraction),
        }
    }
}

/// While the bar shows, a loop steps it every frame; it ends once the bar is hidden.
#[derive(Default)]
pub(super) struct Progress {
    bar: Cell<Bar>,
}

impl BrowserWindow {
    /// Starts the bar when the shown tab begins loading; the running loop sees every other change.
    pub(super) fn show_progress(&self) {
        if self.progress.bar.get() != Bar::Hidden {
            return;
        }
        if !self.progress_step(0.0) {
            return;
        }
        let me = self.me.clone();
        exec::spawn(async move {
            let mut last = Instant::now();
            loop {
                exec::sleep(FRAME).await;
                let Some(window) = me.upgrade() else { return };
                let now = Instant::now();
                let dt = now.duration_since(last).as_secs_f64();
                last = now;
                if !window.progress_step(dt) {
                    return;
                }
            }
        });
    }

    /// How much of the address pill the load progress covers, while it shows.
    pub fn address_progress_shown(&self) -> Option<f64> {
        self.progress.bar.get().fraction()
    }

    /// Steps the bar and draws it. Returns whether it still shows.
    fn progress_step(&self, dt: f64) -> bool {
        let bar = match self.active_tab() {
            Some(tab) => self.progress.bar.get().step(tab.id, tab.state().load, dt),
            None => Bar::Hidden,
        };
        self.progress.bar.set(bar);
        let element = &self.ui.address_progress;
        match bar.fraction() {
            Some(fraction) => {
                let track = self
                    .ui
                    .address_progress_track
                    .ActualWidth()
                    .unwrap_or_default();
                let _ = element.SetWidth(track * fraction);
                let _ = xaml::set_visible(element, true);
                true
            }
            None => {
                let _ = xaml::set_visible(element, false);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut bar: Bar, tab: TabId, load: Load, seconds: f64) -> Bar {
        for _ in 0..(seconds / 0.016) as usize {
            bar = bar.step(tab, load, 0.016);
        }
        bar
    }

    #[test]
    fn a_load_eases_toward_each_stage_ceiling_without_reaching_it() {
        let bar = Bar::Hidden.step(1, Load::Started, 0.0);
        assert_eq!(bar.fraction(), Some(START));
        let started = run(bar, 1, Load::Started, 10.0).fraction().unwrap();
        assert!(started > 0.29 && started < 0.3, "{started}");
        let committed = run(bar, 1, Load::Committed, 1.0).fraction().unwrap();
        assert!(committed > started * 1.5 && committed < 0.85, "{committed}");
    }

    #[test]
    fn a_new_navigation_on_the_same_tab_never_moves_the_bar_back() {
        let bar = run(Bar::Hidden.step(1, Load::Committed, 0.0), 1, Load::Committed, 3.0);
        let before = bar.fraction().unwrap();
        let after = bar.step(1, Load::Started, 0.016).fraction().unwrap();
        assert!(after >= before, "{before} -> {after}");
    }

    #[test]
    fn a_completed_load_runs_to_the_end_then_hides() {
        let bar = run(Bar::Hidden.step(1, Load::Committed, 0.0), 1, Load::Committed, 0.5);
        let full = run(bar, 1, Load::Idle, 0.3);
        assert_eq!(full.fraction(), Some(1.0));
        assert_eq!(run(bar, 1, Load::Idle, 0.6), Bar::Hidden);
    }

    #[test]
    fn switching_tabs_hides_the_bar_or_starts_it_for_the_other_tab() {
        let bar = run(Bar::Hidden.step(1, Load::Committed, 0.0), 1, Load::Committed, 2.0);
        assert_eq!(bar.step(2, Load::Idle, 0.016), Bar::Hidden);
        assert_eq!(bar.step(2, Load::Started, 0.016).fraction(), Some(START));
    }
}
