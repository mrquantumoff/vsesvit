//! Long-lived message channels (`runtime.connect`, `tabs.connect` and their `Port`s) and
//! who may message an extension from another one. Platform-neutral: the runtime keeps one
//! [`Ports`], tells it what the contexts did, and answers each [`Wake`] it gets back.
//!
//! A channel joins the opener's port to one port per context that has an `onConnect`
//! listener, as in Chrome: what the opener posts reaches every receiver, what a receiver
//! posts reaches the opener, and the opener's `onDisconnect` fires once every receiver is
//! gone (with [`NO_RECEIVER`] when none ever accepted). A context collects its events by
//! keeping one `port.receive` call open per port, so a port works in any frame and world
//! the call came from; the runtime parks that call here as a waiter `W` until there is
//! something to deliver.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use vsesvit_core::extensions::manifest::Manifest;

use crate::protocol::NO_RECEIVER;

/// What `port.postMessage` on a closed port throws in Chrome.
pub const DISCONNECTED: &str = "Attempting to use a disconnected port object";

#[derive(Clone, Debug, PartialEq)]
pub enum PortEvent {
    Message(Value),
    /// The other side went away; the text is what `runtime.lastError` shows meanwhile.
    Disconnect(Option<String>),
}

impl PortEvent {
    /// The wire form `port.receive` resolves with a list of.
    pub fn to_json(&self) -> Value {
        match self {
            PortEvent::Message(m) => json!({ "m": m }),
            PortEvent::Disconnect(e) => json!({ "d": e }),
        }
    }
}

/// A parked `port.receive` call and the events it resolves with.
pub type Wake<W> = (W, Vec<PortEvent>);

struct End<C, W> {
    channel: u64,
    context: C,
    inbox: Vec<PortEvent>,
    waiter: Option<W>,
    /// Out of its channel, with a [`PortEvent::Disconnect`] queued; removed once that is
    /// delivered.
    closed: bool,
}

struct Channel {
    opener: String,
    /// Ports handed to contexts that have not said yet whether they listen.
    offered: BTreeSet<String>,
    joined: BTreeSet<String>,
    /// Every context the connection goes to has been offered a port.
    sealed: bool,
    accepted: bool,
}

pub struct Ports<C, W> {
    ends: BTreeMap<String, End<C, W>>,
    channels: BTreeMap<u64, Channel>,
    next: u64,
}

impl<C, W> Default for Ports<C, W> {
    fn default() -> Self {
        Ports { ends: BTreeMap::new(), channels: BTreeMap::new(), next: 1 }
    }
}

impl<C: PartialEq, W> Ports<C, W> {
    /// The opener's port `id` (chosen by the opener) starts a channel.
    pub fn open(&mut self, id: &str, context: C) -> Result<(), String> {
        if self.ends.contains_key(id) {
            return Err(format!("port {id} is already open"));
        }
        let channel = self.take_number();
        self.channels.insert(channel, Channel { opener: id.to_owned(), offered: BTreeSet::new(), joined: BTreeSet::new(), sealed: false, accepted: false });
        self.ends.insert(id.to_owned(), End { channel, context, inbox: Vec::new(), waiter: None, closed: false });
        Ok(())
    }

    /// A port for one context the connection goes to, which joins the channel if that
    /// context accepts (see [`Ports::answer`]). `None` once the opener has gone.
    pub fn offer(&mut self, opener: &str, context: C) -> Option<String> {
        let channel = self.ends.get(opener).filter(|e| !e.closed)?.channel;
        let id = format!("r{}", self.take_number());
        self.channels.get_mut(&channel)?.offered.insert(id.clone());
        self.ends.insert(id.clone(), End { channel, context, inbox: Vec::new(), waiter: None, closed: false });
        Some(id)
    }

    /// Every context has been offered a port: with none of them left, the opener is told.
    pub fn seal(&mut self, opener: &str) -> Vec<Wake<W>> {
        let Some(channel) = self.ends.get(opener).filter(|e| !e.closed).map(|e| e.channel) else { return Vec::new() };
        if let Some(c) = self.channels.get_mut(&channel) {
            c.sealed = true;
        }
        self.finish(channel)
    }

    /// Whether the context offered `port` had an `onConnect` listener.
    pub fn answer(&mut self, port: &str, accepted: bool) -> Vec<Wake<W>> {
        let Some(channel) = self.ends.get(port).map(|e| e.channel) else { return Vec::new() };
        let Some(c) = self.channels.get_mut(&channel) else {
            // The opener left meanwhile; an accepting context still hears of it.
            if !accepted {
                self.ends.remove(port);
            }
            return Vec::new();
        };
        if !c.offered.remove(port) {
            return Vec::new();
        }
        if accepted {
            c.joined.insert(port.to_owned());
            c.accepted = true;
        } else {
            self.ends.remove(port);
        }
        self.finish(channel)
    }

    /// `port.postMessage` from `context`, which must own `from`.
    pub fn post(&mut self, from: &str, context: &C, message: Value) -> Result<Vec<Wake<W>>, String> {
        let end = self.ends.get(from).filter(|e| !e.closed && e.context == *context).ok_or(DISCONNECTED)?;
        let c = self.channels.get(&end.channel).ok_or(DISCONNECTED)?;
        let to: Vec<String> = if c.opener == from { c.offered.iter().chain(&c.joined).cloned().collect() } else { vec![c.opener.clone()] };
        Ok(to.iter().filter_map(|id| self.deliver(id, PortEvent::Message(message.clone()))).collect())
    }

    /// `port.disconnect()` from `context`, which must own `port`.
    pub fn disconnect(&mut self, port: &str, context: &C) -> Vec<Wake<W>> {
        if !self.ends.get(port).is_some_and(|e| e.context == *context) {
            return Vec::new();
        }
        self.leave(port)
    }

    /// `port.receive` from `context`: the queued events now, or `waiter` parked until
    /// there are some. A port that is gone (or not the caller's) reads as disconnected.
    pub fn receive(&mut self, port: &str, context: &C, waiter: W) -> Option<Wake<W>> {
        let Some(end) = self.ends.get_mut(port).filter(|e| e.context == *context) else {
            return Some((waiter, vec![PortEvent::Disconnect(None)]));
        };
        if end.inbox.is_empty() {
            end.waiter = Some(waiter);
            return None;
        }
        Some((waiter, self.drain(port)))
    }

    /// Contexts that went away (a document unloaded, a tab or view closed, an extension
    /// unloaded) lose their ports, as if each had called `disconnect()`.
    pub fn close_where(&mut self, gone: impl Fn(&C) -> bool) -> Vec<Wake<W>> {
        let ids: Vec<String> = self.ends.iter().filter(|(_, e)| gone(&e.context)).map(|(id, _)| id.clone()).collect();
        ids.iter().flat_map(|id| self.leave(id)).collect()
    }

    fn take_number(&mut self) -> u64 {
        let n = self.next;
        self.next += 1;
        n
    }

    fn leave(&mut self, port: &str) -> Vec<Wake<W>> {
        let Some(end) = self.ends.remove(port) else { return Vec::new() };
        let mut wakes: Vec<Wake<W>> = end.waiter.map(|w| (w, vec![PortEvent::Disconnect(None)])).into_iter().collect();
        if end.closed {
            return wakes;
        }
        let Some(c) = self.channels.get_mut(&end.channel) else { return wakes };
        if c.opener == port {
            let receivers: Vec<String> = c.offered.iter().chain(&c.joined).cloned().collect();
            self.channels.remove(&end.channel);
            wakes.extend(receivers.iter().filter_map(|r| self.close(r, None)));
        } else {
            // Only a context that accepted knows its port's id.
            if c.offered.remove(port) {
                c.accepted = true;
            }
            c.joined.remove(port);
            wakes.extend(self.finish(end.channel));
        }
        wakes
    }

    /// The opener of a sealed channel without receivers is told it is alone.
    fn finish(&mut self, channel: u64) -> Vec<Wake<W>> {
        let Some(c) = self.channels.get(&channel) else { return Vec::new() };
        if !c.sealed || !c.offered.is_empty() || !c.joined.is_empty() {
            return Vec::new();
        }
        let error = (!c.accepted).then(|| NO_RECEIVER.to_owned());
        let opener = c.opener.clone();
        self.channels.remove(&channel);
        self.close(&opener, error).into_iter().collect()
    }

    fn close(&mut self, port: &str, error: Option<String>) -> Option<Wake<W>> {
        self.ends.get_mut(port)?.closed = true;
        self.deliver(port, PortEvent::Disconnect(error))
    }

    fn deliver(&mut self, port: &str, event: PortEvent) -> Option<Wake<W>> {
        let end = self.ends.get_mut(port)?;
        end.inbox.push(event);
        let waiter = end.waiter.take()?;
        Some((waiter, self.drain(port)))
    }

    fn drain(&mut self, port: &str) -> Vec<PortEvent> {
        let Some(end) = self.ends.get_mut(port) else { return Vec::new() };
        let events = std::mem::take(&mut end.inbox);
        if end.closed {
            self.ends.remove(port);
        }
        events
    }
}

/// Whether `manifest`'s extension takes messages and connections from the extension
/// `caller`, as Chrome decides it: from every extension without `externally_connectable`,
/// else only from those its `ids` name (`"*"` for all).
pub fn accepts_extension(manifest: &Manifest, caller: &str) -> bool {
    match manifest.raw.get("externally_connectable") {
        None | Some(Value::Null) => true,
        Some(declared) => declared.get("ids").and_then(Value::as_array).is_some_and(|ids| ids.iter().any(|id| id == "*" || id == caller)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Table = Ports<&'static str, u32>;

    fn message(v: i32) -> PortEvent {
        PortEvent::Message(json!(v))
    }

    /// The events each woken waiter got, by waiter.
    fn woke(wakes: Vec<Wake<u32>>) -> Vec<(u32, Vec<PortEvent>)> {
        let mut wakes = wakes;
        wakes.sort_by_key(|(w, _)| *w);
        wakes
    }

    fn connected(targets: &[&'static str]) -> (Table, Vec<String>) {
        let mut ports = Table::default();
        ports.open("o", "content").unwrap();
        let ids: Vec<String> = targets.iter().map(|t| ports.offer("o", *t).unwrap()).collect();
        assert!(ports.seal("o").is_empty());
        (ports, ids)
    }

    #[test]
    fn a_connection_nobody_listens_for_disconnects_the_opener_with_chromes_error() {
        let mut ports = Table::default();
        ports.open("o", "content").unwrap();
        assert!(ports.seal("o").is_empty(), "no waiter yet: the event is queued");
        assert_eq!(ports.receive("o", &"content", 1), Some((1, vec![PortEvent::Disconnect(Some(NO_RECEIVER.to_owned()))])));
        assert_eq!(ports.receive("o", &"content", 2), Some((2, vec![PortEvent::Disconnect(None)])), "and it is gone");

        let (mut ports, ids) = connected(&["background", "popup"]);
        assert!(ports.receive("o", &"content", 1).is_none(), "parked");
        assert!(ports.answer(&ids[0], false).is_empty());
        assert_eq!(woke(ports.answer(&ids[1], false)), vec![(1, vec![PortEvent::Disconnect(Some(NO_RECEIVER.to_owned()))])]);
    }

    #[test]
    fn the_opener_reaches_every_receiver_and_each_receiver_reaches_the_opener() {
        let (mut ports, ids) = connected(&["background", "popup"]);
        // Posted before anyone answered: kept for the receivers that accept.
        assert!(ports.post("o", &"content", json!(1)).unwrap().is_empty());
        assert!(ports.answer(&ids[0], true).is_empty());
        assert!(ports.answer(&ids[1], true).is_empty());
        assert_eq!(ports.receive(&ids[0], &"background", 10), Some((10, vec![message(1)])));
        assert!(ports.receive(&ids[1], &"popup", 11).is_some());
        assert!(ports.receive(&ids[0], &"background", 10).is_none());
        assert!(ports.receive(&ids[1], &"popup", 11).is_none());
        assert_eq!(woke(ports.post("o", &"content", json!(2)).unwrap()), vec![(10, vec![message(2)]), (11, vec![message(2)])]);

        assert!(ports.receive("o", &"content", 1).is_none());
        assert_eq!(woke(ports.post(&ids[1], &"popup", json!(3)).unwrap()), vec![(1, vec![message(3)])]);
        assert!(ports.post(&ids[1], &"background", json!(4)).is_err(), "only the owning context posts on a port");
    }

    #[test]
    fn the_opener_hears_of_the_disconnect_once_every_receiver_is_gone() {
        let (mut ports, ids) = connected(&["background", "popup"]);
        ports.answer(&ids[0], true);
        ports.answer(&ids[1], true);
        assert!(ports.receive("o", &"content", 1).is_none());
        assert!(ports.disconnect(&ids[0], &"background").is_empty());
        assert_eq!(woke(ports.disconnect(&ids[1], &"popup")), vec![(1, vec![PortEvent::Disconnect(None)])]);
        assert_eq!(ports.post("o", &"content", json!(1)), Err(DISCONNECTED.to_owned()));
    }

    #[test]
    fn the_opener_disconnecting_reaches_every_receiver_after_what_it_posted() {
        let (mut ports, ids) = connected(&["background", "popup"]);
        ports.answer(&ids[0], true);
        assert!(ports.receive(&ids[0], &"background", 10).is_none());
        ports.post("o", &"content", json!(1)).unwrap();
        assert!(ports.receive(&ids[0], &"background", 10).is_none());
        assert_eq!(woke(ports.disconnect("o", &"content")), vec![(10, vec![PortEvent::Disconnect(None)])]);
        // The popup accepts after the opener left: it still gets the message, then the end.
        assert!(ports.answer(&ids[1], true).is_empty());
        assert_eq!(ports.receive(&ids[1], &"popup", 11), Some((11, vec![message(1), PortEvent::Disconnect(None)])));
        assert!(ports.ends.is_empty() && ports.channels.is_empty(), "nothing left behind");
    }

    #[test]
    fn a_receiver_that_leaves_inside_its_listener_counts_as_having_accepted() {
        let (mut ports, ids) = connected(&["background"]);
        assert!(ports.receive("o", &"content", 1).is_none());
        assert_eq!(woke(ports.disconnect(&ids[0], &"background")), vec![(1, vec![PortEvent::Disconnect(None)])]);
        assert!(ports.answer(&ids[0], true).is_empty());
    }

    #[test]
    fn a_context_that_goes_away_closes_its_ports() {
        let (mut ports, ids) = connected(&["background", "popup"]);
        ports.answer(&ids[0], true);
        ports.answer(&ids[1], true);
        assert!(ports.receive(&ids[0], &"background", 10).is_none());
        assert!(ports.receive(&ids[1], &"popup", 11).is_none());
        let wakes = woke(ports.close_where(|c| *c == "content"));
        assert_eq!(wakes, vec![(10, vec![PortEvent::Disconnect(None)]), (11, vec![PortEvent::Disconnect(None)])]);
        assert!(ports.ends.is_empty() && ports.channels.is_empty());
    }

    #[test]
    fn an_opener_that_leaves_before_the_offers_stops_them() {
        let mut ports = Table::default();
        ports.open("o", "content").unwrap();
        ports.disconnect("o", &"content");
        assert_eq!(ports.offer("o", "background"), None);
        assert!(ports.seal("o").is_empty());
        assert!(ports.open("x", "content").is_ok());
        assert!(ports.open("x", "content").is_err(), "a port id is used once");
    }

    #[test]
    fn events_have_a_wire_form() {
        assert_eq!(message(1).to_json(), json!({ "m": 1 }));
        assert_eq!(PortEvent::Disconnect(None).to_json(), json!({ "d": null }));
        assert_eq!(PortEvent::Disconnect(Some("x".into())).to_json(), json!({ "d": "x" }));
    }

    #[test]
    fn externally_connectable_names_the_extensions_that_may_connect() {
        let manifest = |extra: &str| Manifest::parse(&format!(r#"{{"manifest_version":3,"name":"x","version":"1"{extra}}}"#), &|_| None).unwrap();
        assert!(accepts_extension(&manifest(""), "anyone"));
        let listed = manifest(r#","externally_connectable":{"ids":["friend"]}"#);
        assert!(accepts_extension(&listed, "friend") && !accepts_extension(&listed, "stranger"));
        assert!(accepts_extension(&manifest(r#","externally_connectable":{"ids":["*"]}"#), "stranger"));
        assert!(!accepts_extension(&manifest(r#","externally_connectable":{"matches":["https://x.test/*"]}"#), "friend"), "no ids: no extension");
    }
}
