//! Line-oriented control channel on a localhost TCP port (`SPIKE_CTL_PORT`), the
//! Windows stand-in for the Linux spike's FIFO. Each line is acknowledged with `ok`
//! once it has been queued; the log file is the oracle for what happened.
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use std::io::{BufRead as _, Write as _};

pub fn spawn() -> Option<UnboundedReceiver<String>> {
    let port: u16 = std::env::var("SPIKE_CTL_PORT").ok()?.parse().ok()?;
    let listener = match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            crate::log(&format!("ctl: bind 127.0.0.1:{port} failed: {e}"));
            return None;
        }
    };
    let (tx, rx) = unbounded();
    std::thread::Builder::new()
        .name("spike-ctl".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut w = match stream.try_clone() {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                for line in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if tx.unbounded_send(line).is_err() {
                        return;
                    }
                    let _ = w.write_all(b"ok\n");
                }
            }
        })
        .ok()?;
    crate::log(&format!("ctl: listening on 127.0.0.1:{port}"));
    Some(rx)
}
