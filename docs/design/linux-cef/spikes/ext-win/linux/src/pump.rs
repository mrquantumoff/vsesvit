//! Drives CEF's UI message loop and Chromium's GLib sources from GPUI's main thread.
//!
//! * CEF calls `on_schedule_message_pump_work(delay_ms)` from any thread; the handler
//!   pushes `delay_ms` into a futures channel, which wakes the GPUI foreground task
//!   below (GPUI's dispatcher pings its calloop loop when a foreground task is woken).
//! * Chromium's X11 connection, GTK/GDK and other fd watches are GSources on
//!   `g_main_context_default()`, which GPUI never iterates. A watcher thread polls the
//!   fds that GLib reports from `g_main_context_query` and wakes the same task.
//!   GLib's own timeout cannot be used: Chromium's work source (state_ == null under the
//!   external pump) always reports a 0 ms timeout. A slow GLib tick covers pure GLib timers.
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::{FutureExt as _, StreamExt as _};
use std::os::raw::{c_int, c_uint, c_ushort, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct GPollFD {
    pub fd: c_int,
    pub events: c_ushort,
    pub revents: c_ushort,
}

#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_main_context_default() -> *mut c_void;
    fn g_main_context_acquire(ctx: *mut c_void) -> c_int;
    fn g_main_context_release(ctx: *mut c_void);
    fn g_main_context_prepare(ctx: *mut c_void, priority: *mut c_int) -> c_int;
    fn g_main_context_query(ctx: *mut c_void, max_priority: c_int, timeout: *mut c_int, fds: *mut GPollFD, n_fds: c_int) -> c_int;
    fn g_main_context_check(ctx: *mut c_void, max_priority: c_int, fds: *mut GPollFD, n_fds: c_int) -> c_int;
    fn g_main_context_dispatch(ctx: *mut c_void);
}

#[derive(Default)]
pub struct PumpStats {
    pub cef_work: AtomicU64,
    pub glib_rounds: AtomicU64,
    pub glib_dispatch: AtomicU64,
    pub glib_wakes: AtomicU64,
    pub loop_wakes: AtomicU64,
    pub ticks: AtomicU64,
    /// Latency from an immediate (delay <= 0) request reaching the pump task to the
    /// do_message_loop_work call that serves it.
    pub imm_count: AtomicU64,
    pub imm_total_us: AtomicU64,
    pub imm_max_us: AtomicU64,
    /// Time spent inside do_message_loop_work.
    pub work_total_us: AtomicU64,
    pub work_max_us: AtomicU64,
}

impl PumpStats {
    pub fn summary(&self) -> String {
        let r = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let n = r(&self.imm_count).max(1);
        let w = r(&self.cef_work).max(1);
        format!(
            "immediate_requests={} latency_avg={}us latency_max={}us work_calls={} work_avg={}us work_max={}us",
            r(&self.imm_count), r(&self.imm_total_us) / n, r(&self.imm_max_us), r(&self.cef_work), r(&self.work_total_us) / w, r(&self.work_max_us)
        )
    }
}

/// One manual, non-blocking GLib iteration (prepare/query/poll(0)/check/dispatch),
/// repeated while sources are ready. Returns the fd set GLib wants watched.
fn glib_service(stats: &PumpStats) -> Vec<GPollFD> {
    unsafe {
        let ctx = g_main_context_default();
        if g_main_context_acquire(ctx) == 0 {
            return Vec::new();
        }
        let mut fds = vec![GPollFD::default(); 64];
        let mut n;
        let mut rounds = 0;
        loop {
            let mut prio: c_int = 0;
            g_main_context_prepare(ctx, &mut prio);
            let mut timeout: c_int = 0;
            loop {
                n = g_main_context_query(ctx, prio, &mut timeout, fds.as_mut_ptr(), fds.len() as c_int) as usize;
                if n <= fds.len() {
                    break;
                }
                fds.resize(n, GPollFD::default());
            }
            libc::poll(fds.as_mut_ptr() as *mut libc::pollfd, n as libc::nfds_t, 0);
            let ready = g_main_context_check(ctx, prio, fds.as_mut_ptr(), n as c_int);
            g_main_context_dispatch(ctx);
            stats.glib_rounds.fetch_add(1, Ordering::Relaxed);
            rounds += 1;
            if ready == 0 || rounds >= 32 {
                break;
            }
            stats.glib_dispatch.fetch_add(1, Ordering::Relaxed);
        }
        g_main_context_release(ctx);
        fds.truncate(n);
        fds
    }
}

/// Thread that blocks in poll(2) on GLib's fds and wakes the GPUI task when one is ready.
struct GlibWatcher {
    state: Arc<Mutex<(Vec<GPollFD>, bool)>>, // (fds, armed)
    eventfd: c_int,
}

impl GlibWatcher {
    fn spawn(wake: UnboundedSender<()>, stats: Arc<PumpStats>) -> Self {
        let eventfd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        let state: Arc<Mutex<(Vec<GPollFD>, bool)>> = Arc::new(Mutex::new((Vec::new(), false)));
        let st = state.clone();
        std::thread::Builder::new()
            .name("glib-fd-watch".into())
            .spawn(move || loop {
                let (mut fds, armed) = {
                    let s = st.lock().unwrap();
                    (s.0.clone(), s.1)
                };
                let mut pfds: Vec<libc::pollfd> = Vec::with_capacity(fds.len() + 1);
                pfds.push(libc::pollfd { fd: eventfd, events: libc::POLLIN, revents: 0 });
                if armed {
                    for f in fds.iter_mut() {
                        pfds.push(libc::pollfd { fd: f.fd, events: f.events as i16, revents: 0 });
                    }
                }
                let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, -1) };
                if r < 0 {
                    continue;
                }
                if pfds[0].revents != 0 {
                    let mut buf = 0u64;
                    unsafe { libc::read(eventfd, &mut buf as *mut u64 as *mut c_void, 8) };
                }
                if pfds[1..].iter().any(|p| p.revents != 0) {
                    // Disarm until the main thread has iterated GLib (fds may stay readable).
                    st.lock().unwrap().1 = false;
                    stats.glib_wakes.fetch_add(1, Ordering::Relaxed);
                    if wake.unbounded_send(()).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn glib watcher");
        Self { state, eventfd }
    }

    fn arm(&self, fds: Vec<GPollFD>) {
        *self.state.lock().unwrap() = (fds, true);
        let one = 1u64;
        unsafe { libc::write(self.eventfd, &one as *const u64 as *const c_void, 8) };
    }
}

pub struct PumpConfig {
    /// Cap on how long CEF work may wait (CEF's sample uses 1000/30 ms). 0 = no cap.
    pub cef_max_delay: Duration,
    /// Slow GLib tick for pure GLib timers (GTK dialogs, IME). 0 = none.
    pub glib_tick: Duration,
    /// Adaptive back-off: after 1 s without CEF requests or X/GLib input, cap at this
    /// instead of `cef_max_delay`. 0 = disabled.
    pub cef_idle_delay: Duration,
}

impl PumpConfig {
    pub fn from_env() -> Self {
        let ms = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Self {
            cef_max_delay: Duration::from_millis(ms("SPIKE_PUMP_MAX_MS", 33)),
            glib_tick: Duration::from_millis(ms("SPIKE_GLIB_TICK_MS", 250)),
            cef_idle_delay: Duration::from_millis(ms("SPIKE_PUMP_IDLE_MS", 250)),
        }
    }
}

/// Starts the pump as a GPUI foreground task. Runs until `stop` is set or the
/// channel closes. Never call `do_message_loop_work` re-entrantly: this single task
/// is the only caller.
pub fn start(cx: &mut gpui::App, mut rx: UnboundedReceiver<i64>, stats: Arc<PumpStats>, stop: Arc<std::sync::atomic::AtomicBool>) -> gpui::Task<()> {
    let cfg = PumpConfig::from_env();
    crate::log(&format!("pump: cef_max_delay={:?} glib_tick={:?} idle_delay={:?}", cfg.cef_max_delay, cfg.glib_tick, cfg.cef_idle_delay));
    let (wake_tx, mut wake_rx) = unbounded::<()>();
    let watcher = GlibWatcher::spawn(wake_tx, stats.clone());
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_cx| {
        // CEF work deadline (None = nothing scheduled).
        let mut cef_due: Option<Instant> = Some(Instant::now());
        let mut glib_due = Instant::now();
        let mut imm_since: Option<Instant> = None;
        let mut last_activity = Instant::now();
        // Experiment switch: without the fd watcher, X input to CEF windows waits for the next tick.
        let no_watch = std::env::var("SPIKE_NO_GLIB_WATCH").is_ok();
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let now = Instant::now();
            let mut wake_at = if cfg.glib_tick.is_zero() { now + Duration::from_secs(3600) } else { glib_due };
            if let Some(d) = cef_due {
                wake_at = wake_at.min(d);
            }
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
                            // Latest request wins (CEF contract): drain the rest.
                            while let Ok(Some(d)) = rx.try_recv().map(Some) {
                                if d <= 0 && imm_since.is_none() { imm_since = Some(Instant::now()); }
                                cef_due = Some(schedule(d, &cfg));
                            }
                        }
                        None => break,
                    },
                    _ = wake_rx.next() => { stats.loop_wakes.fetch_add(1, Ordering::Relaxed); last_activity = Instant::now(); }
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
            // Always service GLib first: X11/GDK events may produce CEF work.
            let fds = glib_service(&stats);
            glib_due = Instant::now() + cfg.glib_tick;
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
                // Work may have been scheduled from inside; it is in `rx` already.
            }
            if !no_watch {
                watcher.arm(fds);
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

/// Pump synchronously (no GPUI) until `done()` or the timeout: used for shutdown after
/// GPUI's loop has exited.
pub fn pump_blocking(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let stats = PumpStats::default();
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        glib_service(&stats);
        cef::do_message_loop_work();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

#[allow(dead_code)]
fn _assert_types(_: c_uint) {}
