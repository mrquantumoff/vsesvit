//! Owned by W1.3.
//! Linux: Chromium's GLib sources (X11, GDK/GtkUi, D-Bus, AT-SPI) live on the default main context,
//! which GPUI never iterates (SC section 4.1, G13). Each pump step services it by hand (acquire,
//! prepare, query, poll 0, check, dispatch, release; its 0 ms prepare timeout is ignored), and the
//! `glib-fd-watch` thread polls the fds GLib asked for and wakes the pump. The watcher stays disarmed
//! until main re-arms it after the next service, so a readable fd wakes the pump once, not in a loop.

use std::os::raw::{c_int, c_ushort, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct GPollFD {
    fd: c_int,
    events: c_ushort,
    revents: c_ushort,
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

/// The fds GLib wants watched after a service.
pub struct Fds(Vec<GPollFD>);

/// One non-blocking iteration of the default context, repeated while sources are ready (at most 32
/// rounds). Main thread.
pub fn service() -> Fds {
    // SAFETY: GLib's documented manual iteration on the default context, on the thread that acquires it.
    unsafe {
        let ctx = g_main_context_default();
        if g_main_context_acquire(ctx) == 0 {
            return Fds(Vec::new());
        }
        let mut fds = vec![GPollFD::default(); 64];
        let mut n;
        let mut rounds = 0;
        loop {
            let mut priority: c_int = 0;
            g_main_context_prepare(ctx, &mut priority);
            let mut timeout: c_int = 0;
            loop {
                n = g_main_context_query(ctx, priority, &mut timeout, fds.as_mut_ptr(), fds.len() as c_int) as usize;
                if n <= fds.len() {
                    break;
                }
                fds.resize(n, GPollFD::default());
            }
            libc::poll(fds.as_mut_ptr().cast::<libc::pollfd>(), n as libc::nfds_t, 0);
            let ready = g_main_context_check(ctx, priority, fds.as_mut_ptr(), n as c_int);
            g_main_context_dispatch(ctx);
            rounds += 1;
            if ready == 0 || rounds >= 32 {
                break;
            }
        }
        g_main_context_release(ctx);
        fds.truncate(n);
        Fds(fds)
    }
}

struct Shared {
    fds: Mutex<(Vec<GPollFD>, bool)>,
    stop: AtomicBool,
    eventfd: c_int,
}

/// The `glib-fd-watch` thread.
pub struct Watcher {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts the thread. It sends a zero delay on `wake` when a watched fd is ready.
    pub fn spawn(wake: UnboundedSender<Duration>) -> std::io::Result<Watcher> {
        // SAFETY: plain eventfd(2).
        let eventfd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if eventfd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let shared = Arc::new(Shared { fds: Mutex::new((Vec::new(), false)), stop: AtomicBool::new(false), eventfd });
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new().name("glib-fd-watch".into()).spawn(move || watch(&thread_shared, &wake))?;
        Ok(Watcher { shared, thread: Some(thread) })
    }

    /// Re-arms the thread with the fds of the last service.
    pub fn arm(&self, fds: Fds) {
        if let Ok(mut state) = self.shared.fds.lock() {
            *state = (fds.0, true);
        }
        self.kick();
    }

    fn kick(&self) {
        let one = 1u64;
        // SAFETY: an 8-byte write to our own eventfd.
        unsafe { libc::write(self.shared.eventfd, (&one as *const u64).cast(), 8) };
    }

    /// Stops and joins the thread (shutdown step 8).
    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.kick();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the thread that polled it has exited.
        unsafe { libc::close(self.shared.eventfd) };
    }
}

fn watch(shared: &Shared, wake: &UnboundedSender<Duration>) {
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let (fds, armed) = shared.fds.lock().map(|s| s.clone()).unwrap_or_default();
        let mut polled = Vec::with_capacity(fds.len() + 1);
        polled.push(libc::pollfd { fd: shared.eventfd, events: libc::POLLIN, revents: 0 });
        if armed {
            polled.extend(fds.iter().map(|f| libc::pollfd { fd: f.fd, events: f.events as i16, revents: 0 }));
        }
        // SAFETY: poll(2) on a buffer we own.
        let ready = unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, -1) };
        if ready < 0 {
            continue;
        }
        if polled[0].revents != 0 {
            let mut buf = 0u64;
            // SAFETY: an 8-byte read from our own eventfd.
            unsafe { libc::read(shared.eventfd, (&mut buf as *mut u64).cast(), 8) };
        }
        if polled[1..].iter().any(|p| p.revents != 0) {
            if let Ok(mut state) = shared.fds.lock() {
                state.1 = false;
            }
            if wake.unbounded_send(Duration::ZERO).is_err() {
                return;
            }
        }
    }
}
