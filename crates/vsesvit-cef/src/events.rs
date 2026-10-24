//! Owned by W1.3.
//! The event sink: an unbounded sender of `BrowserEvent` that any thread may use (design section 1.6).
//! Handlers never reach the shell any other way; the shell drains the receiver on main after each pump
//! step, never inside an engine call.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use vsesvit_engine::{BrowserEvent, TabEvent, TabId};

/// The sending half. `Clone + Send + Sync`, so handlers on any thread hold one.
#[derive(Clone, Debug)]
pub struct Sink(UnboundedSender<BrowserEvent>);

impl Sink {
    /// A sink and the receiver the shell drains.
    pub fn new() -> (Sink, UnboundedReceiver<BrowserEvent>) {
        let (tx, rx) = unbounded();
        (Sink(tx), rx)
    }

    /// Queues an event. A closed receiver means the shell is gone; the event is dropped.
    pub fn send(&self, event: BrowserEvent) {
        let _ = self.0.unbounded_send(event);
    }

    /// Queues an event about one tab.
    pub fn tab(&self, tab: TabId, event: TabEvent) {
        self.send(BrowserEvent::Tab(tab, event));
    }
}
