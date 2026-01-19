use std::sync::{Arc, Mutex};

use iced::Alignment::Center;
use iced::futures::channel::mpsc;
use iced::widget::{
    button, center_x, center_y, checkbox, column, container, rich_text, row, scrollable, span,
    text, text_input,
};
use iced::{Element, Fill, Font};

use super::components::{
    BOLD_FONT, code_block_container, primary_button, primary_text_input, tab_button,
    tab_button_active,
};
use super::theme::{ColorPalette, get_current_theme, palette};
use crate::credentials::CredentialStore;
use crate::logging::{LogEntry, LogLevel};
use crate::osu::core::{BeatmapData, MemoryEvent, OsuCommand, OsuStatus};
use crate::preferences::PreferencesStore;
use crate::twitch::{
    DEFAULT_NP_COMMAND, DEFAULT_NP_FORMAT, TwitchCommand, TwitchEvent, TwitchStatus,
    parse_beatmap_placeholders,
};
use crate::{get_osu_channel, get_twitch_channel, log_debug, log_error, log_info, log_warn};

pub type CommandReceiver<T> = Arc<Mutex<Option<mpsc::Receiver<T>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Main,
    Settings,
    Data,
    Console,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    TokenInputChanged(String),
    AutoConnectToggled(bool),
    TokenHelpClicked,
    ConnectClicked,
    DisconnectClicked,
    ClearTokenClicked,
    NpCommandChanged(String),
    NpFormatChanged(String),
    ResetNpCommand,
    ResetNpFormat,
    OsuEvent(MemoryEvent),
    TwitchEvent(TwitchEvent),
    LogEvent(LogEntry),
    LinkClicked(String),
}

const MAX_LOG_ENTRIES: usize = 500;

#[allow(dead_code)]
pub struct State {
    active_tab: Tab,
    token_input_value: String,
    token_saved: bool,
    auto_connect_value: bool,
    np_command: String,
    np_format: String,
    current_beatmap: Option<BeatmapData>,
    osu_status: OsuStatus,
    osu_cmd_tx: mpsc::Sender<OsuCommand>,
    pub osu_cmd_rx: CommandReceiver<OsuCommand>,
    twitch_status: TwitchStatus,
    twitch_cmd_tx: mpsc::Sender<TwitchCommand>,
    pub twitch_cmd_rx: CommandReceiver<TwitchCommand>,
    log_entries: Vec<LogEntry>,
}

impl State {
    pub fn new() -> Self {
        let (osu_cmd_tx, osu_cmd_rx) = {
            let (tx, rx) = get_osu_channel();
            (tx.clone(), Arc::clone(rx))
        };
        let (twitch_cmd_tx, twitch_cmd_rx) = {
            let (tx, rx) = get_twitch_channel();
            (tx.clone(), Arc::clone(rx))
        };

        let (token_input_value, token_saved) = match CredentialStore::load_token() {
            Ok(token) => {
                log_debug!("gui", "Loaded saved token from credential store");
                (token, true)
            }
            Err(e) => {
                log_debug!("gui", "No saved token found: {}", e);
                (String::new(), false)
            }
        };

        let auto_connect_value = PreferencesStore::load()
            .map(|prefs| prefs.auto_connect())
            .unwrap_or(false);

        let twitch_status = if auto_connect_value && token_saved {
            log_info!("gui", "Auto-connecting to Twitch...");
            let _ = twitch_cmd_tx.clone().try_send(TwitchCommand::Connect {
                token: token_input_value.clone(),
                np_command: DEFAULT_NP_COMMAND.to_string(),
                np_format: DEFAULT_NP_FORMAT.to_string(),
            });
            TwitchStatus::Connecting
        } else {
            TwitchStatus::default()
        };

        Self {
            active_tab: Tab::Main,
            token_input_value,
            token_saved,
            auto_connect_value,
            np_command: DEFAULT_NP_COMMAND.to_string(),
            np_format: DEFAULT_NP_FORMAT.to_string(),
            current_beatmap: None,
            osu_status: OsuStatus::default(),
            osu_cmd_tx,
            osu_cmd_rx,
            twitch_status,
            twitch_cmd_tx,
            twitch_cmd_rx,
            log_entries: Vec::new(),
        }
    }

    pub fn title(&self) -> String {
        String::from("osu! twitchbot")
    }

    pub fn view(&self) -> Element<'_, Message> {
        let theme = get_current_theme();
        let p = palette(&theme);

        let tabs = row![
            button(text("Main").size(12))
                .style(if self.active_tab == Tab::Main {
                    tab_button_active
                } else {
                    tab_button
                })
                .on_press(Message::TabSelected(Tab::Main)),
            button(text("Settings").size(12))
                .style(if self.active_tab == Tab::Settings {
                    tab_button_active
                } else {
                    tab_button
                })
                .on_press(Message::TabSelected(Tab::Settings)),
            button(text("Data").size(12))
                .style(if self.active_tab == Tab::Data {
                    tab_button_active
                } else {
                    tab_button
                })
                .on_press(Message::TabSelected(Tab::Data)),
            button(text("Console").size(12))
                .style(if self.active_tab == Tab::Console {
                    tab_button_active
                } else {
                    tab_button
                })
                .on_press(Message::TabSelected(Tab::Console)),
        ]
        .spacing(2)
        .padding([5, 10]);

        let tab_bar = container(tabs)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(p.bg_secondary.into()),
                ..Default::default()
            });

        let content = match self.active_tab {
            Tab::Main => self.view_main_tab(&p),
            Tab::Settings => self.view_settings_tab(&p),
            Tab::Data => self.view_data_tab(&p),
            Tab::Console => self.view_console_tab(&p),
        };

        let footer = self.view_footer(&p);

        column![tab_bar, content, footer].into()
    }

    fn view_main_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        let token_label = row![
            text("Token").size(14),
            rich_text![
                span::<String, Font>("("),
                span::<String, Font>("?")
                    .color(p.accent)
                    .underline(true)
                    .link("https://osu-twitchbot.dyl.blue/"),
                span::<String, Font>(")"),
            ]
            .size(14)
            .on_link_click(|_| Message::TokenHelpClicked)
        ]
        .spacing(5);

        let token_placeholder = if self.token_saved && self.token_input_value.is_empty() {
            "Token saved securely"
        } else {
            "Enter your token here..."
        };

        let token_text_input = text_input(token_placeholder, &self.token_input_value)
            .secure(true)
            .size(12)
            .style(primary_text_input)
            .on_input(Message::TokenInputChanged);

        let action_button = match &self.twitch_status {
            TwitchStatus::Connected(_) => button(text("Disconnect").size(14))
                .style(primary_button)
                .on_press(Message::DisconnectClicked),
            TwitchStatus::Connecting => {
                button(text("Connecting...").size(14)).style(primary_button)
            }
            TwitchStatus::Disconnected | TwitchStatus::Error(_) => {
                let btn = button(text("Connect").size(14)).style(primary_button);
                if !self.token_input_value.is_empty() || self.token_saved {
                    btn.on_press(Message::ConnectClicked)
                } else {
                    btn
                }
            }
        };

        let mut main_row = row![token_label, token_text_input, action_button]
            .spacing(10)
            .align_y(Center);

        if self.token_saved {
            let clear_btn = button(text("Clear").size(14))
                .style(primary_button)
                .on_press(Message::ClearTokenClicked);
            main_row = main_row.push(clear_btn);
        }

        let auto_connect_checkbox = checkbox(self.auto_connect_value)
            .label("Auto-connect on startup")
            .on_toggle(Message::AutoConnectToggled)
            .size(14)
            .text_size(12);

        let main_content = column![main_row, auto_connect_checkbox]
            .spacing(10)
            .padding(10);

        let github_url = "https://github.com/medylme/osu-twitchbot";

        let version_string = if cfg!(debug_assertions) {
            "osu! twitchbot (DEV)".to_string()
        } else {
            format!("osu! twitchbot (v{})", env!("CARGO_PKG_VERSION"))
        };

        let version_text = text(version_string).size(12).color(p.text_secondary);
        let creator_text = rich_text![
            span::<String, Font>("Created by ").color(p.text_secondary),
            span::<String, Font>("me").color(p.text_muted),
            span::<String, Font>("dyl").color(p.accent).font(BOLD_FONT),
            span::<String, Font>("me").color(p.text_muted),
            span::<String, Font>(" • ").color(p.text_secondary),
            span::<String, Font>("GitHub")
                .color(p.accent)
                .underline(true)
                .link(github_url)
        ]
        .size(11)
        .on_link_click(Message::LinkClicked);

        let info_section = column![version_text, creator_text]
            .spacing(4)
            .align_x(Center);

        let full_content = column![main_content, info_section]
            .spacing(20)
            .align_x(Center);

        center_y(center_x(full_content)).height(Fill).into()
    }

    fn view_settings_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        let now_playing_header = text("Now Playing").size(14);

        let command_label = text("Command:").size(12);
        let command_input = text_input(DEFAULT_NP_COMMAND, &self.np_command)
            .size(12)
            .width(50)
            .style(primary_text_input)
            .on_input(Message::NpCommandChanged);
        let reset_command_btn = button(text("Reset").size(12))
            .style(primary_button)
            .on_press(Message::ResetNpCommand);
        let command_row = row![command_label, command_input, reset_command_btn]
            .spacing(10)
            .align_y(Center);

        let format_label = text("Message Format:").size(12);
        let format_input = text_input(DEFAULT_NP_FORMAT, &self.np_format)
            .size(12)
            .width(Fill)
            .style(primary_text_input)
            .on_input(Message::NpFormatChanged);
        let reset_format_btn = button(text("Reset").size(12))
            .style(primary_button)
            .on_press(Message::ResetNpFormat);
        let format_row = row![format_label, format_input, reset_format_btn]
            .spacing(10)
            .align_y(Center);

        let format_help = text("Available placeholders: {artist}, {title}, {diff}, {creator}, {mods}, {link}, {status}")
            .size(11)
            .color(p.text_secondary);

        let format_preview = self.build_format_preview(p);

        let settings_content = column![
            now_playing_header,
            command_row,
            format_row,
            format_help,
            format_preview
        ]
        .spacing(10)
        .padding(10);

        container(settings_content).height(Fill).into()
    }

    fn view_data_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        let content = match &self.current_beatmap {
            Some(beatmap) => {
                let mods_text = match &beatmap.mods {
                    Some(mods) if !mods.mods_string.is_empty() => mods.mods_string.clone(),
                    _ => "None".to_string(),
                };

                let beatmap_link = if beatmap.id <= 0 {
                    None
                } else {
                    Some(format!("https://osu.ppy.sh/b/{}", beatmap.id))
                };

                let data_rows: Vec<(&str, String)> = vec![
                    (
                        "ID",
                        if beatmap.id <= 0 {
                            "Local".to_string()
                        } else {
                            beatmap.id.to_string()
                        },
                    ),
                    ("Artist", beatmap.artist.clone()),
                    ("Title", beatmap.title.clone()),
                    ("Difficulty", beatmap.difficulty_name.clone()),
                    ("Creator", beatmap.creator.clone()),
                    ("Status", beatmap.status.to_string()),
                    ("Active Mods", mods_text),
                ];

                let table = column(data_rows.into_iter().map(|(label, value)| {
                    row![
                        text(label).size(11).color(p.text_secondary).width(70),
                        text(value).size(11).color(p.text_primary),
                    ]
                    .spacing(10)
                    .into()
                }))
                .spacing(4);

                let link_row = match &beatmap_link {
                    Some(link) => row![
                        text("Link").size(11).color(p.text_secondary).width(70),
                        rich_text![
                            span::<String, Font>(link.clone())
                                .color(p.accent_alt)
                                .underline(true)
                                .link(link.clone())
                        ]
                        .size(11)
                        .on_link_click(Message::LinkClicked)
                    ]
                    .spacing(10),
                    None => row![
                        text("Link").size(11).color(p.text_secondary).width(70),
                        text("Local").size(11).color(p.text_primary)
                    ]
                    .spacing(10),
                };

                column![table, link_row].spacing(4).padding(10)
            }
            None => {
                let no_data = text("No beatmap data available")
                    .size(12)
                    .color(p.text_secondary);

                let hint = text("Launch osu! and select a beatmap to see data here!")
                    .size(11)
                    .color(p.text_muted);

                column![no_data, hint].spacing(5).padding(10)
            }
        };

        scrollable(content).height(Fill).width(Fill).into()
    }

    fn view_console_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        // filter out debug logs
        let filtered_entries: Vec<&LogEntry> = self
            .log_entries
            .iter()
            .filter(|e| e.level >= LogLevel::Info)
            .collect();

        let inner_content: Element<'_, Message> = if filtered_entries.is_empty() {
            let placeholder = text("Console output will appear here...")
                .size(12)
                .color(p.text_muted);
            center_y(center_x(placeholder)).height(Fill).into()
        } else {
            let log_column = column(filtered_entries.iter().map(|entry| {
                let level_color = match entry.level {
                    LogLevel::Debug => p.status_info,
                    LogLevel::Info => p.status_success,
                    LogLevel::Warn => p.status_warning,
                    LogLevel::Error => p.status_error,
                };

                rich_text![
                    span::<String, Font>(&entry.timestamp).color(p.text_secondary),
                    span::<String, Font>("  "),
                    span::<String, Font>(format!("{:5}", entry.level)).color(level_color),
                    span::<String, Font>("  "),
                    span::<String, Font>(format!("[{}]", entry.module)).color(p.status_module),
                    span::<String, Font>(" "),
                    span::<String, Font>(&entry.message).color(p.text_primary),
                ]
                .size(11)
                .font(Font::MONOSPACE)
                .into()
            }))
            .spacing(2)
            .padding(10);

            scrollable(log_column).height(Fill).width(Fill).into()
        };

        container(inner_content)
            .height(Fill)
            .width(Fill)
            .padding(10)
            .style(code_block_container)
            .into()
    }

    fn build_format_preview(&self, p: &ColorPalette) -> Element<'_, Message> {
        // use current beatmap data if available, otherwise use sample data
        let preview_text = if let Some(beatmap) = &self.current_beatmap {
            parse_beatmap_placeholders(beatmap, &self.np_format)
        } else {
            // sample data for preview when no beatmap is loaded
            self.np_format
                .replace("{artist}", "Artist")
                .replace("{title}", "Title")
                .replace("{diff}", "Difficulty")
                .replace("{creator}", "Creator")
                .replace("{mods}", "+HD")
                .replace("{link}", "https://osu.ppy.sh/b/123456")
                .replace("{status}", "Ranked")
                .replace("{id}", "123456")
        };

        let preview_label = span::<String, Font>("Preview: ").color(p.text_secondary);
        let preview_content = span::<String, Font>(preview_text).color(p.text_primary);

        let preview_rich_text = rich_text![preview_label, preview_content].size(11);

        container(preview_rich_text)
            .padding(8)
            .width(Fill)
            .style(code_block_container)
            .into()
    }

    fn view_footer(&self, p: &ColorPalette) -> Element<'_, Message> {
        let text_primary = p.text_primary;
        let text_muted = p.text_muted;
        let bg_primary = p.bg_primary;

        let osu_status = rich_text![
            span::<String, Font>("osu!").color(text_primary),
            span::<String, Font>(" | ").color(text_muted),
            span::<String, Font>(self.osu_status.to_string()).color(text_primary),
        ]
        .size(12);
        let twitch_status = rich_text![
            span::<String, Font>("Twitch").color(text_primary),
            span::<String, Font>(" | ").color(text_muted),
            span::<String, Font>(self.twitch_status.to_string()).color(text_primary),
        ]
        .size(12);

        container(column![osu_status, twitch_status])
            .padding([5, 10])
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(bg_primary.into()),
                ..Default::default()
            })
            .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
            }
            Message::TokenInputChanged(value) => {
                self.token_input_value = value;
            }
            Message::AutoConnectToggled(value) => {
                self.auto_connect_value = value;
                if let Err(e) = PreferencesStore::set_auto_connect(value) {
                    log_warn!("gui", "Failed to save auto-connect preference: {}", e);
                }
            }
            Message::TokenHelpClicked => {
                let _ = open::that("https://osu-twitchbot.dyl.blue/");
            }
            Message::ConnectClicked => {
                log_debug!(
                    "gui",
                    "Connect clicked, token_input_empty={}, token_saved={}",
                    self.token_input_value.is_empty(),
                    self.token_saved
                );

                let token = if !self.token_input_value.is_empty() {
                    self.token_input_value.clone()
                } else if self.token_saved {
                    match CredentialStore::load_token() {
                        Ok(t) => t,
                        Err(e) => {
                            self.twitch_status =
                                TwitchStatus::Error(format!("Failed to load token: {}", e));
                            return;
                        }
                    }
                } else {
                    return;
                };

                self.twitch_status = TwitchStatus::Connecting;

                if let Err(e) = CredentialStore::save_token(&token) {
                    log_warn!("gui", "Failed to save token to credential store: {}", e);
                } else {
                    log_debug!("gui", "Token saved to credential store");
                    self.token_saved = true;
                }

                if let Err(e) = self.twitch_cmd_tx.try_send(TwitchCommand::Connect {
                    token,
                    np_command: self.np_command.clone(),
                    np_format: self.np_format.clone(),
                }) {
                    log_error!("gui", "Failed to send connect command: {}", e);
                    self.twitch_status =
                        TwitchStatus::Error("Failed to send connect command".to_string());
                }
            }
            Message::DisconnectClicked => {
                log_debug!("gui", "Disconnect clicked");
                self.twitch_status = TwitchStatus::Disconnected;
                if let Err(e) = self.twitch_cmd_tx.try_send(TwitchCommand::Disconnect) {
                    log_error!("gui", "Failed to send disconnect command: {}", e);
                }
            }
            Message::ClearTokenClicked => {
                log_debug!("gui", "Clear token clicked");
                if let Err(e) = CredentialStore::delete_token() {
                    log_warn!("gui", "Failed to delete token from credential store: {}", e);
                } else {
                    log_debug!("gui", "Token deleted from credential store");
                }
                self.token_input_value.clear();
                self.token_saved = false;
            }
            Message::NpCommandChanged(value) => {
                self.np_command = value;
                let _ = self
                    .twitch_cmd_tx
                    .try_send(TwitchCommand::UpdatePreferences {
                        np_command: Some(self.np_command.clone()),
                        np_format: None,
                    });
            }
            Message::NpFormatChanged(value) => {
                self.np_format = value;
                let _ = self
                    .twitch_cmd_tx
                    .try_send(TwitchCommand::UpdatePreferences {
                        np_command: None,
                        np_format: Some(self.np_format.clone()),
                    });
            }
            Message::ResetNpCommand => {
                log_debug!("gui", "Reset NP command");
                self.np_command = DEFAULT_NP_COMMAND.to_string();
                let _ = self
                    .twitch_cmd_tx
                    .try_send(TwitchCommand::UpdatePreferences {
                        np_command: Some(self.np_command.clone()),
                        np_format: None,
                    });
            }
            Message::ResetNpFormat => {
                log_debug!("gui", "Reset NP format");
                self.np_format = DEFAULT_NP_FORMAT.to_string();
                let _ = self
                    .twitch_cmd_tx
                    .try_send(TwitchCommand::UpdatePreferences {
                        np_command: None,
                        np_format: Some(self.np_format.clone()),
                    });
            }
            Message::OsuEvent(event) => match event {
                MemoryEvent::StatusChanged(ref status) => {
                    match status {
                        OsuStatus::Connected(client) => {
                            log_info!("osu", "Connected to {}", client);
                        }
                        OsuStatus::Disconnected => {
                            if matches!(self.osu_status, OsuStatus::Connected(_)) {
                                log_info!("osu", "Disconnected from osu!");
                            }
                        }
                        _ => {}
                    }
                    self.osu_status = status.clone();
                }
                MemoryEvent::BeatmapChanged(beatmap) => {
                    self.current_beatmap = beatmap;
                }
                MemoryEvent::BeatmapDataResponse(_) => {}
            },
            Message::TwitchEvent(event) => match event {
                TwitchEvent::Connected(ref username) => {
                    log_info!("twitch", "Connected to Twitch as {}", username);
                    self.twitch_status = TwitchStatus::Connected(username.clone());
                }
                TwitchEvent::Disconnected => {
                    log_info!("twitch", "Disconnected from Twitch");
                    self.twitch_status = TwitchStatus::Disconnected;
                }
                TwitchEvent::Error(ref e) => {
                    log_error!("twitch", "Connection error: {}", e);
                    self.twitch_status = TwitchStatus::Error(e.clone());
                }
            },
            Message::LogEvent(entry) => {
                self.log_entries.push(entry);
                // clamp amount of log entries
                if self.log_entries.len() > MAX_LOG_ENTRIES {
                    self.log_entries.remove(0);
                }
            }
            Message::LinkClicked(url) => {
                let _ = open::that(url);
            }
        }
    }
}
