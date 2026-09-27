//! A single-threaded executor on the UI thread's `DispatcherQueue`.
//!
//! WinRT async operations implement `IntoFuture`, but their completion handlers must be `Send`
//! and may run on any thread, while all shell state lives on the UI thread. Futures spawned
//! here are polled only on the UI thread, so they can hold `Rc` state; wakers only carry a
//! task id and an agile `DispatcherQueue`, and every poll is enqueued (never run inline).
//!
//! A poll can itself pump messages (an outgoing cross-process COM call from an STA does), so a
//! queued poll of the same task may run while that task is still being polled. Such a wake is
//! recorded on the running slot and the task is polled again once the current poll ends.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use crate::bindings::{DispatcherQueue, DispatcherQueueHandler};

type Task = Pin<Box<dyn Future<Output = ()>>>;

enum Slot {
    Idle(Task),
    Running { woken: bool },
}

thread_local! {
    static QUEUE: RefCell<Option<DispatcherQueue>> = const { RefCell::new(None) };
    static TASKS: RefCell<HashMap<u64, Slot>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

/// Binds the executor to the calling thread's dispatcher. Call once on the UI thread.
pub(crate) fn init() -> windows_core::Result<()> {
    let queue = DispatcherQueue::GetForCurrentThread()?;
    QUEUE.with_borrow_mut(|q| *q = Some(queue));
    Ok(())
}

/// Runs `future` on the UI thread. It first runs on a later dispatcher turn, never inside the
/// caller, so spawning from an event handler cannot re-enter the caller's state.
pub(crate) fn spawn(future: impl Future<Output = ()> + 'static) {
    let Some(queue) = QUEUE.with_borrow(Clone::clone) else {
        log::error!("exec::spawn before exec::init; task dropped");
        return;
    };
    let id = NEXT_ID.replace(NEXT_ID.get() + 1);
    TASKS.with_borrow_mut(|tasks| tasks.insert(id, Slot::Idle(Box::pin(future))));
    schedule(&queue, id);
}

/// Drops every pending task and detaches from the dispatcher; later `spawn`s are ignored.
pub(crate) fn shutdown() {
    let tasks = TASKS.with_borrow_mut(std::mem::take);
    drop(tasks);
    QUEUE.with_borrow_mut(Option::take);
}

fn schedule(queue: &DispatcherQueue, id: u64) {
    let handler = DispatcherQueueHandler::new(move || poll(id));
    if !matches!(queue.TryEnqueue(&handler), Ok(true)) {
        log::warn!("dispatcher is shutting down; task {id} dropped");
        let dropped = TASKS.with_borrow_mut(|tasks| tasks.remove(&id));
        drop(dropped);
    }
}

fn poll(id: u64) {
    let task = TASKS.with_borrow_mut(|tasks| match tasks.get_mut(&id) {
        Some(Slot::Running { woken }) => {
            *woken = true;
            None
        }
        Some(slot) => match std::mem::replace(slot, Slot::Running { woken: false }) {
            Slot::Idle(task) => Some(task),
            Slot::Running { .. } => None,
        },
        None => None,
    });
    let (Some(mut task), Some(queue)) = (task, QUEUE.with_borrow(Clone::clone)) else {
        return;
    };
    let waker = Waker::from(Arc::new(TaskWaker {
        id,
        queue: queue.clone(),
    }));
    let pending = task
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending();
    // A finished future is dropped outside the borrow: its drop may run XAML code that
    // re-enters the executor.
    let mut finished = None;
    let woken_meanwhile = TASKS.with_borrow_mut(|tasks| {
        let woken = matches!(tasks.get(&id), Some(Slot::Running { woken: true }));
        if pending {
            tasks.insert(id, Slot::Idle(task));
        } else {
            tasks.remove(&id);
            finished = Some(task);
        }
        woken
    });
    drop(finished);
    if pending && woken_meanwhile {
        schedule(&queue, id);
    }
}

struct TaskWaker {
    id: u64,
    queue: DispatcherQueue,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let id = self.id;
        let _ = self
            .queue
            .TryEnqueue(&DispatcherQueueHandler::new(move || poll(id)));
    }
}

/// Runs `work` on a new thread; the returned future completes on the UI thread with its result.
/// This is how slow `Send` work (network, disk) reports back to UI-thread state.
pub(crate) fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Background<T> {
    let handoff = Arc::new(Mutex::new(Handoff {
        value: None,
        waker: None,
    }));
    let worker = handoff.clone();
    std::thread::spawn(move || {
        let value = work();
        let waker = {
            let mut handoff = worker.lock().unwrap_or_else(PoisonError::into_inner);
            handoff.value = Some(value);
            handoff.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    Background { handoff }
}

struct Handoff<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

pub(crate) struct Background<T> {
    handoff: Arc<Mutex<Handoff<T>>>,
}

impl<T> Future for Background<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut handoff = self.handoff.lock().unwrap_or_else(PoisonError::into_inner);
        match handoff.value.take() {
            Some(value) => Poll::Ready(value),
            None => {
                handoff.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// The UI thread's dispatcher, for a worker thread to `post` to.
pub(crate) fn dispatcher() -> Option<DispatcherQueue> {
    QUEUE.with_borrow(Clone::clone)
}

/// Runs `f` on the dispatcher's thread. Callable from any thread.
pub(crate) fn post(queue: &DispatcherQueue, f: impl Fn() + 'static) {
    let _ = queue.TryEnqueue(&DispatcherQueueHandler::new(f));
}

/// Completes after `duration`, without blocking the UI thread.
pub(crate) fn sleep(duration: Duration) -> Sleep {
    Sleep {
        deadline: Instant::now() + duration,
        armed: None,
    }
}

pub(crate) struct Sleep {
    deadline: Instant,
    armed: Option<Arc<AtomicBool>>,
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Poll::Ready(());
        }
        if self
            .armed
            .as_ref()
            .is_none_or(|fired| fired.load(Ordering::Acquire))
        {
            let fired = Arc::new(AtomicBool::new(false));
            let waker = cx.waker().clone();
            let flag = fired.clone();
            std::thread::spawn(move || {
                std::thread::sleep(remaining);
                flag.store(true, Ordering::Release);
                waker.wake();
            });
            self.armed = Some(fired);
        }
        Poll::Pending
    }
}

/// `future`'s output, or `None` if it takes longer than `limit`. Engine operations on a web view
/// that closes meanwhile never complete, so scripted waits on them need a bound.
pub(crate) async fn timeout<T>(limit: Duration, future: impl Future<Output = T>) -> Option<T> {
    let mut future = std::pin::pin!(future);
    let mut timer = std::pin::pin!(sleep(limit));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(value) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(value));
        }
        timer.as_mut().poll(cx).map(|()| None)
    })
    .await
}

/// Polls `condition` on the UI thread every `step` until it returns `Some` or `timeout` passes.
pub(crate) async fn wait_for<T>(
    timeout: Duration,
    step: Duration,
    mut condition: impl FnMut() -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = condition() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        sleep(step).await;
    }
}
