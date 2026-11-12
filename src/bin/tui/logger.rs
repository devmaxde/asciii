//! Logger for builds with the `tui` feature.
//!
//! Wraps the exact same `env_logger` configuration as `asciii::util::setup_log`.
//! While the TUI owns the screen, records are kept in a ring buffer (shown in the log view)
//! instead of being written to stderr, which would garble the alternate screen.
//! Outside the TUI (every CLI command) it behaves exactly like `setup_log`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use log::{Level, Log, Metadata, Record};

const CAPACITY: usize = 2000;

/// Records go into the buffer.
static RECORD: AtomicBool = AtomicBool::new(false);
/// The TUI owns the screen, do not write to stderr.
static SCREEN: AtomicBool = AtomicBool::new(false);
static LINES: Mutex<VecDeque<LogLine>> = Mutex::new(VecDeque::new());

#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: DateTime<Local>,
    pub level: Level,
    pub message: String,
}

struct TuiLogger {
    inner: env_logger::Logger,
}

impl Log for TuiLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.inner.matches(record) {
            return;
        }
        if RECORD.load(Ordering::Relaxed) {
            if let Ok(mut lines) = LINES.lock() {
                if lines.len() >= CAPACITY {
                    lines.pop_front();
                }
                lines.push_back(LogLine {
                    time: Local::now(),
                    level: record.level(),
                    message: record.args().to_string(),
                });
            }
        }
        if !SCREEN.load(Ordering::Relaxed) {
            self.inner.log(record);
        }
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// Installs the logger, replaces `asciii::util::setup_log()`.
pub fn init() {
    let inner = asciii::util::log_builder().build();
    let filter = inner.filter();
    if log::set_boxed_logger(Box::new(TuiLogger { inner })).is_ok() {
        log::set_max_level(filter);
    }
}

/// Start keeping records for the log view.
pub fn start_recording() {
    RECORD.store(true, Ordering::Relaxed);
}

/// `true` while the TUI draws on the alternate screen; records are then not written to stderr.
pub fn set_screen(owned_by_tui: bool) {
    SCREEN.store(owned_by_tui, Ordering::Relaxed);
}

/// Copy of the recorded lines, oldest first.
pub fn lines() -> Vec<LogLine> {
    LINES.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

/// Number of recorded warnings and errors.
pub fn problem_count() -> usize {
    LINES
        .lock()
        .map(|l| l.iter().filter(|line| line.level <= Level::Warn).count())
        .unwrap_or(0)
}
