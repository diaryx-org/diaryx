use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const LOG_DIR_NAME: &str = "logs";
pub const LOG_FILE_NAME: &str = "diaryx.log";

const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;

struct FileBackedLogger {
    level: LevelFilter,
    file: Mutex<File>,
}

impl Log for FileBackedLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format!(
            "{} {:<5} [{}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );

        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(line.as_bytes());

        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

static LOGGER: OnceLock<FileBackedLogger> = OnceLock::new();

fn parse_level_filter() -> LevelFilter {
    match std::env::var("RUST_LOG")
        .ok()
        .unwrap_or_else(|| "info".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => LevelFilter::Off,
        "error" => LevelFilter::Error,
        "warn" | "warning" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

fn rotate_if_needed(log_file: &Path) -> Result<(), String> {
    let metadata = match fs::metadata(log_file) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("Failed to inspect log file metadata: {err}")),
    };

    if metadata.len() < MAX_LOG_FILE_BYTES {
        return Ok(());
    }

    let rotated_path = log_file.with_extension("previous.log");
    if rotated_path.exists() {
        fs::remove_file(&rotated_path)
            .map_err(|err| format!("Failed to remove previous rotated log: {err}"))?;
    }

    fs::rename(log_file, rotated_path)
        .map_err(|err| format!("Failed to rotate oversized log file: {err}"))?;
    Ok(())
}

pub fn log_paths(data_dir: &Path) -> (PathBuf, PathBuf) {
    let log_dir = data_dir.join(LOG_DIR_NAME);
    let log_file = log_dir.join(LOG_FILE_NAME);
    (log_dir, log_file)
}

pub fn init(log_file: &Path) -> Result<(), String> {
    if LOGGER.get().is_some() {
        return Ok(());
    }

    if let Some(parent) = log_file.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create log directory '{}': {err}",
                parent.display()
            )
        })?;
    }

    rotate_if_needed(log_file)?;

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .map_err(|err| format!("Failed to open log file '{}': {err}", log_file.display()))?;

    let level = parse_level_filter();
    LOGGER
        .set(FileBackedLogger {
            level,
            file: Mutex::new(file),
        })
        .map_err(|_| "Logger was already initialized".to_string())?;

    log::set_logger(LOGGER.get().expect("logger initialized"))
        .map_err(|err| format!("Failed to install logger: {err}"))?;
    log::set_max_level(level);
    Ok(())
}
