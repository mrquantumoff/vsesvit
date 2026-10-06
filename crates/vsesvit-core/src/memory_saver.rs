//! Memory Saver, as Chrome's: a tab left in the background long enough goes to sleep, its page
//! freed from memory, and wakes when the user goes back to it. On by default
//! ([`keys::MEMORY_SAVER`]), after the delay of [`keys::MEMORY_SAVER_MODE`].
//!
//! Each shell keeps an [`IdleClock`] per tab and runs a [`Sweep`] over its tabs every
//! [`SWEEP_EVERY`]; the sweep says which go to sleep. A tab that something keeps awake (it is on
//! screen, pinned, playing sound, capturing, its site may notify, a page it opened or that opened
//! it can reach it) restarts its clock, so it sleeps only once it has been left alone for the
//! whole delay. Before putting a tab to sleep, the shell runs [`UNSAVED_INPUT_SCRIPT`] in it and
//! keeps it awake (restarting its clock) when the page has form input not yet submitted.
//!
//! The shells put tabs to sleep and wake them with their engine's means; a sleeping tab keeps its
//! address, title, icon and history, so the session, tab search and sync's open tabs still list
//! it.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::permissions::{Origin, Permission, Setting};
use crate::prefs::keys;
use crate::{Profile, Url};

/// How often each shell sweeps its tabs.
pub const SWEEP_EVERY: Duration = Duration::from_secs(60);

/// The title of the Settings switch (sentence case; GTK title-cases it itself).
pub const TITLE: &str = "Memory Saver";
/// Under it in Settings.
pub const DESCRIPTION: &str = "Frees up memory from inactive tabs, which become active again when you go back to them";
/// The title of the Settings choice of [`MemorySaverMode`].
pub const MODE_TITLE: &str = "Memory savings";

/// How soon tabs sleep: Chrome's three choices, with its delays.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySaverMode {
    Moderate,
    Balanced,
    Maximum,
}

impl MemorySaverMode {
    pub const ALL: [MemorySaverMode; 3] = [Self::Moderate, Self::Balanced, Self::Maximum];

    /// How long a tab is left alone before it sleeps.
    pub fn delay(self) -> Duration {
        let hours = match self {
            Self::Moderate => 6,
            Self::Balanced => 4,
            Self::Maximum => 2,
        };
        Duration::from_secs(hours * 60 * 60)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moderate => "Moderate",
            Self::Balanced => "Balanced",
            Self::Maximum => "Maximum",
        }
    }

    /// For Settings.
    pub fn description(self) -> &'static str {
        match self {
            Self::Moderate => "Tabs become inactive after 6 hours in the background",
            Self::Balanced => "Tabs become inactive after 4 hours in the background",
            Self::Maximum => "Tabs become inactive after 2 hours in the background",
        }
    }
}

/// What a shell knows about one of its awake tabs at a sweep.
#[derive(Clone, Copy, Debug, Default)]
pub struct TabActivity<'a> {
    /// The address of the page it shows.
    pub url: &'a str,
    /// On screen: selected in its window (minimized or not, as Chrome), beside it in a split
    /// view, or in picture-in-picture.
    pub shown: bool,
    pub pinned: bool,
    /// Playing sound, or the media player's tab, which can play it again.
    pub audible: bool,
    /// Using the camera, the microphone or the screen.
    pub capturing: bool,
    /// It opened a tab, or a page opened it, that can reach its page (`window.opener`), which
    /// the engine may run in the same process. Chrome keeps such tabs awake as well.
    pub related: bool,
}

/// When an awake tab was last kept awake (or opened).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleClock(Instant);

impl IdleClock {
    pub fn new(now: Instant) -> IdleClock {
        IdleClock(now)
    }

    pub fn restart(&mut self, now: Instant) {
        self.0 = now;
    }
}

/// One pass over a shell's tabs, at `now`, with the preferences and site settings it read once.
pub struct Sweep {
    now: Instant,
    /// `None` while Memory Saver is off.
    delay: Option<Duration>,
    /// Sites allowed to show notifications, whose tabs stay awake to show them, as in Chrome.
    notifying: Vec<Origin>,
}

impl Sweep {
    pub fn new(p: &mut Profile, now: Instant) -> Sweep {
        let delay = p.prefs().get(&keys::MEMORY_SAVER).then(|| p.prefs().get(&keys::MEMORY_SAVER_MODE).delay());
        let notifying = p
            .site_permissions()
            .all()
            .into_iter()
            .filter(|s| s.permission == Permission::Notifications && s.setting == Setting::Allow)
            .map(|s| s.origin)
            .collect();
        Sweep { now, delay, notifying }
    }

    /// Whether `tab` goes to sleep now. Whatever keeps it awake restarts its `clock`; while
    /// Memory Saver is off, the clocks still run, as Chrome's do, so turning it on puts the tabs
    /// left alone long enough to sleep at the next sweep.
    pub fn sleeps(&self, tab: &TabActivity, clock: &mut IdleClock) -> bool {
        if self.kept_awake(tab) {
            clock.restart(self.now);
            return false;
        }
        self.delay.is_some_and(|delay| self.now.saturating_duration_since(clock.0) >= delay)
    }

    fn kept_awake(&self, tab: &TabActivity) -> bool {
        let Ok(url) = Url::parse(tab.url) else { return true };
        // Only web pages sleep: a new tab, an extension's page or a local file costs little and
        // may not come back as it was.
        let web = matches!(url.scheme(), "http" | "https");
        let notifies = Origin::of(&url).is_some_and(|origin| self.notifying.contains(&origin));
        !web || tab.shown || tab.pinned || tab.audible || tab.capturing || tab.related || notifies
    }
}

/// Run in a tab's page before it sleeps: `true` when a text field, check box, radio button or
/// list on the page holds something other than what the page loaded with, as when the user has
/// typed into a form and not submitted it. Chrome keeps such tabs awake.
pub const UNSAVED_INPUT_SCRIPT: &str = r#"(() => {
  const skipped = new Set(['hidden', 'submit', 'button', 'reset', 'image', 'file']);
  for (const field of document.querySelectorAll('input, textarea')) {
    if (skipped.has(field.type) || field.disabled || field.readOnly) continue;
    const changed = field.type === 'checkbox' || field.type === 'radio'
      ? field.checked !== field.defaultChecked
      : field.value !== field.defaultValue;
    if (changed) return true;
  }
  for (const list of document.querySelectorAll('select')) {
    if (list.disabled) continue;
    const options = [...list.options];
    if (list.multiple || list.size > 1) {
      if (options.some(o => o.selected !== o.defaultSelected)) return true;
      continue;
    }
    const chosen = options.findLastIndex(o => o.defaultSelected);
    if (list.selectedIndex !== (chosen === -1 ? (options.length ? 0 : -1) : chosen)) return true;
  }
  return false;
})()"#;

/// Whether what [`UNSAVED_INPUT_SCRIPT`] returned, as the engine serializes it, says the page
/// has unsaved input. A page the script could not run in is taken to have none.
pub fn has_unsaved_input(result: &str) -> bool {
    result.trim() == "true"
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn sweep(delay: Option<Duration>, now: Instant) -> Sweep {
        Sweep { now, delay, notifying: vec![Origin::parse("https://chat.example").unwrap()] }
    }

    fn page(url: &str) -> TabActivity<'_> {
        TabActivity { url, ..TabActivity::default() }
    }

    #[test]
    fn chromes_delays_from_moderate_to_maximum() {
        let hours: Vec<u64> = MemorySaverMode::ALL.iter().map(|m| m.delay().as_secs() / 3600).collect();
        assert_eq!(hours, [6, 4, 2]);
    }

    #[test]
    fn a_background_web_page_sleeps_once_left_alone_for_the_delay() {
        let start = Instant::now();
        let mut clock = IdleClock::new(start);
        let tab = page("https://example.com/a");
        assert!(!sweep(Some(4 * HOUR), start + 4 * HOUR - Duration::from_secs(1)).sleeps(&tab, &mut clock));
        assert!(sweep(Some(4 * HOUR), start + 4 * HOUR).sleeps(&tab, &mut clock));
        assert!(!sweep(None, start + 10 * HOUR).sleeps(&tab, &mut clock), "Memory Saver is off");
    }

    #[test]
    fn whatever_keeps_a_tab_awake_restarts_its_clock() {
        let start = Instant::now();
        let url = "https://example.com/";
        let kept = [
            TabActivity { shown: true, ..page(url) },
            TabActivity { pinned: true, ..page(url) },
            TabActivity { audible: true, ..page(url) },
            TabActivity { capturing: true, ..page(url) },
            TabActivity { related: true, ..page(url) },
            page("https://chat.example/inbox"),
            page("about:blank"),
            page("file:///home/user/notes.html"),
            page("chrome-extension://abcdefghijklmnopabcdefghijklmnop/options.html"),
            page("not a url"),
        ];
        for tab in kept {
            let mut clock = IdleClock::new(start);
            assert!(!sweep(Some(2 * HOUR), start + 3 * HOUR).sleeps(&tab, &mut clock), "{tab:?} slept");
            assert_eq!(clock, IdleClock::new(start + 3 * HOUR), "{tab:?} kept its clock");
        }
        let mut clock = IdleClock::new(start);
        let switched_away = page(url);
        assert!(!sweep(Some(2 * HOUR), start + 3 * HOUR).sleeps(&TabActivity { shown: true, ..switched_away }, &mut clock));
        assert!(!sweep(Some(2 * HOUR), start + 4 * HOUR).sleeps(&switched_away, &mut clock));
        assert!(sweep(Some(2 * HOUR), start + 5 * HOUR).sleeps(&switched_away, &mut clock));
    }

    #[test]
    fn the_clocks_run_while_memory_saver_is_off() {
        let start = Instant::now();
        let mut clock = IdleClock::new(start);
        let tab = page("http://example.com/");
        assert!(!sweep(None, start + 5 * HOUR).sleeps(&tab, &mut clock));
        assert!(sweep(Some(4 * HOUR), start + 5 * HOUR).sleeps(&tab, &mut clock));
    }

    #[test]
    fn only_the_literal_true_is_unsaved_input() {
        assert!(has_unsaved_input("true"));
        assert!(!has_unsaved_input("false"));
        assert!(!has_unsaved_input(""));
        assert!(!has_unsaved_input("\"true\""));
    }
}
