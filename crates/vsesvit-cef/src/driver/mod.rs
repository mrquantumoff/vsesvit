//! Owned by W1.3.
//! `CefDriver`, the engine's `EngineDriver` (design section 3.4): the wakes CEF asks for
//! (`on_schedule_message_pump_work`, any thread) and, on Linux, the GLib fd watcher's; one pump step
//! (`do_work`: GLib first on Linux, then `do_message_loop_work`); and shutdown step 8. The timing
//! policy (33/250 ms) is the shell's (`bridge/pump.rs`).

#[cfg(target_os = "linux")]
pub mod glib;

use std::time::{Duration, Instant};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use vsesvit_engine::EngineDriver;

use crate::registry;
use crate::teardown::Phase;

/// The driver.
pub struct CefDriver {
    wakes: Option<UnboundedReceiver<Duration>>,
    #[cfg(target_os = "linux")]
    watcher: Option<glib::Watcher>,
}

impl CefDriver {
    /// A driver whose wakes arrive on `wakes`; `wake` is their sender, for the GLib watcher.
    pub fn new(wake: UnboundedSender<Duration>, wakes: UnboundedReceiver<Duration>) -> CefDriver {
        #[cfg(target_os = "linux")]
        let watcher = match glib::Watcher::spawn(wake) {
            Ok(w) => Some(w),
            Err(e) => {
                log::error!("vsesvit-cef: glib-fd-watch did not start ({e}); GLib work waits for the pump's poll");
                None
            }
        };
        #[cfg(not(target_os = "linux"))]
        let _ = wake;
        CefDriver {
            wakes: Some(wakes),
            #[cfg(target_os = "linux")]
            watcher,
        }
    }

    fn step(&self) {
        #[cfg(target_os = "linux")]
        let fds = glib::service();
        cef::do_message_loop_work();
        #[cfg(target_os = "linux")]
        if let Some(watcher) = &self.watcher {
            watcher.arm(fds);
        }
    }
}

impl EngineDriver for CefDriver {
    fn schedule(&mut self) -> UnboundedReceiver<Duration> {
        self.wakes.take().unwrap_or_else(|| {
            log::error!("vsesvit-cef: the driver's wakes were taken twice");
            unbounded().1
        })
    }

    fn do_work(&self) {
        if registry::with(|r| r.teardown.cef_alive()) == Some(true) {
            self.step();
        }
    }

    /// Shutdown step 8, only once drained: stop and join `glib-fd-watch`, pump for 200 ms, empty the
    /// registry (dropping every CEF reference), then `cef::shutdown` on main. With browsers still
    /// alive this is the stuck path: the live entries are logged and no CEF call is made (G16).
    fn shutdown(self: Box<Self>) {
        let drained = registry::with(|r| {
            if r.teardown.phase() == Phase::Drained {
                Ok(())
            } else {
                r.teardown.shut_down();
                Err(r.live_report())
            }
        });
        match drained {
            Some(Ok(())) => {}
            Some(Err(live)) => {
                log::error!("vsesvit-cef: shutdown with {} live browsers; cef::shutdown skipped", live.len());
                for line in live {
                    log::error!("vsesvit-cef: still alive: {line}");
                }
                return;
            }
            None => {
                log::error!("vsesvit-cef: shutdown without the registry; cef::shutdown skipped");
                return;
            }
        }
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut this = *self;
        #[cfg(target_os = "linux")]
        if let Some(watcher) = this.watcher.take() {
            watcher.stop();
        }
        let end = Instant::now() + Duration::from_millis(200);
        while Instant::now() < end {
            #[cfg(target_os = "linux")]
            glib::service();
            cef::do_message_loop_work();
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(mut registry) = registry::take() {
            registry.teardown.shut_down();
            drop(registry);
        }
        drop(this);
        let started = Instant::now();
        cef::shutdown();
        log::info!("vsesvit-cef: cef::shutdown took {:?}", started.elapsed());
    }
}
