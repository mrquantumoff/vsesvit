//! Owned by W1.4.
//! Boot step 1 (design section 3.1): one `log` backend on both OSes, as WinUI has. Lines go to stderr
//! and to a log file, `<profile>/vsesvit.log` (`OUT_DIR/vsesvit.log` for the self-test), which is the
//! only place they survive in a Windows release build. `VSESVIT_LOG=warn|debug|trace` changes our
//! crates' level from info; other crates log warnings only, and everything at `trace`.
//!
//! Lines are held in memory until the process knows whether it owns the profile: the owner starts a
//! new file, while a launch that forwards to the running owner appends to that owner's file instead
//! of wiping it.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use log::{LevelFilter, Log, Metadata, Record};

enum Sink {
    Pending(Vec<String>),
    File(File),
    Nowhere,
}

struct Logger {
    start: Instant,
    level: LevelFilter,
    others: LevelFilter,
    sink: Mutex<Sink>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// How [`open_file`] opens the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileMode {
    /// This process owns the profile: a new log.
    Truncate,
    /// Another process owns it: add to its log.
    Append,
}

/// Installs the logger: stderr at once, the file once [`open_file`] names it. Starts no thread.
pub fn init() {
    let level = match std::env::var("VSESVIT_LOG").as_deref() {
        Ok("trace") => LevelFilter::Trace,
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let others = if level == LevelFilter::Trace { LevelFilter::Trace } else { LevelFilter::Warn };
    let logger = LOGGER.get_or_init(|| Logger { start: Instant::now(), level, others, sink: Mutex::new(Sink::Pending(Vec::new())) });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Starts writing `path`, beginning with the lines logged so far. Only the first call counts.
pub fn open_file(path: &Path, mode: FileMode) {
    let Some(logger) = LOGGER.get() else { return };
    let mut sink = logger.sink.lock().unwrap_or_else(PoisonError::into_inner);
    let Sink::Pending(pending) = std::mem::replace(&mut *sink, Sink::Nowhere) else { return };
    // Always in append mode, so the owner's lines never overwrite those a forwarding launch adds.
    let opened = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| if mode == FileMode::Truncate { File::create(path).map(drop) } else { Ok(()) })
        .and_then(|()| OpenOptions::new().create(true).append(true).open(path));
    match opened {
        Ok(mut file) => {
            for line in pending {
                let _ = file.write_all(line.as_bytes());
            }
            *sink = Sink::File(file);
        }
        Err(e) => eprintln!("vsesvit: no log file {}: {e}", path.display()),
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let ours = metadata.target().starts_with("vsesvit");
        metadata.level() <= if ours { self.level } else { self.others }
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let elapsed = self.start.elapsed();
        let line = format!(
            "[{:>4}.{:03} {:<5} {} {}] {}\n",
            elapsed.as_secs(),
            elapsed.subsec_millis(),
            record.level(),
            std::process::id(),
            record.target().trim_start_matches("vsesvit_gpui::"),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        match &mut *self.sink.lock().unwrap_or_else(PoisonError::into_inner) {
            Sink::Pending(lines) => lines.push(line),
            Sink::File(file) => {
                let _ = file.write_all(line.as_bytes());
            }
            Sink::Nowhere => {}
        }
    }

    fn flush(&self) {
        if let Sink::File(file) = &mut *self.sink.lock().unwrap_or_else(PoisonError::into_inner) {
            let _ = file.flush();
        }
    }
}
