//! Owned by W1.3.
//! The shutdown state machine (design section 3.6, G16): Running → ClosingUser → ClosingInternal →
//! Drained → Shutdown, with Stuck as the one failure exit from a Closing state. It decides; the
//! registry acts. Pure, so every transition is tested without CEF.

/// Browsers alive from the moment the glue asks CEF to create them until their `on_before_close`
/// (design section 3.6, the live-browser count). User and internal browsers are counted apart, so each
/// shutdown phase can report its own end.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveCount {
    /// Tabs, popups, extension popups, DevTools, Chrome-created windows, parked download browsers.
    pub user: u32,
    /// The maintenance, bridge and management browsers.
    pub internal: u32,
}

impl LiveCount {
    /// Every live browser.
    pub fn total(self) -> u32 {
        self.user + self.internal
    }
}

/// Where shutdown is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No shutdown asked for; browsers may be created.
    Running,
    /// `begin_shutdown`: the user's browsers are closing; the hidden ones still serve the last jobs.
    ClosingUser,
    /// `close_internal`: every browser is closing.
    ClosingInternal,
    /// The live count reached zero in `ClosingInternal`; `cef::shutdown` may run.
    Drained,
    /// `cef::shutdown` ran. No CEF call is allowed any more.
    Shutdown,
    /// The driver was asked to shut down with browsers still alive. No CEF call is allowed any more,
    /// `cef::shutdown` included (G16).
    Stuck,
}

/// What a transition asks the glue to announce.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// `BrowserEvent::UserBrowsersClosed`.
    UserBrowsersClosed,
    /// `BrowserEvent::AllBrowsersClosed`.
    AllBrowsersClosed,
}

/// The state machine. Each report is made exactly once.
#[derive(Debug)]
pub struct Teardown {
    phase: Phase,
    user_reported: bool,
    all_reported: bool,
}

impl Default for Teardown {
    fn default() -> Self {
        Teardown { phase: Phase::Running, user_reported: false, all_reported: false }
    }
}

impl Teardown {
    /// Where shutdown is.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether new browsers, popups and prompts are admitted: only while Running.
    pub fn admits(&self) -> bool {
        self.phase == Phase::Running
    }

    /// Whether CEF may still be called.
    pub fn cef_alive(&self) -> bool {
        !matches!(self.phase, Phase::Shutdown | Phase::Stuck)
    }

    /// `begin_shutdown`. True when this call left Running (the caller then closes the user's browsers
    /// and ends the prompts); false when shutdown had already begun (idempotent).
    pub fn begin_shutdown(&mut self) -> bool {
        if self.phase != Phase::Running {
            return false;
        }
        self.phase = Phase::ClosingUser;
        true
    }

    /// `close_internal`. True when this call entered ClosingInternal (the caller then closes the hidden
    /// browsers). From Running it begins shutdown first, so the caller must also run phase one when
    /// `begin_shutdown` returned true.
    pub fn close_internal(&mut self) -> bool {
        match self.phase {
            Phase::Running | Phase::ClosingUser => {
                self.phase = Phase::ClosingInternal;
                true
            }
            _ => false,
        }
    }

    /// The count changed (or a phase began): the reports now due, in order.
    pub fn on_count(&mut self, count: LiveCount) -> Vec<Report> {
        let mut due = Vec::new();
        let closing = matches!(self.phase, Phase::ClosingUser | Phase::ClosingInternal);
        if closing && count.user == 0 && !self.user_reported {
            self.user_reported = true;
            due.push(Report::UserBrowsersClosed);
        }
        if self.phase == Phase::ClosingInternal && count.total() == 0 && !self.all_reported {
            self.all_reported = true;
            self.phase = Phase::Drained;
            due.push(Report::AllBrowsersClosed);
        }
        due
    }

    /// The driver's shutdown: Drained → Shutdown and true, so `cef::shutdown` runs; from any other
    /// phase → Stuck and false, so nothing more is called.
    pub fn shut_down(&mut self) -> bool {
        match self.phase {
            Phase::Drained => {
                self.phase = Phase::Shutdown;
                true
            }
            Phase::Shutdown => false,
            _ => {
                self.phase = Phase::Stuck;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn count(user: u32, internal: u32) -> LiveCount {
        LiveCount { user, internal }
    }

    #[test]
    fn both_phases_report_once_and_in_order() {
        let mut t = Teardown::default();
        assert!(t.admits());
        assert!(t.on_count(count(0, 0)).is_empty(), "nothing is reported while running");
        assert!(t.begin_shutdown());
        assert!(!t.begin_shutdown(), "idempotent");
        assert!(!t.admits());
        assert!(t.on_count(count(2, 1)).is_empty());
        assert_eq!(t.on_count(count(0, 1)), [Report::UserBrowsersClosed]);
        assert!(t.on_count(count(0, 1)).is_empty(), "reported once");
        assert!(t.close_internal());
        assert!(!t.close_internal(), "idempotent");
        assert_eq!(t.phase(), Phase::ClosingInternal);
        assert_eq!(t.on_count(count(0, 0)), [Report::AllBrowsersClosed]);
        assert_eq!(t.phase(), Phase::Drained);
        assert!(t.on_count(count(0, 0)).is_empty());
        assert!(t.shut_down());
        assert_eq!(t.phase(), Phase::Shutdown);
        assert!(!t.cef_alive());
        assert!(!t.shut_down());
    }

    #[test]
    fn close_internal_from_running_reports_both_at_once() {
        let mut t = Teardown::default();
        assert!(t.close_internal());
        assert!(!t.begin_shutdown(), "phase one is part of it");
        assert_eq!(t.on_count(count(0, 0)), [Report::UserBrowsersClosed, Report::AllBrowsersClosed]);
    }

    #[test]
    fn user_phase_with_no_user_browsers_reports_at_once() {
        let mut t = Teardown::default();
        t.begin_shutdown();
        assert_eq!(t.on_count(count(0, 3)), [Report::UserBrowsersClosed]);
        assert_eq!(t.phase(), Phase::ClosingUser, "the hidden browsers keep it from draining");
    }

    #[test]
    fn shutting_down_with_live_browsers_is_stuck() {
        for setup in [0, 1, 2] {
            let mut t = Teardown::default();
            if setup >= 1 {
                t.begin_shutdown();
            }
            if setup >= 2 {
                t.close_internal();
                t.on_count(count(1, 0));
            }
            assert!(!t.shut_down(), "case {setup}");
            assert_eq!(t.phase(), Phase::Stuck);
            assert!(!t.cef_alive());
            assert!(t.on_count(count(0, 0)).is_empty(), "nothing is reported once stuck");
        }
    }
}
