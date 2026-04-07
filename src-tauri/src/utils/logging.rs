use std::fs::{create_dir_all, remove_file, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::constants::MAX_LOG_FILE_BYTES;

static LOGGER: FileLogger = FileLogger;
static LOG_FILE: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();
static CURRENT_LEVEL: AtomicUsize = AtomicUsize::new(LevelFilter::Error as usize);

struct FileLogger;

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level().to_level_filter() <= current_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let Some(file) = LOG_FILE.get() else {
            return;
        };

        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("{ts} [{:>5}] {}\n", record.level(), record.args());
        if let Ok(mut guard) = file.lock() {
            let _ = guard.write_all(line.as_bytes());
            if record.level() <= Level::Warn {
                let _ = guard.flush();
            }
        }
    }

    fn flush(&self) {
        if let Some(file) = LOG_FILE.get() {
            if let Ok(mut guard) = file.lock() {
                let _ = guard.flush();
            }
        }
    }
}

pub fn init(log_path: &Path, level: &str) -> Result<(), String> {
    if LOG_FILE.get().is_none() {
        let parent = log_path
            .parent()
            .ok_or_else(|| "log file path has no parent directory".to_string())?;
        create_dir_all(parent).map_err(|e| format!("failed to create log directory: {e}"))?;
        let file = open_log_file(log_path)?;
        let _ = LOG_FILE.set(Mutex::new(BufWriter::new(file)));
        log::set_logger(&LOGGER).map_err(|e| format!("failed to install logger: {e}"))?;
        log::set_max_level(LevelFilter::Debug);
    }

    set_level(level);
    log::info!(
        "Logger initialized: level={}, path={}",
        sanitize_level(level),
        log_path.display()
    );
    Ok(())
}

pub fn set_level(level: &str) {
    CURRENT_LEVEL.store(sanitize_level(level) as usize, Ordering::Relaxed);
}

pub fn sanitize_level(level: &str) -> LevelFilter {
    match level.trim().to_ascii_lowercase().as_str() {
        "silent" => LevelFilter::Off,
        "error" => LevelFilter::Error,
        "warning" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        _ => LevelFilter::Error,
    }
}

fn current_level() -> LevelFilter {
    match CURRENT_LEVEL.load(Ordering::Relaxed) {
        x if x == LevelFilter::Off as usize => LevelFilter::Off,
        x if x == LevelFilter::Error as usize => LevelFilter::Error,
        x if x == LevelFilter::Warn as usize => LevelFilter::Warn,
        x if x == LevelFilter::Info as usize => LevelFilter::Info,
        x if x == LevelFilter::Debug as usize => LevelFilter::Debug,
        x if x == LevelFilter::Trace as usize => LevelFilter::Trace,
        _ => LevelFilter::Error,
    }
}

fn open_log_file(log_path: &Path) -> Result<File, String> {
    if log_path.exists() {
        let metadata = log_path
            .metadata()
            .map_err(|e| format!("failed to inspect log file: {e}"))?;
        if metadata.len() > MAX_LOG_FILE_BYTES {
            remove_file(log_path).map_err(|e| format!("failed to rotate log file: {e}"))?;
        }
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("failed to open log file: {e}"))
}
