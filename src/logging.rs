use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Local;
use iced::futures::channel::mpsc;
use owo_colors::OwoColorize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub module: String,
    pub message: String,
}

impl LogEntry {
    pub fn new(level: LogLevel, module: &str, message: String) -> Self {
        let timestamp = Local::now().format("%H:%M:%S%.3f").to_string();
        Self {
            timestamp,
            level,
            module: module.to_string(),
            message,
        }
    }
}

type LogChannelType = (
    mpsc::Sender<LogEntry>,
    Arc<Mutex<Option<mpsc::Receiver<LogEntry>>>>,
);

static LOG_CHANNEL: OnceLock<LogChannelType> = OnceLock::new();

pub fn get_log_channel() -> &'static LogChannelType {
    LOG_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel(100);
        (tx, Arc::new(Mutex::new(Some(rx))))
    })
}

fn print_colored(entry: &LogEntry) {
    let version_str = format!("v{}", VERSION);

    let level_str = match entry.level {
        LogLevel::Debug => format!("{:5}", entry.level).blue().to_string(),
        LogLevel::Info => format!("{:5}", entry.level).green().to_string(),
        LogLevel::Warn => format!("{:5}", entry.level).yellow().to_string(),
        LogLevel::Error => format!("{:5}", entry.level).red().to_string(),
    };

    let module_str = format!("[{}]", entry.module).cyan().to_string();

    println!(
        "{} {}  {}  {} {}",
        entry.timestamp.dimmed(),
        version_str.dimmed(),
        level_str,
        module_str,
        entry.message
    );
}

pub fn log(level: LogLevel, module: &str, message: String) {
    let entry = LogEntry::new(level, module, message);

    // terminal
    print_colored(&entry);

    // gui
    let (tx, _) = get_log_channel();
    let _ = tx.clone().try_send(entry);
}

#[cfg(debug_assertions)]
pub fn log_debug(module: &str, message: String) {
    log(LogLevel::Debug, module, message);
}

#[cfg(not(debug_assertions))]
pub fn log_debug(_module: &str, _message: String) {
    // no-op for release builds
}

pub fn log_info(module: &str, message: String) {
    log(LogLevel::Info, module, message);
}

pub fn log_warn(module: &str, message: String) {
    log(LogLevel::Warn, module, message);
}

pub fn log_error(module: &str, message: String) {
    log(LogLevel::Error, module, message);
}

#[macro_export]
macro_rules! log_debug {
    ($module:literal, $($arg:tt)*) => {
        $crate::logging::log_debug($module, format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_info {
    ($module:literal, $($arg:tt)*) => {
        $crate::logging::log_info($module, format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_warn {
    ($module:literal, $($arg:tt)*) => {
        $crate::logging::log_warn($module, format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_error {
    ($module:literal, $($arg:tt)*) => {
        $crate::logging::log_error($module, format!($($arg)*));
    };
}
