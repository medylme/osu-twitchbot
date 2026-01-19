#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::sync::{Arc, Mutex, OnceLock};

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::window;
use iced::{Subscription, stream};
use tokio::time::{self, Duration};

mod credentials;
mod gui;
mod logging;
mod osu;
mod preferences;
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
use twitch::{TwitchClient, TwitchCommand, TwitchEvent};
#[cfg(not(debug_assertions))]
use updater::core::is_auto_update_enabled;
use updater::core::set_auto_update_enabled;

pub const APP_NAME: &str = "dyl-osu-twitchbot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROCESS_SCAN_INTERVAL_MS: u64 = 2000;

fn main() -> iced::Result {
    set_auto_update_enabled(args_auto_update());
    set_theme_override(args_theme_override());

    #[cfg(not(debug_assertions))]
    if is_auto_update_enabled() {
        updater::install::cleanup_old_binary();
        let _ = updater::splash::run_startup_update_check();
    }

    log_info!("main", "Starting osu-twitchbot");

    let icon = window::icon::from_file_data(
        include_bytes!("../assets/icon.png"),
        Some(image::ImageFormat::Png),
    )
    .ok();

    iced::application(State::new, State::update, State::view)
        .subscription(|_| {
            Subscription::batch([
                Subscription::run(osu_worker).map(Message::OsuEvent),
                Subscription::run(twitch_worker).map(Message::TwitchEvent),
                Subscription::run(log_worker).map(Message::LogEvent),
            ])
        })
        .theme(theme)
        .title(State::title)
        .window(window::Settings {
            icon,
            resizable: false,
            size: iced::Size::new(500.0, 250.0),
            ..Default::default()
        })
        .centered()
        .run()
}

type OsuChannelType = (
    mpsc::Sender<OsuCommand>,
    Arc<Mutex<Option<mpsc::Receiver<OsuCommand>>>>,
);

type TwitchChannelType = (
    mpsc::Sender<TwitchCommand>,
    Arc<Mutex<Option<mpsc::Receiver<TwitchCommand>>>>,
);

type OsuEventForwardType = (
    mpsc::Sender<MemoryEvent>,
    Arc<Mutex<Option<mpsc::Receiver<MemoryEvent>>>>,
);

static OSU_CHANNEL: OnceLock<OsuChannelType> = OnceLock::new();
static TWITCH_CHANNEL: OnceLock<TwitchChannelType> = OnceLock::new();
static OSU_EVENT_FORWARD: OnceLock<OsuEventForwardType> = OnceLock::new();

fn get_osu_channel() -> &'static OsuChannelType {
    OSU_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel(10);
        (tx, Arc::new(Mutex::new(Some(rx))))
    })
}

fn get_twitch_channel() -> &'static TwitchChannelType {
    TWITCH_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel(10);
        (tx, Arc::new(Mutex::new(Some(rx))))
    })
}

fn get_osu_event_forward() -> &'static OsuEventForwardType {
    OSU_EVENT_FORWARD.get_or_init(|| {
        let (tx, rx) = mpsc::channel(10);
        (tx, Arc::new(Mutex::new(Some(rx))))
    })
}

fn log_worker() -> impl iced::futures::Stream<Item = LogEntry> {
    stream::channel(100, |mut tx: mpsc::Sender<LogEntry>| async move {
        let (_, rx_holder) = get_log_channel();
        let log_rx = rx_holder.lock().unwrap().take();

        let Some(mut log_rx) = log_rx else {
            std::future::pending::<()>().await;
            return;
        };

        while let Some(entry) = log_rx.next().await {
            let _ = tx.send(entry).await;
        }
    })
}

fn osu_worker() -> impl iced::futures::Stream<Item = MemoryEvent> {
    stream::channel(10, |mut tx: mpsc::Sender<MemoryEvent>| async move {
        let (_, rx_holder) = get_osu_channel();
        let cmd_rx = rx_holder.lock().unwrap().take();

        let Some(mut cmd_rx) = cmd_rx else {
            std::future::pending::<()>().await;
            return;
        };

        let (forward_tx, _) = get_osu_event_forward();
        let mut forward_tx = forward_tx.clone();

        let mut current_beatmap: Option<BeatmapData> = None;

        loop {
            let _ = tx
                .send(MemoryEvent::StatusChanged(OsuStatus::Scanning))
                .await;

            let process: DetectedProcess = loop {
                if let Ok(Some(cmd)) = cmd_rx.try_next() {
                    match cmd {
                        OsuCommand::RequestBeatmapData => {
                            let event = MemoryEvent::BeatmapDataResponse(current_beatmap.clone());
                            let _ = tx.send(event.clone()).await;
                            let _ = forward_tx.send(event).await;
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
            }

            current_beatmap = None;
            let event = MemoryEvent::BeatmapChanged(None);
            let _ = tx.send(event.clone()).await;
            let _ = forward_tx.send(event).await;

            let _ = tx
                .send(MemoryEvent::StatusChanged(OsuStatus::Disconnected))
                .await;
            time::sleep(Duration::from_millis(PROCESS_SCAN_INTERVAL_MS)).await;
        }
    })
}

fn twitch_worker() -> impl iced::futures::Stream<Item = TwitchEvent> {
    stream::channel(10, |mut tx: mpsc::Sender<TwitchEvent>| async move {
        let (_, rx_holder) = get_twitch_channel();
        let cmd_rx = rx_holder.lock().unwrap().take();

        let Some(mut cmd_rx) = cmd_rx else {
            std::future::pending::<()>().await;
            return;
        };

        let (osu_tx, _) = get_osu_channel();

        let mut websocket_handle: Option<tokio::task::JoinHandle<()>> = None;
        let mut current_client: Option<Arc<TwitchClient>> = None;

        while let Some(cmd) = cmd_rx.next().await {
            match cmd {
                TwitchCommand::Connect {
                    token,
                    np_command,
                    np_format,
                } => {
                    // clean up any existing connections
                    if let Some(handle) = websocket_handle.take() {
                        handle.abort();
                    }
                    current_client = None;

                    let result = TwitchClient::new(&token, np_command, np_format).await;
                    match result {
                        Ok(client) => {
                            let client = Arc::new(client);
                            let display_name = client.user.display_name.clone();
                            let user_id = client.user.id.clone();

                            let subscribe_result =
                                client.subscribe_to_channel_messages(&user_id).await;

                            match subscribe_result {
                                Ok(()) => {
                                    let (_, forward_rx_holder) = get_osu_event_forward();
                                    let osu_event_rx = forward_rx_holder.lock().unwrap().take();

                                    if osu_event_rx.is_none() {
                                        log_warn!(
                                            "twitch",
                                            "osu event forward channel already taken!"
                                        );
                                    }

                                    let osu_event_rx = osu_event_rx.unwrap_or_else(|| {
                                        let (_, rx) = mpsc::channel::<MemoryEvent>(10);
                                        rx
                                    });

                                    let osu_tx_clone = osu_tx.clone();
                                    let mut tx_clone = tx.clone();
                                    let client_clone = Arc::clone(&client);

                                    let ws_handle = tokio::spawn(async move {
                                        if let Err(e) = client_clone
                                            .init_websocket_handler(osu_tx_clone, osu_event_rx)
                                            .await
                                        {
                                            log_error!("twitch", "Websocket handler error: {}", e);

                                            if e.to_string().contains("Server requested reconnect")
                                            {
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
                                    log_error!("twitch", "Subscription error: {:#?}", e);
                                    let error_msg = e.to_string();
                                    let _ = tx.send(TwitchEvent::Error(error_msg)).await;
                                }
                            }
                        }
                        Err(e) => {
                            log_error!("twitch", "Client creation error: {:#?}", e);
                            let error_msg = e.to_string();
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
                } => {
                    if let Some(ref client) = current_client {
                        client.update_preferences(np_command, np_format).await;
                    }
                }
            }
        }

        if let Some(handle) = websocket_handle {
            handle.abort();
        }
    })
}

fn theme(_state: &State) -> iced::Theme {
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
