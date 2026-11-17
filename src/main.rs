#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures::SinkExt;
use iced::window;
use iced::{Element, Subscription, Task, stream};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

mod credentials;
mod gui;
mod logging;
mod osu;
mod placeholders;
mod preferences;
mod tray;
mod twitch;
mod updater;

use gui::core::{Message, State};
use gui::theme::{ThemeOverride, get_current_theme, set_theme_override};
use logging::{LogEntry, get_log_channel};
use osu::core::{
    DetectedProcess, MemoryError, MemoryEvent, OsuClient, OsuStatus, detect_osu_processes,
    publish_beatmap, publish_profile,
};
use osu::lazer::run_lazer_reader;
use osu::stable::run_stable_reader;
use tray::{TrayEvent, TrayStatus};
use twitch::{
    ChatSettings, TwitchClient, TwitchCommand, TwitchEvent, is_invalid_access_token_error,
};
#[cfg(not(debug_assertions))]
use updater::core::is_auto_update_enabled;
use updater::core::set_auto_update_enabled;

pub const APP_NAME: &str = "osu-twitchbot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROCESS_SCAN_INTERVAL_MS: u64 = 2000;
const SINGLE_INSTANCE_PORT: u16 = 47727;

pub const MIN_WINDOW_SIZE: iced::Size = iced::Size::new(620.0, 400.0);
pub const MAX_WINDOW_SIZE: iced::Size = iced::Size::new(1000.0, 900.0);

fn ensure_single_instance() -> bool {
    use std::io::{ErrorKind, Write};
    use std::net::{TcpListener, TcpStream};

    match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => {
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    drop(stream);
                    let _ = get_tray_event().0.clone().try_send(TrayEvent::OpenWindow);
                }
            });
            true
        }
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
                let _ = stream.write_all(b"activate");
            }
            false
        }
        Err(_) => true,
    }
}

fn main() -> iced::Result {
    if !ensure_single_instance() {
        return Ok(());
    }

    let telemetry_enabled = preferences::PreferencesStore::load_or_default().telemetry_enabled();

    let _log_guards = logging::init(telemetry_enabled);

    let _guard = (cfg!(not(debug_assertions)) && telemetry_enabled)
        .then_some(option_env!("SENTRY_DSN"))
        .flatten()
        .map(|dsn| {
            sentry::init((
                dsn,
                sentry::ClientOptions {
                    release: sentry::release_name!(),
                    environment: Some(std::borrow::Cow::Borrowed("production")),
                    send_default_pii: false,
                    auto_session_tracking: true,
                    session_mode: sentry::SessionMode::Application,
                    attach_stacktrace: true,
                    ..Default::default()
                },
            ))
        });

    if _guard.is_some() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(client) = sentry::Hub::current().client() {
                client.flush(Some(std::time::Duration::from_secs(2)));
            }
            prev(info);
        }));
    }

    set_auto_update_enabled(args_auto_update());
    set_theme_override(args_theme_override());

    #[cfg(not(debug_assertions))]
    {
        updater::install::cleanup_old_binary();
        if is_auto_update_enabled() {
            let _ = updater::splash::run_startup_update_check();
        }
    }

    log_info!("main", "Starting osu-twitchbot");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    runtime.spawn(osu_worker());
    runtime.spawn(twitch_worker());

    #[cfg(feature = "tray")]
    {
        let status_rx = get_tray_status_channel().1.lock().unwrap().take();
        if let Some(status_rx) = status_rx {
            let active = tray::spawn(status_rx, get_tray_event().0.clone());
            TRAY_ACTIVE.store(active, Ordering::Relaxed);
            if !active {
                log_warn!(
                    "main",
                    "System tray unavailable; closing the window will quit"
                );
            }
        }
    }

    let result = iced::daemon(boot, State::update, view)
        .subscription(|_| {
            Subscription::batch([
                Subscription::run(osu_event_subscription).map(Message::OsuEvent),
                Subscription::run(twitch_event_subscription).map(Message::TwitchEvent),
                Subscription::run(log_subscription).map(Message::LogEvent),
                Subscription::run(tray_event_subscription).map(Message::Tray),
                window::close_requests().map(Message::WindowCloseRequested),
                window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
            ])
        })
        .theme(theme)
        .title(title)
        .run();

    drop(runtime);
    result
}

fn boot() -> (State, Task<Message>) {
    let (_id, open) = window::open(main_window_settings());
    (State::new(), open.map(Message::WindowOpened))
}

fn view(state: &State, _window: window::Id) -> Element<'_, Message> {
    state.view()
}

fn title(state: &State, _window: window::Id) -> String {
    state.title()
}

pub fn main_window_settings() -> window::Settings {
    let icon = window::icon::from_file_data(
        include_bytes!("../assets/icon.png"),
        Some(image::ImageFormat::Png),
    )
    .ok();

    window::Settings {
        icon,
        resizable: true,
        size: iced::Size::new(780.0, 470.0),
        min_size: Some(MIN_WINDOW_SIZE),
        max_size: Some(MAX_WINDOW_SIZE),
        position: window::Position::Centered,
        exit_on_close_request: false,
        ..Default::default()
    }
}

static TRAY_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn tray_active() -> bool {
    TRAY_ACTIVE.load(Ordering::Relaxed)
}

type Channel<T> = (mpsc::Sender<T>, Arc<Mutex<Option<mpsc::Receiver<T>>>>);

fn new_channel<T>(buffer: usize) -> Channel<T> {
    let (tx, rx) = mpsc::channel(buffer);
    (tx, Arc::new(Mutex::new(Some(rx))))
}

static TWITCH_CHANNEL: OnceLock<Channel<TwitchCommand>> = OnceLock::new();

static OSU_EVENT: OnceLock<Channel<MemoryEvent>> = OnceLock::new();
static TWITCH_EVENT: OnceLock<Channel<TwitchEvent>> = OnceLock::new();

static TRAY_EVENT: OnceLock<Channel<TrayEvent>> = OnceLock::new();
static TRAY_STATUS: OnceLock<Channel<TrayStatus>> = OnceLock::new();

fn get_twitch_channel() -> &'static Channel<TwitchCommand> {
    TWITCH_CHANNEL.get_or_init(|| new_channel(10))
}

fn get_osu_event() -> &'static Channel<MemoryEvent> {
    OSU_EVENT.get_or_init(|| new_channel(10))
}

fn get_twitch_event() -> &'static Channel<TwitchEvent> {
    TWITCH_EVENT.get_or_init(|| new_channel(10))
}

fn get_tray_event() -> &'static Channel<TrayEvent> {
    TRAY_EVENT.get_or_init(|| new_channel(10))
}

pub fn get_tray_status_channel() -> &'static Channel<TrayStatus> {
    TRAY_STATUS.get_or_init(|| new_channel(10))
}

fn bridge<T: Send + 'static>(
    channel: &'static Channel<T>,
    buffer: usize,
) -> impl iced::futures::Stream<Item = T> {
    stream::channel(
        buffer,
        move |mut output: iced::futures::channel::mpsc::Sender<T>| async move {
            let taken = channel.1.lock().unwrap().take();
            let Some(mut rx) = taken else {
                std::future::pending::<()>().await;
                return;
            };
            while let Some(item) = rx.recv().await {
                let _ = output.send(item).await;
            }
        },
    )
}

fn log_subscription() -> impl iced::futures::Stream<Item = LogEntry> {
    bridge(get_log_channel(), 100)
}

fn osu_event_subscription() -> impl iced::futures::Stream<Item = MemoryEvent> {
    bridge(get_osu_event(), 10)
}

fn twitch_event_subscription() -> impl iced::futures::Stream<Item = TwitchEvent> {
    bridge(get_twitch_event(), 10)
}

fn tray_event_subscription() -> impl iced::futures::Stream<Item = TrayEvent> {
    bridge(get_tray_event(), 10)
}

struct ReaderFailureStreak {
    pid: u32,
    message: String,
    count: u32,
    reported: bool,
}

const PERSISTENT_FAILURE_REPORT_THRESHOLD: u32 = 30;
const ESTABLISHED_SESSION_SECS: u64 = 60;
const STARTUP_GRACE_SECS: u64 = 60;
const EXIT_GRACE: Duration = Duration::from_secs(10);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, PartialEq)]
enum ReaderFailure {
    Exited,
    StartingUp,
    Failed,
}

fn classify_reader_failure(
    exited: bool,
    init_failed: bool,
    process_age_secs: u64,
) -> ReaderFailure {
    if exited {
        ReaderFailure::Exited
    } else if init_failed && process_age_secs < STARTUP_GRACE_SECS {
        ReaderFailure::StartingUp
    } else {
        ReaderFailure::Failed
    }
}

fn first_unsupported_report(version: &str) -> bool {
    static REPORTED: Mutex<Option<String>> = Mutex::new(None);

    let mut reported = REPORTED.lock().unwrap();
    if reported.as_deref() == Some(version) {
        return false;
    }
    *reported = Some(version.to_string());
    true
}

async fn exits_within(pid: u32, grace: Duration) -> bool {
    let deadline = time::Instant::now() + grace;

    loop {
        if !osu::core::is_process_alive(pid) {
            return true;
        }
        if time::Instant::now() >= deadline {
            return false;
        }
        time::sleep(EXIT_POLL_INTERVAL).await;
    }
}

async fn osu_worker() {
    let mut tx = get_osu_event().0.clone();

    let mut failure_streak: Option<ReaderFailureStreak> = None;
    let mut startup_notice_pid: Option<u32> = None;

    loop {
        let _ = tx
            .send(MemoryEvent::StatusChanged(OsuStatus::Scanning))
            .await;

        let process: DetectedProcess = loop {
            let processes = detect_osu_processes();
            if let Some(found) = processes.into_iter().next() {
                break found;
            }
            time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
        };

        let reader_started = time::Instant::now();

        let result = match process.client {
            OsuClient::Lazer => run_lazer_reader(process.pid, process.version, &mut tx).await,
            OsuClient::Stable => {
                run_stable_reader(process.pid, process.songs_folder.clone(), &mut tx).await
            }
        };

        publish_beatmap(None, &tx).await;
        publish_profile(None, &tx).await;

        let _ = tx
            .send(MemoryEvent::StatusChanged(OsuStatus::Disconnected))
            .await;

        if let Err(e) = result {
            let session_secs = reader_started.elapsed().as_secs();
            let init_failed = matches!(
                e,
                MemoryError::InitFailed(_) | MemoryError::UnsupportedVersion(_)
            );
            // a reader that was running usually fails because the game is closing
            let exited = if init_failed {
                !osu::core::is_process_alive(process.pid)
            } else {
                exits_within(process.pid, EXIT_GRACE).await
            };
            let process_age_secs = process.run_time_secs + session_secs;

            match classify_reader_failure(exited, init_failed, process_age_secs) {
                ReaderFailure::Exited => {
                    log_debug!(
                        "osu",
                        "osu! process exited (pid {}), last reader error: {}",
                        process.pid,
                        e
                    );
                    failure_streak = None;
                }
                ReaderFailure::StartingUp => {
                    if startup_notice_pid != Some(process.pid) {
                        log_info!("osu", "Waiting for osu! to finish starting...");
                        startup_notice_pid = Some(process.pid);
                    }
                    log_debug!("osu", "Reader not ready yet: {}", e);
                }
                ReaderFailure::Failed => {
                    if let MemoryError::UnsupportedVersion(version) = &e {
                        if first_unsupported_report(version) {
                            log_error!("osu", "{}, an update is needed", e);
                        }
                        let _ = tx
                            .send(MemoryEvent::StatusChanged(OsuStatus::Unsupported(format!(
                                "Lazer {version}"
                            ))))
                            .await;

                        // nothing changes until the game restarts
                        while osu::core::is_process_alive(process.pid) {
                            time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
                        }
                        continue;
                    }

                    if matches!(e, MemoryError::AccessDenied) {
                        log_warn!("osu", "{}", osu::core::privilege_hint());
                    }

                    let message = e.to_string();
                    let established = session_secs >= ESTABLISHED_SESSION_SECS;

                    match &mut failure_streak {
                        Some(streak) if streak.pid == process.pid && streak.message == message => {
                            streak.count += 1;
                            if !streak.reported
                                && streak.count >= PERSISTENT_FAILURE_REPORT_THRESHOLD
                            {
                                log_error!(
                                    "osu",
                                    "{:?} reader failing persistently ({} consecutive attempts): {}",
                                    process.client,
                                    streak.count,
                                    e
                                );
                                streak.reported = true;
                            } else {
                                log_debug!(
                                    "osu",
                                    "Memory reader error (attempt {}): {}",
                                    streak.count,
                                    e
                                );
                            }
                        }
                        _ => {
                            if established {
                                log_error!("osu", "{:?} reader error: {}", process.client, e);
                            } else {
                                log_warn!("osu", "{:?} reader error: {}", process.client, e);
                            }
                            failure_streak = Some(ReaderFailureStreak {
                                pid: process.pid,
                                message,
                                count: 1,
                                reported: established,
                            });
                        }
                    }
                }
            }
        }

        time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
    }
}

const RECONNECT_DELAYS_SECS: [u64; 5] = [2, 5, 15, 30, 60];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const STABLE_CONNECTION: Duration = Duration::from_secs(60);

fn reconnect_delay(attempt: usize) -> Duration {
    Duration::from_secs(RECONNECT_DELAYS_SECS[attempt.min(RECONNECT_DELAYS_SECS.len() - 1)])
}

struct TwitchConnection {
    client: Arc<TwitchClient>,
    handle: tokio::task::JoinHandle<()>,
    since: time::Instant,
}

type ConnectionEnd = (u64, Result<(), String>);

async fn open_twitch_connection(
    token: &str,
    settings: ChatSettings,
    id: u64,
    ended_tx: &mpsc::Sender<ConnectionEnd>,
) -> Result<TwitchConnection, String> {
    let connect = async {
        let client = TwitchClient::new(token, settings)
            .await
            .map_err(|e| e.to_string())?;
        client
            .subscribe_to_channel_messages(&client.user.id)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(Arc::new(client))
    };

    let client = time::timeout(CONNECT_TIMEOUT, connect)
        .await
        .map_err(|_| "Timed out connecting to Twitch".to_string())??;

    let handle = tokio::spawn({
        let client = Arc::clone(&client);
        let ended_tx = ended_tx.clone();
        async move {
            let result = client
                .init_websocket_handler()
                .await
                .map_err(|e| e.to_string());
            let _ = ended_tx.send((id, result)).await;
        }
    });

    Ok(TwitchConnection {
        client,
        handle,
        since: time::Instant::now(),
    })
}

async fn twitch_worker() {
    let cmd_rx = get_twitch_channel().1.lock().unwrap().take();
    let Some(mut cmd_rx) = cmd_rx else {
        return;
    };

    let tx = get_twitch_event().0.clone();
    let (ended_tx, mut ended_rx) = mpsc::channel::<ConnectionEnd>(4);

    let mut connection: Option<TwitchConnection> = None;
    let mut wanted: Option<(String, ChatSettings)> = None;
    let mut retry: Option<(time::Instant, usize)> = None;
    let mut connection_id: u64 = 0;
    let mut short_lived: usize = 0;

    loop {
        let retry_at = retry.map_or_else(time::Instant::now, |(at, _)| at);

        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    break;
                };

                match cmd {
                    TwitchCommand::Connect { token, settings } => {
                        if let Some(old) = connection.take() {
                            old.handle.abort();
                        }
                        retry = None;
                        short_lived = 0;
                        connection_id += 1;

                        match open_twitch_connection(&token, settings.clone(), connection_id, &ended_tx).await {
                            Ok(new) => {
                                let name = new.client.user.display_name.clone();
                                connection = Some(new);
                                wanted = Some((token, settings));
                                let _ = tx.send(TwitchEvent::Connected(name)).await;
                            }
                            Err(e) => {
                                log_debug!("twitch", "Failed to connect: {}", e);
                                wanted = None;
                                let _ = tx.send(TwitchEvent::Error(e)).await;
                            }
                        }
                    }
                    TwitchCommand::Disconnect => {
                        if let Some(old) = connection.take() {
                            old.handle.abort();
                        }
                        wanted = None;
                        retry = None;
                        connection_id += 1;

                        let _ = tx.send(TwitchEvent::Disconnected).await;
                    }
                    TwitchCommand::UpdateSettings(settings) => {
                        if let Some((_, wanted_settings)) = &mut wanted {
                            wanted_settings.clone_from(&settings);
                        }
                        if let Some(connection) = &connection {
                            connection.client.update_settings(settings).await;
                        }
                    }
                }
            }

            Some((id, result)) = ended_rx.recv() => {
                if id != connection_id || wanted.is_none() {
                    continue;
                }

                let lasted = connection.take().map(|old| old.since.elapsed());
                short_lived = match lasted {
                    Some(lasted) if lasted < STABLE_CONNECTION => short_lived + 1,
                    _ => 0,
                };

                let reason = result.err().unwrap_or_else(|| "closed by Twitch".to_string());
                let _ = tx.send(TwitchEvent::Reconnecting(reason)).await;
                retry = Some((
                    time::Instant::now() + reconnect_delay(short_lived),
                    short_lived,
                ));
            }

            () = time::sleep_until(retry_at), if retry.is_some() => {
                let Some(((token, settings), (_, attempt))) = wanted.clone().zip(retry.take()) else {
                    continue;
                };
                connection_id += 1;

                match open_twitch_connection(&token, settings, connection_id, &ended_tx).await {
                    Ok(new) => {
                        let name = new.client.user.display_name.clone();
                        connection = Some(new);
                        let _ = tx.send(TwitchEvent::Connected(name)).await;
                    }
                    Err(e) if is_invalid_access_token_error(&e) => {
                        wanted = None;
                        let _ = tx.send(TwitchEvent::Error(e)).await;
                    }
                    Err(e) => {
                        log_debug!("twitch", "Reconnect attempt {} failed: {}", attempt + 1, e);
                        retry = Some((
                            time::Instant::now() + reconnect_delay(attempt + 1),
                            attempt + 1,
                        ));
                    }
                }
            }
        }
    }

    if let Some(connection) = connection {
        connection.handle.abort();
    }
}

fn theme(_state: &State, _window: window::Id) -> iced::Theme {
    get_current_theme()
}

fn args_theme_override() -> ThemeOverride {
    let args: Vec<String> = std::env::args().collect();

    for i in 0..args.len() {
        if (args[i] == "--theme" || args[i] == "-t")
            && let Some(value) = args.get(i + 1)
        {
            if let Some(theme) = ThemeOverride::from_str(value) {
                return theme;
            } else {
                eprintln!(
                    "Warning: Invalid theme '{}'. Use 'light', 'dark', or 'system'.",
                    value
                );
            }
        }
    }

    ThemeOverride::System
}

fn args_auto_update() -> bool {
    let args: Vec<String> = std::env::args().collect();

    for arg in &args {
        if arg == "--no-update" {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnects_back_off_up_to_a_minute() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(2));
        assert_eq!(reconnect_delay(4), Duration::from_secs(60));
        assert_eq!(reconnect_delay(500), Duration::from_secs(60));
    }

    #[test]
    fn an_unsupported_version_is_reported_once() {
        assert!(first_unsupported_report("2026.1.0"));
        assert!(!first_unsupported_report("2026.1.0"));
        assert!(first_unsupported_report("2026.2.0"));
    }

    #[test]
    fn a_closed_game_is_never_a_failure() {
        assert_eq!(
            classify_reader_failure(true, false, 3600),
            ReaderFailure::Exited
        );
        assert_eq!(
            classify_reader_failure(true, true, 5),
            ReaderFailure::Exited
        );
    }

    #[test]
    fn a_fresh_game_gets_time_to_start() {
        assert_eq!(
            classify_reader_failure(false, true, 5),
            ReaderFailure::StartingUp
        );
        assert_eq!(
            classify_reader_failure(false, true, STARTUP_GRACE_SECS),
            ReaderFailure::Failed
        );
    }

    #[test]
    fn a_running_reader_that_breaks_is_a_failure() {
        assert_eq!(
            classify_reader_failure(false, false, 5),
            ReaderFailure::Failed
        );
    }
}
