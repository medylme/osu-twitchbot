use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Local;
use iced::futures::channel::mpsc;
use owo_colors::OwoColorize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::{APP_NAME, VERSION};

const LOG_FILE_LATEST: &str = "latest.log";

#[allow(dead_code)]
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

#[macro_export]
macro_rules! log_debug {
    ($module:literal, $($arg:tt)*) => {
        ::tracing::debug!(target: $module, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_info {
    ($module:literal, $($arg:tt)*) => {
        ::tracing::info!(target: $module, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($module:literal, $($arg:tt)*) => {
        ::tracing::warn!(target: $module, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($module:literal, $($arg:tt)*) => {
        ::tracing::error!(target: $module, $($arg)*);
    };
}

pub fn debug_enabled() -> bool {
    cfg!(debug_assertions)
        || std::env::var("OSU_TWITCHBOT_DEBUG").is_ok()
        || std::env::args().any(|a| a == "--debug")
}

pub fn log_dir() -> Option<PathBuf> {
    let dir = directories::BaseDirs::new()?
        .data_local_dir()
        .join(APP_NAME)
        .join("logs");

    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("could not create log dir {}: {e}", dir.display());
        return None;
    }

    Some(dir)
}

pub fn open_log_dir() {
    match log_dir() {
        Some(path) => {
            if let Err(e) = open::that(&path) {
                tracing::error!(target: "log", "failed to open log dir {}: {e}", path.display());
            }
        }
        None => tracing::warn!(target: "log", "could not resolve log dir"),
    }
}

fn level_to_loglevel(level: &Level) -> LogLevel {
    match *level {
        Level::ERROR => LogLevel::Error,
        Level::WARN => LogLevel::Warn,
        Level::INFO => LogLevel::Info,
        _ => LogLevel::Debug,
    }
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{value:?}");
        }
    }
}

struct GuiLayer;

impl<S: Subscriber> Layer<S> for GuiLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        let entry = LogEntry::new(level_to_loglevel(meta.level()), meta.target(), visitor.0);
        let (tx, _) = get_log_channel();
        let _ = tx.clone().try_send(entry);
    }
}

struct Pretty {
    ansi: bool,
}

impl<S, N> FormatEvent<S, N> for Pretty
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let time = Local::now().format("%H:%M:%S%.3f");
        let level = *meta.level();
        let target = meta.target();

        if self.ansi {
            let level_str = match level {
                Level::TRACE => format!("{:5}", "TRACE").purple().to_string(),
                Level::DEBUG => format!("{:5}", "DEBUG").blue().to_string(),
                Level::INFO => format!("{:5}", "INFO").green().to_string(),
                Level::WARN => format!("{:5}", "WARN").yellow().to_string(),
                Level::ERROR => format!("{:5}", "ERROR").red().to_string(),
            };

            write!(
                writer,
                "{} {}  {}  {} ",
                time.dimmed(),
                format!("v{VERSION}").dimmed(),
                level_str,
                format!("[{target}]").cyan(),
            )?;
        } else {
            write!(
                writer,
                "{time} v{VERSION}  {:5}  [{target}] ",
                level.as_str()
            )?;
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn log_file_name() -> String {
    Local::now()
        .format("osu-twitchbot-%Y%m%d-%H%M%S.log")
        .to_string()
}

fn sentry_layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    sentry_tracing::layer().event_filter(|meta| match *meta.level() {
        Level::ERROR => sentry_tracing::EventFilter::Event,
        Level::WARN => sentry_tracing::EventFilter::Breadcrumb,
        _ => sentry_tracing::EventFilter::Ignore,
    })
}

const APP_TARGETS: [&str; 9] = [
    "main",
    "gui",
    "twitch",
    "osu",
    "memory-lazer",
    "memory-stable",
    "process",
    "prefs",
    "log",
];

fn app_filter(level: Level) -> Targets {
    let mut targets = Targets::new().with_default(LevelFilter::WARN);
    for target in APP_TARGETS {
        targets = targets.with_target(target, level);
    }
    targets
}

pub struct LogGuards {
    _latest: WorkerGuard,
    _timestamped: WorkerGuard,
}

pub fn init(telemetry_enabled: bool) -> Option<LogGuards> {
    let level = if debug_enabled() {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let console = tracing_subscriber::fmt::layer()
        .event_format(Pretty { ansi: true })
        .with_writer(std::io::stdout)
        .with_filter(app_filter(level));

    let gui = GuiLayer.with_filter(app_filter(level));

    if !cfg!(debug_assertions) {
        if let Some(dir) = log_dir() {
            let _ = std::fs::remove_file(dir.join(LOG_FILE_LATEST));

            let latest = tracing_appender::rolling::never(&dir, LOG_FILE_LATEST);
            let archive = tracing_appender::rolling::never(&dir, log_file_name());
            let (latest_nb, latest_guard) = tracing_appender::non_blocking(latest);
            let (archive_nb, archive_guard) = tracing_appender::non_blocking(archive);

            let latest_layer = tracing_subscriber::fmt::layer()
                .event_format(Pretty { ansi: false })
                .with_ansi(false)
                .with_writer(latest_nb)
                .with_filter(app_filter(level));
            let archive_layer = tracing_subscriber::fmt::layer()
                .event_format(Pretty { ansi: false })
                .with_ansi(false)
                .with_writer(archive_nb)
                .with_filter(app_filter(level));

            tracing_subscriber::registry()
                .with(console)
                .with(gui)
                .with(latest_layer)
                .with(archive_layer)
                .with(telemetry_enabled.then(sentry_layer))
                .init();

            return Some(LogGuards {
                _latest: latest_guard,
                _timestamped: archive_guard,
            });
        }

        tracing_subscriber::registry()
            .with(console)
            .with(gui)
            .with(telemetry_enabled.then(sentry_layer))
            .init();
        return None;
    }

    tracing_subscriber::registry()
        .with(console)
        .with(gui)
        .init();
    None
}
