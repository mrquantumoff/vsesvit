//! Drives CEF's UI message loop from GPUI's main thread (`external_message_pump`).
//!
//! On Windows there is no GLib: Chromium's HWNDs live on this thread and GPUI's own
//! `GetMessageW`/`DispatchMessageW` loop delivers their messages. What remains is
//! calling `do_message_loop_work` when CEF asks (`on_schedule_message_pump_work`, any
//! thread, forwarded over a futures channel that wakes GPUI's foreground executor) and
//! polling at a cap, because `MessagePumpExternal` drops the next delayed-run time when
//! its slice ends (spike-core.md section 4.2; the code is platform independent).
use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt as _, StreamExt as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct PumpStats {
    pub cef_work: AtomicU64,
    pub loop_wakes: AtomicU64,
    pub ticks: AtomicU64,
    pub imm_count: AtomicU64,
    pub imm_total_us: AtomicU64,
    pub imm_max_us: AtomicU64,
    pub work_total_us: AtomicU64,
    pub work_max_us: AtomicU64,
}

impl PumpStats {
    pub fn summary(&self) -> String {
        let r = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let n = r(&self.imm_count).max(1);
        let w = r(&self.cef_work).max(1);
        format!(
            "immediate_requests={} latency_avg={}us latency_max={}us work_calls={} work_avg={}us work_max={}us loop_wakes={} timer_ticks={}",
            r(&self.imm_count),
            r(&self.imm_total_us) / n,
            r(&self.imm_max_us),
            r(&self.cef_work),
            r(&self.work_total_us) / w,
            r(&self.work_max_us),
            r(&self.loop_wakes),
            r(&self.ticks)
        )
    }
}

pub struct PumpConfig {
    /// Cap on how long CEF work may wait while active (CEF's sample uses 1000/30 ms). 0 = none.
    pub cef_max_delay: Duration,
    /// After 1 s without CEF requests, cap at this instead. 0 = disabled.
    pub cef_idle_delay: Duration,
}

impl PumpConfig {
    pub fn from_env() -> Self {
        let ms = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Self { cef_max_delay: Duration::from_millis(ms("SPIKE_PUMP_MAX_MS", 33)), cef_idle_delay: Duration::from_millis(ms("SPIKE_PUMP_IDLE_MS", 250)) }
    }
}

/// The single caller of `do_message_loop_work`; runs until `stop` or the channel closes.
pub fn start(cx: &mut gpui::App, mut rx: UnboundedReceiver<i64>, stats: Arc<PumpStats>, stop: Arc<AtomicBool>) -> gpui::Task<()> {
    let cfg = PumpConfig::from_env();
    crate::log(&format!("pump: cef_max_delay={:?} idle_delay={:?}", cfg.cef_max_delay, cfg.cef_idle_delay));
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_cx| {
        let mut cef_due: Option<Instant> = Some(Instant::now());
        let mut imm_since: Option<Instant> = None;
        let mut last_activity = Instant::now();
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let now = Instant::now();
            let wake_at = cef_due.unwrap_or(now + Duration::from_secs(3600));
            let wait = wake_at.saturating_duration_since(now);
            if !wait.is_zero() {
                let mut timer = executor.timer(wait).fuse();
                futures::select_biased! {
                    msg = rx.next() => match msg {
                        Some(delay) => {
                            stats.loop_wakes.fetch_add(1, Ordering::Relaxed);
                            last_activity = Instant::now();
                            if delay <= 0 && imm_since.is_none() { imm_since = Some(Instant::now()); }
                            cef_due = Some(schedule(delay, &cfg));
                            while let Ok(Some(d)) = rx.try_recv().map(Some) {
                                if d <= 0 && imm_since.is_none() { imm_since = Some(Instant::now()); }
                                cef_due = Some(schedule(d, &cfg));
                            }
                        }
                        None => break,
                    },
                    _ = timer => { stats.ticks.fetch_add(1, Ordering::Relaxed); }
                }
            } else {
                while let Ok(Some(d)) = rx.try_recv().map(Some) {
                    cef_due = Some(schedule(d, &cfg));
                }
            }
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if cef_due.is_some_and(|d| d <= Instant::now()) {
                let idle = !cfg.cef_idle_delay.is_zero() && last_activity.elapsed() > Duration::from_secs(1);
                let cap = if idle { cfg.cef_idle_delay } else { cfg.cef_max_delay };
                cef_due = if cap.is_zero() { None } else { Some(Instant::now() + cap) };
                stats.cef_work.fetch_add(1, Ordering::Relaxed);
                if let Some(t) = imm_since.take() {
                    let us = t.elapsed().as_micros() as u64;
                    stats.imm_count.fetch_add(1, Ordering::Relaxed);
                    stats.imm_total_us.fetch_add(us, Ordering::Relaxed);
                    stats.imm_max_us.fetch_max(us, Ordering::Relaxed);
                }
                let t0 = Instant::now();
                cef::do_message_loop_work();
                let us = t0.elapsed().as_micros() as u64;
                stats.work_total_us.fetch_add(us, Ordering::Relaxed);
                stats.work_max_us.fetch_max(us, Ordering::Relaxed);
            }
        }
        crate::log("pump: stopped");
    })
}

fn schedule(delay_ms: i64, cfg: &PumpConfig) -> Instant {
    let mut d = Duration::from_millis(delay_ms.max(0) as u64);
    if !cfg.cef_max_delay.is_zero() {
        d = d.min(cfg.cef_max_delay);
    }
    Instant::now() + d
}

/// Pump synchronously (no GPUI) until `done()` or the timeout; used after GPUI's loop
/// has exited, before `cef::shutdown`.
pub fn pump_blocking(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        cef::do_message_loop_work();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}
