//! Toolkit-agnostic external-message-pump core for `Settings.external_message_pump = 1`.
use cef::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::time::{Duration, Instant};

#[link(name = "glib-2.0")]
extern "C" {
    // libcef already depends on glib; Chromium's UI pump on Linux is MessagePumpGlib
    // and registers its fd watches on g_main_context_default().
    fn g_main_context_iteration(context: *mut std::ffi::c_void, may_block: i32) -> i32;
}

/// Called from BrowserProcessHandler::on_schedule_message_pump_work (ANY thread).
#[derive(Clone)]
pub struct PumpHandle(pub Sender<i64>);

impl PumpHandle {
    pub fn schedule(&self, delay_ms: i64) {
        let _ = self.0.send(delay_ms);
        // A real UI toolkit integration must also wake its event loop here
        // (e.g. a calloop ping / GPUI foreground channel).
    }
}

/// Lives on the main (CEF UI) thread.
pub struct Pump {
    rx: Receiver<i64>,
    deadline: Option<Instant>,
    active: AtomicBool,
}

impl Pump {
    pub fn new(rx: Receiver<i64>) -> Self {
        Self { rx, deadline: None, active: AtomicBool::new(false) }
    }

    /// Call whenever the toolkit wakes up on the main thread. Returns the time at
    /// which it should be called again (None = only when woken by `schedule`).
    pub fn tick(&mut self) -> Option<Instant> {
        // Latest request wins: a new delay cancels the pending one (CEF contract).
        while let Ok(delay) = self.rx.try_recv() {
            let at = Instant::now() + Duration::from_millis(delay.max(0) as u64);
            self.deadline = Some(at);
        }
        // Dispatch Chromium's glib sources (X11/Wayland/D-Bus fds) without blocking.
        unsafe { while g_main_context_iteration(std::ptr::null_mut(), 0) != 0 {} }

        match self.deadline {
            Some(at) if at <= Instant::now() => {
                self.deadline = None;
                if !self.active.swap(true, Ordering::SeqCst) {
                    do_message_loop_work(); // may re-enter schedule() via callbacks
                    self.active.store(false, Ordering::SeqCst);
                }
                // Never sleep more than ~1/30 s (same cap as cef-rs tests_shared).
                Some(self.deadline.unwrap_or(Instant::now() + Duration::from_millis(33)))
            }
            other => other,
        }
    }
}
