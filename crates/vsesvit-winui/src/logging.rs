//! A `log` backend: stderr (the console of debug builds) plus a log file (`<profile>/vsesvit.log`,
//! or `<out_dir>/vsesvit.log` for the self-test), which is the only place logs survive in release
//! builds. `VSESVIT_LOG=debug|trace` raises the level.
//!
//! Lines are held in memory until the process knows whether it owns the profile: the owner
//! starts a new file, while a launch that forwards to the running instance appends to that
//! instance's file instead of wiping it.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
    path: PathBuf,
    sink: Mutex<Sink>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileMode {
    /// This process owns the profile: a new log.
    Truncate,
    /// Another process owns it: add to its log.
    Append,
}

pub(crate) fn init(log_file: &Path) {
    let level = match std::env::var("VSESVIT_LOG").as_deref() {
        Ok("trace") => LevelFilter::Trace,
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let logger = LOGGER.get_or_init(|| Logger {
        start: Instant::now(),
        level,
        path: log_file.to_owned(),
        sink: Mutex::new(Sink::Pending(Vec::new())),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Starts writing the log file, beginning with the lines logged so far.
pub(crate) fn open_file(mode: FileMode) {
    let Some(logger) = LOGGER.get() else { return };
    let opened = logger
        .path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| match mode {
            FileMode::Truncate => File::create(&logger.path),
            FileMode::Append => OpenOptions::new()
                .create(true)
                .append(true)
                .open(&logger.path),
        });
    let mut sink = logger.sink.lock().unwrap_or_else(PoisonError::into_inner);
    let pending = match std::mem::replace(&mut *sink, Sink::Nowhere) {
        Sink::Pending(lines) => lines,
        other => {
            *sink = other;
            return;
        }
    };
    match opened {
        Ok(mut file) => {
            for line in pending {
                let _ = file.write_all(line.as_bytes());
            }
            *sink = Sink::File(file);
        }
        Err(e) => eprintln!("vsesvit: no log file {}: {e}", logger.path.display()),
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
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
            record.target().trim_start_matches("vsesvit_winui::"),
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
