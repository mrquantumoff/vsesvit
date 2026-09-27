//! A `log` backend: stderr (the console of debug builds) plus `<profile>/vsesvit.log`, which is
//! the only place logs survive in release builds. `VSESVIT_LOG=debug|trace` raises the level.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use log::{LevelFilter, Log, Metadata, Record};

struct Logger {
    start: Instant,
    level: LevelFilter,
    file: Mutex<Option<File>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub(crate) fn init(profile_dir: &Path) {
    let level = match std::env::var("VSESVIT_LOG").as_deref() {
        Ok("trace") => LevelFilter::Trace,
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let file = std::fs::create_dir_all(profile_dir)
        .and_then(|()| File::create(profile_dir.join("vsesvit.log")))
        .inspect_err(|e| eprintln!("vsesvit: no log file in {}: {e}", profile_dir.display()))
        .ok();
    let logger = LOGGER.get_or_init(|| Logger {
        start: Instant::now(),
        level,
        file: Mutex::new(file),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
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
            "[{:>4}.{:03} {:<5} {}] {}\n",
            elapsed.as_secs(),
            elapsed.subsec_millis(),
            record.level(),
            record.target().trim_start_matches("vsesvit_winui::"),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}
