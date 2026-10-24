//! Line-oriented control channel on 127.0.0.1 (`SPIKE_CTL_PORT`), shared by both shells.
//! Every line gets exactly one reply line: the shell answers through `Reply` when the command
//! is done (CDP and DevTools results arrive later). `@N <command>` waits up to N seconds
//! (default 30). The log file stays the full record; the reply is what scripts branch on.
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use std::io::{BufRead as _, Write as _};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

#[derive(Clone)]
pub struct Reply(Sender<String>);

impl Reply {
    pub fn send(&self, s: impl Into<String>) {
        let _ = self.0.send(s.into().replace(['\n', '\r'], " "));
    }
    /// A reply nobody waits for (commands issued from inside the shell).
    pub fn none() -> Self {
        Reply(channel().0)
    }
}

pub struct CtlMsg {
    pub line: String,
    pub reply: Reply,
}

pub fn spawn() -> Option<UnboundedReceiver<CtlMsg>> {
    let port: u16 = std::env::var("SPIKE_CTL_PORT").ok()?.parse().ok()?;
    let listener = match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            crate::log(&format!("ctl: bind 127.0.0.1:{port} failed: {e}"));
            return None;
        }
    };
    let (tx, rx) = unbounded::<CtlMsg>();
    std::thread::Builder::new()
        .name("spike-ctl".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let Ok(mut w) = stream.try_clone() else { return };
                    for line in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                        let mut line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let mut wait = 30u64;
                        if let Some(rest) = line.strip_prefix('@') {
                            if let Some((n, cmd)) = rest.split_once(' ') {
                                wait = n.parse().unwrap_or(30);
                                line = cmd.to_string();
                            }
                        }
                        let (rtx, rrx) = channel();
                        if tx.unbounded_send(CtlMsg { line, reply: Reply(rtx) }).is_err() {
                            return;
                        }
                        let answer = rrx.recv_timeout(Duration::from_secs(wait)).unwrap_or_else(|_| "TIMEOUT".into());
                        if w.write_all(format!("{answer}\n").as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        })
        .ok()?;
    crate::log(&format!("ctl: listening on 127.0.0.1:{port}"));
    Some(rx)
}
