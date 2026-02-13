use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const TWENTY_FOUR_HOURS_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub level: String,
    pub target: String,
    pub message: String,
}

pub struct InMemoryLogger {
    buffer: Mutex<VecDeque<LogEntry>>,
    level: log::LevelFilter,
}

impl InMemoryLogger {
    pub fn init() -> &'static Self {
        let level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(log::LevelFilter::Info);

        let logger = Box::new(Self {
            buffer: Mutex::new(VecDeque::new()),
            level,
        });
        let logger: &'static Self = Box::leak(logger);
        log::set_logger(logger).expect("Failed to set logger");
        log::set_max_level(level);
        logger
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        let mut buf = self.buffer.lock().unwrap();
        Self::prune(&mut buf);
        buf.iter().cloned().collect()
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    fn prune(buf: &mut VecDeque<LogEntry>) {
        let cutoff = Self::now_ms() - TWENTY_FOUR_HOURS_MS;
        while buf.front().is_some_and(|e| e.timestamp < cutoff) {
            buf.pop_front();
        }
    }
}

impl log::Log for InMemoryLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let entry = LogEntry {
            timestamp: Self::now_ms(),
            level: record.level().to_string(),
            target: record.target().to_string(),
            message: format!("{}", record.args()),
        };

        // Print to stderr (like env_logger)
        eprintln!(
            "[{} {} {}] {}",
            entry.level, entry.target, entry.timestamp, entry.message
        );

        let mut buf = self.buffer.lock().unwrap();
        buf.push_back(entry);
        Self::prune(&mut buf);
    }

    fn flush(&self) {}
}
