//! Chrome DevTools Protocol over `--remote-debugging-pipe`, in process.
//!
//! Chromium reads commands from fd 3 and writes replies/events to fd 4, each message a JSON
//! text terminated by `\0`. The pipe connection is attached to the *browser* target, which is
//! what `Extensions.loadUnpacked` and friends require. No TCP port is opened, so no other
//! local process can reach it.
//!
//! `setup_pipe()` must run before `cef::initialize` and before anything else could own fds 3/4.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
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

fn cloexec_dup(fd: i32) -> i32 {
    unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 100) }
}

/// Create the two pipes and put Chromium's ends on fds 3 (read) and 4 (write).
/// Our ends are CLOEXEC so sub-processes never inherit them.
pub fn setup_pipe() -> bool {
    let mut a = [0i32; 2]; // commands: we write a[1], Chromium reads fd 3
    let mut b = [0i32; 2]; // replies: Chromium writes fd 4, we read b[0]
    unsafe {
        if libc::pipe2(a.as_mut_ptr(), libc::O_CLOEXEC) != 0 || libc::pipe2(b.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return false;
        }
        // Move every end above 100 first, so dup2 onto 3/4 never collides with an end.
        let (ar, aw, br, bw) = (cloexec_dup(a[0]), cloexec_dup(a[1]), cloexec_dup(b[0]), cloexec_dup(b[1]));
        for fd in [a[0], a[1], b[0], b[1]] {
            libc::close(fd);
        }
        // dup2 clears FD_CLOEXEC on the target: fds 3 and 4 are inheritable by design (CEF
        // reads them in this process; Chromium does not pass them to children).
        if libc::dup2(ar, 3) != 3 || libc::dup2(bw, 4) != 4 {
            return false;
        }
        libc::close(ar);
        libc::close(bw);
        let cdp = Arc::new(Cdp { tx: Mutex::new(File::from_raw_fd(aw)), next: AtomicI64::new(1), pending: Mutex::new(HashMap::new()) });
        let reader = File::from_raw_fd(br);
        let c2 = cdp.clone();
        std::thread::Builder::new().name("cdp-reader".into()).spawn(move || c2.read_loop(reader)).ok();
        let _ = CDP.set(cdp);
    }
    true
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
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    crate::log(&format!("cdp: bad json {text}"));
                    continue;
                };
                if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                    if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(v);
                    }
                } else {
                    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    // Log the events that matter for extension lifecycle; skip noise.
                    if method.starts_with("Target.") || method.starts_with("Extensions.") || method == "Runtime.consoleAPICalled" {
                        crate::log(&format!("cdp event: {}", trunc(&text, 1500)));
                    }
                }
            }
        }
    }

    /// Blocking call (never call from the GPUI/CEF UI thread).
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
        }
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(v) => {
                if let Some(e) = v.get("error") {
                    Err(e.to_string())
                } else {
                    Ok(v.get("result").cloned().unwrap_or(Value::Null))
                }
            }
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

    /// Attach (flat session) to the first target whose type/url match, evaluate, detach.
    pub fn eval_in(&self, pred: &dyn Fn(&Value) -> bool, expr: &str, gesture: bool) -> Result<Value, String> {
        let t = self.targets()?.into_iter().find(|t| pred(t)).ok_or("no matching target")?;
        let tid = t["targetId"].as_str().unwrap_or("").to_string();
        let r = self.call("Target.attachToTarget", json!({ "targetId": tid, "flatten": true }), None)?;
        let sid = r["sessionId"].as_str().ok_or("no sessionId")?.to_string();
        let res = self.call(
            "Runtime.evaluate",
            json!({ "expression": expr, "awaitPromise": true, "returnByValue": true, "userGesture": gesture }),
            Some(&sid),
        );
        let _ = self.call("Target.detachFromTarget", json!({ "sessionId": sid }), None);
        res
    }
}

pub fn trunc(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}…(+{} bytes)", &s[..s.floor_char_boundary(n)], s.len() - n) }
}

/// Control commands that talk CDP, run on a worker thread so the GPUI/CEF thread never blocks.
/// `cdp METHOD [JSON]`, `cdps SESSION METHOD [JSON]`, `targets`, `sweval EXTID EXPR`,
/// `pageeval URLPREFIX EXPR`, `extpageeval EXTID EXPR`.
pub fn ctl(line: String) {
    std::thread::spawn(move || {
        let Some(c) = get() else {
            crate::log("cdp: no pipe (start with SPIKE_CDP_PIPE=1)");
            return;
        };
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let parse = |s: &str| -> Value { if s.trim().is_empty() { json!({}) } else { serde_json::from_str(s).unwrap_or(json!({})) } };
        let out = match cmd {
            "cdp" => {
                let (m, p) = rest.split_once(' ').unwrap_or((rest, ""));
                c.call(m, parse(p), None)
            }
            "cdps" => {
                let mut it = rest.splitn(3, ' ');
                let (s, m, p) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
                c.call(m, parse(p), Some(s))
            }
            "targets" => c.targets().map(|ts| {
                Value::Array(
                    ts.into_iter()
                        .map(|t| json!([t["type"], t["url"], t["title"], t["targetId"], t["browserContextId"], t["attached"]]))
                        .collect(),
                )
            }),
            "sweval" => {
                let (id, expr) = rest.split_once(' ').unwrap_or((rest, "1"));
                let prefix = format!("chrome-extension://{id}/");
                c.eval_in(&|t| t["type"] == "service_worker" && t["url"].as_str().is_some_and(|u| u.starts_with(&prefix)), expr, false)
            }
            "swcdp" | "pagecdp" => {
                // swcdp EXTID METHOD [JSON]: attach to the extension's service worker, call METHOD
                // in that session. pagecdp URLPREFIX METHOD [JSON]: same for a page target.
                let mut it = rest.splitn(3, ' ');
                let (key, m, p) = (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string());
                let sw = cmd == "swcdp";
                let prefix = if sw { format!("chrome-extension://{key}/") } else { key.clone() };
                (|| {
                    let t = c.targets()?.into_iter().find(|t| t["type"] == if sw { "service_worker" } else { "page" } && t["url"].as_str().is_some_and(|u| u.starts_with(&prefix))).ok_or("no matching target")?;
                    let r = c.call("Target.attachToTarget", json!({ "targetId": t["targetId"], "flatten": true }), None)?;
                    let sid = r["sessionId"].as_str().ok_or("no sessionId")?.to_string();
                    let res = c.call(&m, parse(&p), Some(&sid));
                    let _ = c.call("Target.detachFromTarget", json!({ "sessionId": sid }), None);
                    res
                })()
            }
            "extpageeval" => {
                let (id, expr) = rest.split_once(' ').unwrap_or((rest, "1"));
                let prefix = format!("chrome-extension://{id}/");
                c.eval_in(&|t| t["type"] == "page" && t["url"].as_str().is_some_and(|u| u.starts_with(&prefix)), expr, true)
            }
            "pageeval" => {
                let (p, expr) = rest.split_once(' ').unwrap_or((rest, "1"));
                c.eval_in(&|t| t["type"] == "page" && t["url"].as_str().is_some_and(|u| u.starts_with(p)), expr, true)
            }
            _ => Err(format!("unknown cdp ctl {cmd}")),
        };
        match out {
            Ok(v) => crate::log(&format!("cdp> {line} => OK {}", trunc(&v.to_string(), 6000))),
            Err(e) => crate::log(&format!("cdp> {line} => ERR {}", trunc(&e, 3000))),
        }
    });
}
