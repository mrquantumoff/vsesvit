//! Owned by W1.6.
//! The pump (design section 3.4): the one caller of `EngineDriver::do_work`, a GPUI foreground task
//! outside any update. It steps when the engine asks (`schedule`'s wakes, from any thread) and at
//! least every 33 ms while the engine is active, every 250 ms after 1 s without a wake, because
//! Chromium's external pump drops its pending delayed work after each slice (SC section 4.2). The OS
//! work of a step is the driver's.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, BackgroundExecutor, Task};
use vsesvit_engine::EngineDriver;

/// The longest wait between steps while the engine is active.
pub const ACTIVE_CAP: Duration = Duration::from_millis(33);
/// The longest wait once the engine has been quiet for [`IDLE_AFTER`].
pub const IDLE_CAP: Duration = Duration::from_millis(250);
/// How long without a wake makes the engine quiet.
pub const IDLE_AFTER: Duration = Duration::from_secs(1);

/// When the next step is due.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cadence {
    due: Instant,
    last_wake: Instant,
}

impl Cadence {
    /// The first step runs at once.
    pub fn new(now: Instant) -> Cadence {
        Cadence { due: now, last_wake: now }
    }

    pub fn due(&self) -> Instant {
        self.due
    }

    /// The engine asked for a step after `delay`; never later than the active cap.
    pub fn wake(&mut self, delay: Duration, now: Instant) {
        self.last_wake = now;
        self.due = self.due.min(now + delay.min(ACTIVE_CAP));
    }

    /// A step runs now; the next one is due at the cap, unless a wake brings it forward. `queued` are
    /// the wakes that arrived since the last wait: this step serves the immediate ones, and the
    /// shortest positive delay among them still asks for a later step.
    pub fn stepped(&mut self, now: Instant, queued: &[Duration]) {
        if !queued.is_empty() {
            self.last_wake = now;
        }
        let cap = if now.saturating_duration_since(self.last_wake) > IDLE_AFTER { IDLE_CAP } else { ACTIVE_CAP };
        self.due = now + cap;
        if let Some(&later) = queued.iter().filter(|d| !d.is_zero()).min() {
            self.wake(later, now);
        }
    }
}

/// Where the driver lives while the pump runs. The pump borrows it for each step; whoever started the
/// pump keeps a clone, so the driver outlives the pump task (design section 3.6, "Who owns the driver
/// at the end").
pub type DriverSlot = Rc<RefCell<Option<Box<dyn EngineDriver>>>>;

/// The running pump. Shutdown step 7 stops it (design section 3.6).
pub struct PumpHandle {
    task: Task<()>,
    driver: DriverSlot,
}

impl PumpHandle {
    /// Ends the pump after its current step and leaves the driver in its slot. Unlike [`stop`](Self::stop)
    /// it works mid-step too: the task only ends once the step returns.
    pub fn halt(self) {
        drop(self.task);
    }

    /// Ends the pump after its current step and hands back the driver, for `EngineDriver::shutdown`
    /// once GPUI's `run` returned. Called mid-step, which only a call from inside an engine callback
    /// could be (section 1.6 rule 1 forbids it), the pump keeps running and comes back as the error.
    pub fn stop(self) -> Result<Box<dyn EngineDriver>, PumpHandle> {
        let taken = match self.driver.try_borrow_mut() {
            Ok(mut driver) => driver.take(),
            Err(_) => None,
        };
        match taken {
            Some(driver) => {
                drop(self.task);
                Ok(driver)
            }
            None => {
                log::error!("pump: stop() during a pump step; the pump keeps the driver");
                Err(self)
            }
        }
    }
}

/// Starts the pump task over the driver in `driver`. With none there, the pump never steps.
pub(crate) fn start(cx: &mut App, driver: DriverSlot) -> PumpHandle {
    let Some(wakes) = driver.borrow_mut().as_mut().map(|d| d.schedule()) else {
        log::error!("pump: no driver to pump");
        return PumpHandle { task: Task::ready(()), driver };
    };
    let clock = cx.background_executor().clone();
    let task = cx.spawn({
        let driver = driver.clone();
        async move |_| run(driver, wakes, clock).await
    });
    PumpHandle { task, driver }
}

async fn run(driver: DriverSlot, mut wakes: UnboundedReceiver<Duration>, clock: BackgroundExecutor) {
    let mut cadence = Cadence::new(clock.now());
    let mut wakes_open = true;
    loop {
        let now = clock.now();
        if cadence.due() > now {
            let timer = clock.timer(cadence.due() - now).fuse();
            if wakes_open {
                futures::pin_mut!(timer);
                futures::select_biased! {
                    wake = wakes.next() => match wake {
                        Some(delay) => cadence.wake(delay, clock.now()),
                        None => wakes_open = false,
                    },
                    () = timer => {}
                }
            } else {
                timer.await;
            }
            continue;
        }
        let mut queued = Vec::new();
        while let Ok(delay) = wakes.try_recv() {
            queued.push(delay);
        }
        cadence.stepped(now, &queued);
        let Ok(held) = driver.try_borrow() else { return };
        let Some(step) = held.as_ref() else { return };
        step.do_work();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn steps_every_33_ms_while_active_and_every_250_ms_once_quiet() {
        let t0 = Instant::now();
        let mut c = Cadence::new(t0);
        assert_eq!(c.due(), t0, "the first step runs at once");
        c.stepped(t0, &[]);
        assert_eq!(c.due(), t0 + ACTIVE_CAP);
        let quiet = t0 + IDLE_AFTER + MS;
        c.stepped(quiet, &[]);
        assert_eq!(c.due(), quiet + IDLE_CAP);
    }

    #[test]
    fn a_wake_brings_the_step_forward_but_never_past_the_active_cap() {
        let t0 = Instant::now();
        let mut c = Cadence::new(t0);
        c.stepped(t0 + IDLE_AFTER + MS, &[]);
        let now = t0 + IDLE_AFTER + 10 * MS;
        c.wake(5 * MS, now);
        assert_eq!(c.due(), now + 5 * MS);
        c.wake(Duration::from_secs(10), now);
        assert_eq!(c.due(), now + 5 * MS, "a later wake does not postpone an earlier step");
        let quiet = t0 + IDLE_AFTER + MS;
        let mut c = Cadence::new(t0);
        c.stepped(quiet, &[]);
        assert_eq!(c.due(), quiet + IDLE_CAP);
        c.wake(100 * MS, quiet);
        assert_eq!(c.due(), quiet + ACTIVE_CAP, "a long delay is capped");
        c.stepped(quiet + ACTIVE_CAP, &[]);
        assert_eq!(c.due(), quiet + 2 * ACTIVE_CAP, "a wake makes the engine active again");
        let now = quiet + 2 * ACTIVE_CAP;
        c.stepped(now, &[Duration::ZERO, 10 * MS, 20 * MS]);
        assert_eq!(c.due(), now + 10 * MS, "a delayed wake that queued before the step still gets its step");
    }
}
