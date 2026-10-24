//! Chrome DevTools Protocol over `--remote-debugging-pipe`, in process, browser target.
//! Each message is JSON terminated by `\0`. The OS layer creates the pipes and hands us our
//! two ends (Linux: fds 3/4 for Chromium; Windows: `--remote-debugging-io-pipes`).
use crate::ctl::Reply;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub struct Cdp {
    tx: Mutex<File>,
    next: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Value>>>,
}

static CDP: OnceLock<Arc<Cdp>> = OnceLock::new();

pub fn get() -> Option<Arc<Cdp>> {
    CDP.get().cloned()
}

/// `writer`: our end Chromium reads commands from; `reader`: our end it writes replies to.
pub fn start(writer: File, reader: File) {
    let cdp = Arc::new(Cdp { tx: Mutex::new(writer), next: AtomicI64::new(1), pending: Mutex::new(HashMap::new()) });
    let c2 = cdp.clone();
    std::thread::Builder::new().name("cdp-reader".into()).spawn(move || c2.read_loop(reader)).ok();
    let _ = CDP.set(cdp);
}

impl Cdp {
    fn read_loop(&self, mut f: File) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = match f.read(&mut chunk) {
                Ok(0) | Err(_) => {
                    crate::log("cdp: pipe closed");
                    return;
                }
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            while let Some(pos) = buf.iter().position(|&b| b == 0) {
                let msg: Vec<u8> = buf.drain(..=pos).collect();
                let text = String::from_utf8_lossy(&msg[..msg.len() - 1]).into_owned();
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                    if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(v);
                    }
                } else {
                    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method.starts_with("Target.targetCreated") || method.starts_with("Target.targetDestroyed") || method.starts_with("Extensions.") {
                        crate::log(&format!("cdp event: {}", trunc(&text, 800)));
                    }
                }
            }
        }
    }

    /// Blocking call; never from the CEF UI thread.
    pub fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        {
            let mut w = self.tx.lock().unwrap();
            let mut bytes = serde_json::to_vec(&msg).unwrap();
            bytes.push(0);
            w.write_all(&bytes).map_err(|e| e.to_string())?;
            let _ = w.flush();
        }
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(v) => match v.get("error") {
                Some(e) => Err(e.to_string()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            },
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err("timeout".into())
            }
        }
    }

    pub fn targets(&self) -> Result<Vec<Value>, String> {
        let r = self.call("Target.getTargets", json!({}), None)?;
        Ok(r.get("targetInfos").and_then(|t| t.as_array()).cloned().unwrap_or_default())
    }

    /// Attach (flat) to the first matching target, run one method there, detach.
    pub fn in_target(&self, pred: &dyn Fn(&Value) -> bool, method: &str, params: Value) -> Result<Value, String> {
        let t = self.targets()?.into_iter().find(|t| pred(t)).ok_or("no matching target")?;
        let r = self.call("Target.attachToTarget", json!({ "targetId": t["targetId"], "flatten": true }), None)?;
        let sid = r["sessionId"].as_str().ok_or("no sessionId")?.to_string();
        let res = self.call(method, params, Some(&sid));
        let _ = self.call("Target.detachFromTarget", json!({ "sessionId": sid }), None);
        res
    }
}

pub fn trunc(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}…(+{} bytes)", &s[..s.floor_char_boundary(n)], s.len() - n) }
}

/// `cdp METHOD [JSON]` (browser session), `cdptargets`, `cdpsw EXTID EXPR` (evaluate in the
/// extension's worker), `cdpin URLPREFIX METHOD [JSON]` (any target by URL prefix).
/// Runs on a worker thread; answers through `reply`.
pub fn ctl(line: String, reply: Reply) {
    std::thread::spawn(move || {
        let Some(c) = get() else {
            reply.send("ERR no pipe (SPIKE_CDP_PIPE=1)");
            return;
        };
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let parse = |s: &str| -> Value { if s.trim().is_empty() { json!({}) } else { serde_json::from_str(s).unwrap_or(json!({})) } };
        let out = match cmd {
            "cdp" => {
                let (m, p) = rest.split_once(' ').unwrap_or((rest, ""));
                c.call(m, parse(p), None)
            }
            "cdptargets" => c.targets().map(|ts| Value::Array(ts.into_iter().map(|t| json!([t["type"], t["url"], t["targetId"]])).collect())),
            "cdpsw" => {
                let (id, expr) = rest.split_once(' ').unwrap_or((rest, "1"));
                let prefix = format!("chrome-extension://{id}/");
                c.in_target(
                    &|t| t["type"] == "service_worker" && t["url"].as_str().is_some_and(|u| u.starts_with(&prefix)),
                    "Runtime.evaluate",
                    json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }),
                )
            }
            "cdpin" => {
                let mut it = rest.splitn(3, ' ');
                let (p, m, j) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
                c.in_target(&|t| t["url"].as_str().is_some_and(|u| u.starts_with(p)), m, parse(j))
            }
            _ => Err(format!("unknown cdp command {cmd}")),
        };
        let text = match out {
            Ok(v) => format!("OK {}", v),
            Err(e) => format!("ERR {e}"),
        };
        crate::log(&format!("cdp> {line} => {}", trunc(&text, 4000)));
        reply.send(text);
    });
}
