//! Line-oriented control channel for driving the spike from a shell:
//! `SPIKE_CTL=/path/fifo`; `echo "new https://example.org" > /path/fifo`.
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use std::io::BufRead as _;

pub fn spawn() -> Option<UnboundedReceiver<String>> {
    let path = std::env::var("SPIKE_CTL").ok()?;
    let _ = std::fs::remove_file(&path);
    let c = std::ffi::CString::new(path.clone()).ok()?;
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        crate::log(&format!("ctl: mkfifo {path} failed"));
        return None;
    }
    let (tx, rx) = unbounded();
    std::thread::Builder::new()
        .name("spike-ctl".into())
        .spawn(move || loop {
            // Opening blocks until a writer appears; EOF when it closes, so reopen.
            let Ok(f) = std::fs::File::open(&path) else { return };
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                let line = line.trim().to_string();
                if !line.is_empty() && tx.unbounded_send(line).is_err() {
                    return;
                }
            }
        })
        .ok()?;
    crate::log(&format!("ctl: listening on {}", std::env::var("SPIKE_CTL").unwrap_or_default()));
    Some(rx)
}
