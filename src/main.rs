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
    BeatmapData, DetectedProcess, MemoryEvent, OsuClient, OsuCommand, OsuStatus,
    detect_osu_processes,
};
use osu::lazer::run_lazer_reader;
use osu::stable::run_stable_reader;
use tray::{TrayEvent, TrayStatus};
use twitch::{TwitchClient, TwitchCommand, TwitchEvent, is_invalid_access_token_error};
#[cfg(not(debug_assertions))]
use updater::core::is_auto_update_enabled;
use updater::core::set_auto_update_enabled;

pub const APP_NAME: &str = "osu-twitchbot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROCESS_SCAN_INTERVAL_MS: u64 = 2000;

pub const MIN_WINDOW_SIZE: iced::Size = iced::Size::new(620.0, 400.0);
pub const MAX_WINDOW_SIZE: iced::Size = iced::Size::new(1000.0, 900.0);

fn main() -> iced::Result {
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

static OSU_CHANNEL: OnceLock<Channel<OsuCommand>> = OnceLock::new();
static TWITCH_CHANNEL: OnceLock<Channel<TwitchCommand>> = OnceLock::new();

static OSU_EVENT: OnceLock<Channel<MemoryEvent>> = OnceLock::new();
static TWITCH_EVENT: OnceLock<Channel<TwitchEvent>> = OnceLock::new();

static OSU_EVENT_FORWARD: OnceLock<Channel<MemoryEvent>> = OnceLock::new();

static TRAY_EVENT: OnceLock<Channel<TrayEvent>> = OnceLock::new();
static TRAY_STATUS: OnceLock<Channel<TrayStatus>> = OnceLock::new();

fn get_osu_channel() -> &'static Channel<OsuCommand> {
    OSU_CHANNEL.get_or_init(|| new_channel(10))
}

fn get_twitch_channel() -> &'static Channel<TwitchCommand> {
    TWITCH_CHANNEL.get_or_init(|| new_channel(10))
}

fn get_osu_event() -> &'static Channel<MemoryEvent> {
    OSU_EVENT.get_or_init(|| new_channel(10))
}

fn get_twitch_event() -> &'static Channel<TwitchEvent> {
    TWITCH_EVENT.get_or_init(|| new_channel(10))
}

fn get_osu_event_forward() -> &'static Channel<MemoryEvent> {
    OSU_EVENT_FORWARD.get_or_init(|| new_channel(10))
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

async fn osu_worker() {
    let cmd_rx = get_osu_channel().1.lock().unwrap().take();
    let Some(mut cmd_rx) = cmd_rx else {
        return;
    };

    let mut tx = get_osu_event().0.clone();
    let mut forward_tx = get_osu_event_forward().0.clone();

    let mut current_beatmap: Option<BeatmapData> = None;

    loop {
        let _ = tx
            .send(MemoryEvent::StatusChanged(OsuStatus::Scanning))
            .await;

        let process: DetectedProcess = loop {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    OsuCommand::RequestBeatmapData => {
                        let event = MemoryEvent::BeatmapDataResponse(current_beatmap.clone());
                        let _ = tx.send(event.clone()).await;
                        let _ = forward_tx.try_send(event);
                    }
                    OsuCommand::UpdateEventForwardSender(new_sender) => {
                        forward_tx = new_sender;
                        log_debug!("osu", "Updated event forward sender");
                    }
                }
            }

            let processes = detect_osu_processes();
            if let Some(found) = processes.into_iter().next() {
                break found;
            }
            time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
        };

        let result = match process.client {
            OsuClient::Lazer => {
                run_lazer_reader(
                    process.pid,
                    process.version,
                    &mut tx,
                    &mut cmd_rx,
                    &mut forward_tx,
                    &mut current_beatmap,
                )
                .await
            }
            OsuClient::Stable => {
                run_stable_reader(
                    process.pid,
                    process.songs_folder.clone(),
                    &mut tx,
                    &mut cmd_rx,
                    &mut forward_tx,
                    &mut current_beatmap,
                )
                .await
            }
        };

        if let Err(e) = result {
            log_error!("osu", "Memory reader error: {:#?}", e);
            if matches!(e, osu::core::MemoryError::AccessDenied) {
                log_warn!("osu", "{}", osu::core::privilege_hint());
            }
        }

        current_beatmap = None;
        let event = MemoryEvent::BeatmapChanged(None);
        let _ = tx.send(event.clone()).await;
        let _ = forward_tx.try_send(event);

        let _ = tx
            .send(MemoryEvent::StatusChanged(OsuStatus::Disconnected))
            .await;
        time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
    }
}

async fn twitch_worker() {
    let cmd_rx = get_twitch_channel().1.lock().unwrap().take();
    let Some(mut cmd_rx) = cmd_rx else {
        return;
    };

    let tx = get_twitch_event().0.clone();
    let osu_tx = get_osu_channel().0.clone();

    let mut websocket_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut current_client: Option<Arc<TwitchClient>> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            TwitchCommand::Connect {
                token,
                np_command,
                np_format,
                pp_command,
                pp_format,
            } => {
                if let Some(handle) = websocket_handle.take() {
                    handle.abort();
                }
                current_client = None;

                let result =
                    TwitchClient::new(&token, np_command, np_format, pp_command, pp_format).await;
                match result {
                    Ok(client) => {
                        let client = Arc::new(client);
                        let display_name = client.user.display_name.clone();
                        let user_id = client.user.id.clone();

                        let subscribe_result = client.subscribe_to_channel_messages(&user_id).await;

                        match subscribe_result {
                            Ok(()) => {
                                let (new_forward_tx, osu_event_rx) =
                                    mpsc::channel::<MemoryEvent>(10);

                                let osu_tx_for_update = osu_tx.clone();
                                if let Err(e) = osu_tx_for_update
                                    .send(OsuCommand::UpdateEventForwardSender(new_forward_tx))
                                    .await
                                {
                                    log_warn!(
                                        "twitch",
                                        "Failed to update osu event forward sender: {}",
                                        e
                                    );
                                }

                                let osu_tx_clone = osu_tx.clone();
                                let tx_clone = tx.clone();
                                let client_clone = Arc::clone(&client);

                                let ws_handle = tokio::spawn(async move {
                                    if let Err(e) = client_clone
                                        .init_websocket_handler(osu_tx_clone, osu_event_rx)
                                        .await
                                    {
                                        let err_s = e.to_string();
                                        if err_s.contains("Server requested reconnect") {
                                            log_warn!("twitch", "Websocket handler error: {}", e);
                                        } else {
                                            log_error!("twitch", "Websocket handler error: {}", e);
                                        }

                                        if err_s.contains("Server requested reconnect") {
                                            let _ = tx_clone
                                                    .send(TwitchEvent::Error(
                                                        "Reconnection needed - please reconnect manually"
                                                            .to_string(),
                                                    ))
                                                    .await;
                                        } else {
                                            let _ = tx_clone
                                                .send(TwitchEvent::Error(e.to_string()))
                                                .await;
                                        }
                                    } else {
                                        let _ = tx_clone.send(TwitchEvent::Disconnected).await;
                                    }
                                });

                                websocket_handle = Some(ws_handle);
                                current_client = Some(client);

                                let _ = tx.send(TwitchEvent::Connected(display_name)).await;
                            }
                            Err(e) => {
                                let error_msg = e.to_string();
                                if is_invalid_access_token_error(&error_msg) {
                                    log_warn!("twitch", "Subscription error: {:#?}", e);
                                } else {
                                    log_error!("twitch", "Subscription error: {:#?}", e);
                                }
                                let _ = tx.send(TwitchEvent::Error(error_msg)).await;
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        if is_invalid_access_token_error(&error_msg) {
                            log_warn!("twitch", "Client creation error: {:#?}", e);
                        } else {
                            log_error!("twitch", "Client creation error: {:#?}", e);
                        }
                        let _ = tx.send(TwitchEvent::Error(error_msg)).await;
                    }
                }
            }
            TwitchCommand::Disconnect => {
                if let Some(handle) = websocket_handle.take() {
                    handle.abort();
                }
                current_client = None;

                let _ = tx.send(TwitchEvent::Disconnected).await;
            }
            TwitchCommand::UpdatePreferences {
                np_command,
                np_format,
                pp_command,
                pp_format,
            } => {
                if let Some(ref client) = current_client {
                    client
                        .update_preferences(np_command, np_format, pp_command, pp_format)
                        .await;
                }
            }
        }
    }

    if let Some(handle) = websocket_handle {
        handle.abort();
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
