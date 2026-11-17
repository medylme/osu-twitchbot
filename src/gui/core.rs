use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use iced::Alignment::Center;
use iced::widget::{
    button, center_x, center_y, checkbox, column, container, mouse_area, rich_text, row,
    scrollable, space, span, text, text_input, tooltip,
};
use iced::{Element, Fill, Font, Task, clipboard, mouse, window};

use super::components::{
    BOLD_FONT, MONO_BOLD_FONT, card_container, ghost_button, nav_button, nav_button_active,
    primary_button, primary_text_input, rail_container, separator, tooltip_container,
    window_container,
};
use super::theme::{ColorPalette, get_current_theme, palette};
use crate::credentials::CredentialStore;
use crate::logging::{LogEntry, LogLevel};
use crate::osu::core::{BeatmapData, MemoryEvent, OsuCommand, OsuStatus};
use crate::osu::pp::{MapAnalysis, MapStats, analyze_map};
use crate::placeholders::Placeholders;
use crate::preferences::PreferencesStore;
use crate::tray::{TrayEvent, TrayStatus};
use crate::twitch::{
    ChatSettings, DEFAULT_COOLDOWN_SECS, DEFAULT_NP_COMMAND, DEFAULT_NP_FORMAT, DEFAULT_PP_COMMAND,
    DEFAULT_PP_FORMAT, DEFAULT_PP_SCORE_FORMAT, INVALID_ACCESS_TOKEN_STATUS, TwitchCommand,
    TwitchEvent, TwitchStatus, is_invalid_access_token_error, is_transient_network_error,
};
use crate::{
    MAX_WINDOW_SIZE, MIN_WINDOW_SIZE, VERSION, get_osu_channel, get_tray_status_channel,
    get_twitch_channel, log_debug, log_error, log_info, log_warn, main_window_settings,
    tray_active,
};

pub type CommandReceiver<T> = Arc<Mutex<Option<mpsc::Receiver<T>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Main,
    Commands,
    Data,
    Console,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    TokenInputChanged(String),
    AutoConnectToggled(bool),
    MinimizeToTrayToggled(bool),
    TokenHelpClicked,
    ConnectClicked,
    DisconnectClicked,
    ClearTokenClicked,
    NpCommandChanged(String),
    NpFormatChanged(String),
    ResetNpCommand,
    ResetNpFormat,
    PpCommandChanged(String),
    PpFormatChanged(String),
    ResetPpCommand,
    ResetPpFormat,
    PpScoreFormatChanged(String),
    ResetPpScoreFormat,
    CooldownChanged(String),
    ResetCooldown,
    OsuEvent(MemoryEvent),
    TwitchEvent(TwitchEvent),
    LogEvent(LogEntry),
    LinkClicked(String),
    CopyPlaceholder(&'static str),
    PlaceholderCopyExpired(u64),
    OpenLogsClicked,
    OpenConfigClicked,
    WindowOpened(window::Id),
    WindowCloseRequested(window::Id),
    WindowResized(window::Id, iced::Size),
    Tray(TrayEvent),
}

const MAX_LOG_ENTRIES: usize = 500;
const PLACEHOLDER_COPIED_NOTICE: Duration = Duration::from_secs(3);

#[allow(dead_code)]
pub struct State {
    active_tab: Tab,
    token_input_value: String,
    token_saved: bool,
    auto_connect_value: bool,
    minimize_to_tray_value: bool,
    chat: ChatSettings,
    cooldown_input: String,
    current_beatmap: Option<BeatmapData>,
    cached_map: Option<MapAnalysis>,
    placeholder_copied: bool,
    placeholder_copies: u64,
    osu_status: OsuStatus,
    osu_cmd_tx: mpsc::Sender<OsuCommand>,
    pub osu_cmd_rx: CommandReceiver<OsuCommand>,
    twitch_status: TwitchStatus,
    twitch_cmd_tx: mpsc::Sender<TwitchCommand>,
    pub twitch_cmd_rx: CommandReceiver<TwitchCommand>,
    log_entries: VecDeque<LogEntry>,
    prefs: PreferencesStore,
    main_window: Option<window::Id>,
    last_tray_status: (String, String),
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

        let prefs = PreferencesStore::load_or_default();
        let auto_connect_value = prefs.auto_connect();
        let minimize_to_tray_value = prefs.minimize_to_tray();
        let chat = prefs.chat_settings();

        let twitch_status = if auto_connect_value && token_saved {
            log_info!("gui", "Auto-connecting to Twitch...");
            let _ = twitch_cmd_tx.clone().try_send(TwitchCommand::Connect {
                token: token_input_value.clone(),
                settings: chat.clone(),
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
            minimize_to_tray_value,
            cooldown_input: chat.cooldown_secs.to_string(),
            chat,
            current_beatmap: None,
            cached_map: None,
            placeholder_copied: false,
            placeholder_copies: 0,
            osu_status: OsuStatus::default(),
            osu_cmd_tx,
            osu_cmd_rx,
            twitch_status,
            twitch_cmd_tx,
            twitch_cmd_rx,
            log_entries: VecDeque::new(),
            prefs,
            main_window: None,
            last_tray_status: (String::new(), String::new()),
        }
    }

    pub fn title(&self) -> String {
        String::from("osu! twitchbot")
    }

    pub fn view(&self) -> Element<'_, Message> {
        let theme = get_current_theme();
        let p = palette(&theme);

        let content = match self.active_tab {
            Tab::Main => self.view_main_tab(&p),
            Tab::Commands => self.view_commands_tab(&p),
            Tab::Data => self.view_data_tab(&p),
            Tab::Console => self.view_console_tab(&p),
        };

        let main = column![
            self.view_statusbar(&p),
            container(space::horizontal())
                .width(Fill)
                .height(1)
                .style(separator),
            content,
        ];

        let layout = row![
            container(self.view_rail(&p))
                .width(168)
                .height(Fill)
                .style(rail_container),
            container(space::vertical())
                .width(1)
                .height(Fill)
                .style(separator),
            main,
        ];

        container(layout)
            .width(Fill)
            .height(Fill)
            .style(window_container)
            .into()
    }

    fn view_rail(&self, p: &ColorPalette) -> Element<'_, Message> {
        let nav_item = |label: &'static str, tab: Tab| {
            button(text(label).size(13))
                .width(Fill)
                .padding([8, 12])
                .style(if self.active_tab == tab {
                    nav_button_active
                } else {
                    nav_button
                })
                .on_press(Message::TabSelected(tab))
        };

        let nav = column![
            nav_item("Main", Tab::Main),
            nav_item("Commands", Tab::Commands),
            nav_item("Data", Tab::Data),
            nav_item("Console", Tab::Console),
        ]
        .spacing(3);

        let version_string = if cfg!(debug_assertions) {
            "Dev".to_string()
        } else {
            format!("v{}", VERSION)
        };
        let version_text = text(version_string).size(11).color(p.text_muted);
        let creator_text = rich_text![
            span::<String, Font>("by ").color(p.text_muted),
            span::<String, Font>("dyl")
                .color(p.text_secondary)
                .font(BOLD_FONT)
                .link("https://github.com/medylme/osu-twitchbot".to_string()),
        ]
        .size(11)
        .on_link_click(Message::LinkClicked);

        let footer = column![version_text, creator_text]
            .spacing(2)
            .padding([0, 4]);

        column![nav, space::vertical(), footer]
            .height(Fill)
            .padding(10)
            .into()
    }

    fn view_statusbar(&self, p: &ColorPalette) -> Element<'_, Message> {
        let osu_on = matches!(self.osu_status, OsuStatus::Connected(_));
        let twitch_on = matches!(self.twitch_status, TwitchStatus::Connected(_));

        let stat = |on: bool, label: &'static str, value: String| {
            let dot_color = if on { p.status_success } else { p.text_muted };
            row![
                text("●").size(10).color(dot_color),
                text(label).size(12).color(p.text_secondary),
                text(value).size(12).font(BOLD_FONT).color(p.text_primary),
            ]
            .spacing(7)
            .align_y(Center)
        };

        row![
            stat(osu_on, "osu!", self.osu_status.to_string()),
            stat(twitch_on, "Twitch", self.twitch_status.to_string()),
        ]
        .spacing(18)
        .padding([10, 20])
        .into()
    }

    fn section_title<'a>(&self, p: &ColorPalette, label: &'static str) -> Element<'a, Message> {
        text(label)
            .size(11)
            .font(BOLD_FONT)
            .color(p.text_secondary)
            .into()
    }

    fn view_main_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        let token_label = rich_text![
            span::<String, Font>("Twitch token").color(p.text_secondary),
            span::<String, Font>("   "),
            span::<String, Font>("get one")
                .color(p.accent)
                .underline(true)
                .link("https://osu-twitchbot.dyl.blue/"),
        ]
        .size(12)
        .on_link_click(|_| Message::TokenHelpClicked);

        let token_placeholder = if self.token_saved && self.token_input_value.is_empty() {
            "Token saved securely"
        } else {
            "Enter your token here..."
        };

        let token_text_input = text_input(token_placeholder, &self.token_input_value)
            .secure(true)
            .size(13)
            .padding([9, 12])
            .font(Font::MONOSPACE)
            .style(primary_text_input)
            .on_input(Message::TokenInputChanged);

        let action_button = match &self.twitch_status {
            TwitchStatus::Connected(_) => button(text("Disconnect").size(13))
                .style(primary_button)
                .padding([8, 16])
                .on_press(Message::DisconnectClicked),
            TwitchStatus::Connecting => button(text("Connecting...").size(13))
                .style(primary_button)
                .padding([8, 16]),
            TwitchStatus::Disconnected | TwitchStatus::Error(_) => {
                let btn = button(text("Connect").size(13))
                    .style(primary_button)
                    .padding([8, 16]);
                if !self.token_input_value.is_empty() || self.token_saved {
                    btn.on_press(Message::ConnectClicked)
                } else {
                    btn
                }
            }
        };

        let mut token_row = row![token_text_input, action_button]
            .spacing(10)
            .align_y(Center);

        if self.token_saved {
            let clear_btn = button(text("Clear").size(13))
                .style(ghost_button)
                .padding([8, 14])
                .on_press(Message::ClearTokenClicked);
            token_row = token_row.push(clear_btn);
        }

        let auto_connect_checkbox = checkbox(self.auto_connect_value)
            .label("Connect automatically on launch")
            .on_toggle(Message::AutoConnectToggled)
            .size(15)
            .text_size(12);

        // the tray gates this: with no tray, closing always quits
        let tray_on = tray_active();
        let minimize_checkbox = {
            let cb = checkbox(self.minimize_to_tray_value && tray_on)
                .label("Minimize to tray on close")
                .size(15)
                .text_size(12);
            if tray_on {
                cb.on_toggle(Message::MinimizeToTrayToggled)
            } else {
                cb
            }
        };

        let minimize_hint = text(if tray_on {
            "When off, closing the window quits the app."
        } else {
            "System tray unavailable; closing the window quits the app."
        })
        .size(11)
        .color(p.text_muted);

        column![
            self.section_title(p, "Connection"),
            column![token_label, token_row].spacing(6),
            auto_connect_checkbox,
            container(space::horizontal())
                .width(Fill)
                .height(1)
                .style(separator),
            self.section_title(p, "Window"),
            minimize_checkbox,
            minimize_hint,
        ]
        .spacing(14)
        .padding([22, 24])
        .into()
    }

    fn reset_button<'a>(&self, on_press: Message) -> Element<'a, Message> {
        button(text("Reset").size(11))
            .style(ghost_button)
            .padding([6, 12])
            .on_press(on_press)
            .into()
    }

    #[allow(clippy::too_many_arguments)]
    fn format_field<'a>(
        &'a self,
        p: &ColorPalette,
        label: &'static str,
        value: &'a str,
        placeholder: &'static str,
        on_input: fn(String) -> Message,
        on_reset: Message,
        help: &'static str,
        preview: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let input = text_input(placeholder, value)
            .size(13)
            .padding([8, 12])
            .width(Fill)
            .font(Font::MONOSPACE)
            .style(primary_text_input)
            .on_input(on_input);

        column![
            column![
                text(label).size(12).color(p.text_secondary),
                row![input, self.reset_button(on_reset)]
                    .spacing(10)
                    .align_y(Center),
            ]
            .spacing(5),
            text(help).size(11).color(p.text_muted).width(Fill),
            preview,
        ]
        .spacing(10)
        .into()
    }

    #[allow(clippy::too_many_arguments)]
    fn command_format_section<'a>(
        &'a self,
        p: &ColorPalette,
        title: &'static str,
        command_value: &'a str,
        command_placeholder: &'static str,
        on_command: fn(String) -> Message,
        on_reset_command: Message,
        format_value: &'a str,
        format_placeholder: &'static str,
        on_format: fn(String) -> Message,
        on_reset_format: Message,
        help: &'static str,
        preview: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let command_input = text_input(command_placeholder, command_value)
            .size(13)
            .padding([8, 12])
            .width(120)
            .font(Font::MONOSPACE)
            .style(primary_text_input)
            .on_input(on_command);

        column![
            self.section_title(p, title),
            column![
                text("Command").size(12).color(p.text_secondary),
                row![command_input, self.reset_button(on_reset_command)]
                    .spacing(10)
                    .align_y(Center),
            ]
            .spacing(5),
            self.format_field(
                p,
                "Format",
                format_value,
                format_placeholder,
                on_format,
                on_reset_format,
                help,
                preview,
            ),
        ]
        .spacing(10)
        .into()
    }

    fn view_commands_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
        let np_section = self.command_format_section(
            p,
            "Now Playing",
            &self.chat.np_command,
            DEFAULT_NP_COMMAND,
            Message::NpCommandChanged,
            Message::ResetNpCommand,
            &self.chat.np_format,
            DEFAULT_NP_FORMAT,
            Message::NpFormatChanged,
            Message::ResetNpFormat,
            concat!(
                "Placeholders: {artist} {title} {diff} {creator} {mods} {status} {link} {state} ",
                "{stars} {bpm} {length} {cs} {ar} {od} {hp} {max_combo} {objects}",
            ),
            self.build_np_format_preview(p),
        );

        let pp_section = self.command_format_section(
            p,
            "Performance Points",
            &self.chat.pp_command,
            DEFAULT_PP_COMMAND,
            Message::PpCommandChanged,
            Message::ResetPpCommand,
            &self.chat.pp_format,
            DEFAULT_PP_FORMAT,
            Message::PpFormatChanged,
            Message::ResetPpFormat,
            "Placeholders: {mods} {pp_95} {pp_97} {pp_98} {pp_99} {pp_100}",
            self.build_pp_format_preview(p),
        );

        let pp_score_format = self.format_field(
            p,
            "Format with accuracy, combo or misses",
            &self.chat.pp_score_format,
            DEFAULT_PP_SCORE_FORMAT,
            Message::PpScoreFormatChanged,
            Message::ResetPpScoreFormat,
            concat!(
                "Viewers can add an accuracy, combo, misses and mods, ",
                "e.g. !pp 98.5 800x 2m +HDDT. Mods alone keep the format above.\n",
                "Placeholders: {acc} {combo} {misses} {pp} {mods}",
            ),
            self.chat_preview(
                p,
                Placeholders::sample_pp_score().apply_pp_score(&self.chat.pp_score_format),
            ),
        );

        let cooldown_input = text_input("1", &self.cooldown_input)
            .size(13)
            .padding([8, 12])
            .width(120)
            .font(Font::MONOSPACE)
            .style(primary_text_input)
            .on_input(Message::CooldownChanged);

        let global_section = column![
            self.section_title(p, "Global"),
            column![
                text("Cooldown (seconds)").size(12).color(p.text_secondary),
                row![cooldown_input, self.reset_button(Message::ResetCooldown)]
                    .spacing(10)
                    .align_y(Center),
            ]
            .spacing(5),
            text("Minimum time between replies, shared by all commands. 0 turns it off.")
                .size(11)
                .color(p.text_muted)
                .width(Fill),
        ]
        .spacing(10);

        let commands_content = column![
            global_section,
            np_section,
            column![pp_section, pp_score_format].spacing(10)
        ]
        .spacing(24)
        .padding([22, 24]);

        scrollable(container(commands_content).width(Fill))
            .height(Fill)
            .into()
    }

    fn view_data_tab<'a>(&'a self, p: &ColorPalette) -> Element<'a, Message> {
        let Some(beatmap) = &self.current_beatmap else {
            let no_data = text("No beatmap selected").size(13).color(p.text_secondary);
            let hint = text("Launch osu! and select a beatmap to see data here!")
                .size(11)
                .color(p.text_muted);
            let placeholder = column![no_data, hint].spacing(5).align_x(Center);
            return center_y(center_x(placeholder)).height(Fill).into();
        };

        let mods_text = match &beatmap.mods {
            Some(mods) if !mods.mods_string.is_empty() => mods.mods_string.clone(),
            _ => "None".to_string(),
        };

        let copyable = |content: Element<'a, Message>, placeholder: &'static str| {
            let hint = container(text(placeholder).size(11).font(Font::MONOSPACE))
                .padding([4, 8])
                .style(tooltip_container);

            tooltip(
                mouse_area(content)
                    .on_press(Message::CopyPlaceholder(placeholder))
                    .interaction(mouse::Interaction::Pointer),
                hint,
                tooltip::Position::Top,
            )
            .gap(4)
        };

        let header = column![
            copyable(
                text(beatmap.title.clone()).size(18).font(BOLD_FONT).into(),
                "{title}",
            ),
            copyable(
                text(beatmap.artist.clone())
                    .size(13)
                    .color(p.text_secondary)
                    .into(),
                "{artist}",
            ),
            copyable(
                text(format!("[{}]", beatmap.difficulty_name))
                    .size(12)
                    .font(Font::MONOSPACE)
                    .color(p.accent)
                    .into(),
                "{diff}",
            ),
        ]
        .spacing(2);

        let field = |label: &'static str| text(label).size(12).color(p.text_muted).width(84);
        let value = |v: String| text(v).size(12).font(Font::MONOSPACE).color(p.text_primary);
        let copyable_value =
            |v: String, placeholder: &'static str| copyable(value(v).into(), placeholder);

        let link = if beatmap.id <= 0 {
            "Local".to_string()
        } else {
            format!("https://osu.ppy.sh/b/{}", beatmap.id)
        };

        let details = column![
            row![
                field("Creator"),
                copyable_value(beatmap.creator.clone(), "{creator}")
            ]
            .spacing(10),
            row![
                field("Status"),
                copyable_value(beatmap.status.to_string(), "{status}")
            ]
            .spacing(10),
            row![field("Mods"), copyable_value(mods_text, "{mods}")].spacing(10),
            row![
                field("State"),
                copyable_value(beatmap.state.to_string(), "{state}")
            ]
            .spacing(10),
            row![field("Beatmap"), copyable_value(link, "{link}")].spacing(10),
        ]
        .spacing(6);

        let cell = |label: &'static str,
                    placeholder: &'static str,
                    value: String,
                    unit: &'static str,
                    highlight: bool| {
            let number_color = if highlight { p.accent } else { p.text_primary };
            let content = container(
                column![
                    text(label).size(11).color(p.text_secondary),
                    rich_text![
                        span::<String, Font>(value)
                            .color(number_color)
                            .font(MONO_BOLD_FONT),
                        span::<String, Font>(unit).color(p.text_muted),
                    ]
                    .size(16),
                ]
                .spacing(2)
                .align_x(Center),
            )
            .width(Fill)
            .align_x(Center);

            copyable(content.into(), placeholder)
        };

        let card = |title: &'static str, body: Element<'a, Message>| {
            container(column![text(title).size(10).color(p.text_muted), body].spacing(10))
                .padding(14)
                .width(Fill)
                .style(card_container)
        };

        let unavailable = || -> Element<'a, Message> {
            text("Not available for this beatmap")
                .size(12)
                .color(p.text_muted)
                .into()
        };

        let (stats_card_body, pp_card_body) = match &self.cached_map {
            Some(MapAnalysis { stats, pp }) => {
                let setting = |label: &'static str, placeholder: &'static str, value: f64| {
                    cell(label, placeholder, MapStats::setting_text(value), "", false)
                };
                let pp_cell = |label: &'static str,
                               placeholder: &'static str,
                               value: f64,
                               highlight: bool| {
                    cell(label, placeholder, format!("{value:.0}"), "pp", highlight)
                };

                let stats_body = column![
                    row![
                        cell("Stars", "{stars}", stats.stars_text(), "", true),
                        cell("BPM", "{bpm}", stats.bpm_text(), "", false),
                        cell("Length", "{length}", stats.length_text(), "", false),
                        cell(
                            "Max combo",
                            "{max_combo}",
                            stats.max_combo.to_string(),
                            "x",
                            false
                        ),
                        cell("Objects", "{objects}", stats.objects.to_string(), "", false),
                    ]
                    .spacing(10),
                    row![
                        setting("CS", "{cs}", stats.cs),
                        setting("AR", "{ar}", stats.ar),
                        setting("OD", "{od}", stats.od),
                        setting("HP", "{hp}", stats.hp),
                    ]
                    .spacing(10),
                ]
                .spacing(12);

                let pp_body = row![
                    pp_cell("95%", "{pp_95}", pp.pp_95, false),
                    pp_cell("97%", "{pp_97}", pp.pp_97, false),
                    pp_cell("98%", "{pp_98}", pp.pp_98, false),
                    pp_cell("99%", "{pp_99}", pp.pp_99, false),
                    pp_cell("100%", "{pp_100}", pp.pp_100, true),
                ]
                .spacing(10);

                (stats_body.into(), pp_body.into())
            }
            None => (unavailable(), unavailable()),
        };

        let content = column![
            header,
            details,
            card("Map Stats", stats_card_body),
            card("Performance Points", pp_card_body),
        ]
        .spacing(16)
        .padding([22, 24]);

        let (footer_text, footer_color) = if self.placeholder_copied {
            ("Placeholder copied!", p.accent)
        } else {
            ("Click on any value to copy its placeholder", p.text_muted)
        };
        let footer = container(text(footer_text).size(11).color(footer_color))
            .width(Fill)
            .align_x(Center)
            .padding([8, 24]);

        column![scrollable(content).height(Fill).width(Fill), footer].into()
    }

    fn view_console_tab(&self, p: &ColorPalette) -> Element<'_, Message> {
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

        let log_box = container(inner_content)
            .height(Fill)
            .width(Fill)
            .padding(6)
            .style(card_container);

        let open_logs_btn = button(text("Open Logs Folder").size(11))
            .style(ghost_button)
            .padding([6, 12])
            .on_press(Message::OpenLogsClicked);

        let open_config_btn = button(text("Open Config Folder").size(11))
            .style(ghost_button)
            .padding([6, 12])
            .on_press(Message::OpenConfigClicked);

        let head = row![
            self.section_title(p, "Console"),
            space::horizontal(),
            open_logs_btn,
            open_config_btn,
        ]
        .spacing(8)
        .align_y(Center);

        column![head, log_box].spacing(10).padding([16, 24]).into()
    }

    fn chat_preview(&self, p: &ColorPalette, message_text: String) -> Element<'_, Message> {
        let who = match &self.twitch_status {
            TwitchStatus::Connected(user) => user.clone(),
            _ => "bot".to_string(),
        };

        let cap = text("Preview · what chat sees")
            .size(10)
            .color(p.text_muted);
        let line = rich_text![
            span::<String, Font>(who)
                .color(p.accent_alt)
                .font(BOLD_FONT),
            span::<String, Font>(": ").color(p.text_secondary),
            span::<String, Font>(message_text)
                .color(p.text_primary)
                .font(Font::MONOSPACE),
        ]
        .size(12);

        container(column![cap, line].spacing(6))
            .padding([10, 12])
            .width(Fill)
            .style(card_container)
            .into()
    }

    fn map_stats(&self) -> Option<&MapStats> {
        self.cached_map.as_ref().map(|analysis| &analysis.stats)
    }

    fn build_np_format_preview(&self, p: &ColorPalette) -> Element<'_, Message> {
        let placeholders = self
            .current_beatmap
            .as_ref()
            .map(|beatmap| Placeholders::from_beatmap(beatmap).with_stats(self.map_stats()))
            .unwrap_or_else(Placeholders::sample);

        self.chat_preview(p, placeholders.apply_np(&self.chat.np_format))
    }

    fn build_pp_format_preview(&self, p: &ColorPalette) -> Element<'_, Message> {
        let placeholders = match (&self.current_beatmap, &self.cached_map) {
            (Some(beatmap), Some(analysis)) => {
                Placeholders::from_beatmap(beatmap).with_pp(&analysis.pp)
            }
            _ => Placeholders::sample_pp(),
        };

        self.chat_preview(p, placeholders.apply_pp(&self.chat.pp_format))
    }

    fn chat_settings_changed(&mut self) {
        log_debug!("gui", "Changed chat settings to {:?}", self.chat);
        if let Err(e) = self.prefs.set_chat_settings(&self.chat) {
            log_warn!("gui", "Failed to save chat settings: {}", e);
        }
        let _ = self
            .twitch_cmd_tx
            .try_send(TwitchCommand::UpdateSettings(self.chat.clone()));
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        self.sync_tray();
        task
    }

    fn sync_tray(&mut self) {
        let status = (self.osu_status.to_string(), self.twitch_status.to_string());
        if status != self.last_tray_status {
            let _ = get_tray_status_channel().0.try_send(TrayStatus {
                osu: status.0.clone(),
                twitch: status.1.clone(),
            });
            self.last_tray_status = status;
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                self.main_window = Some(id);
            }
            Message::WindowResized(id, size) => {
                let clamped = iced::Size::new(
                    size.width
                        .clamp(MIN_WINDOW_SIZE.width, MAX_WINDOW_SIZE.width),
                    size.height
                        .clamp(MIN_WINDOW_SIZE.height, MAX_WINDOW_SIZE.height),
                );
                if clamped != size {
                    return window::resize(id, clamped);
                }
            }
            Message::WindowCloseRequested(id) => {
                if tray_active() && self.minimize_to_tray_value {
                    log_info!("gui", "Window closed to tray");
                    self.main_window = None;
                    return window::close(id);
                }
                return iced::exit();
            }
            Message::Tray(TrayEvent::OpenWindow) => {
                return match self.main_window {
                    Some(id) => window::gain_focus(id),
                    None => {
                        let (id, open) = window::open(main_window_settings());
                        self.main_window = Some(id);
                        open.map(Message::WindowOpened)
                    }
                };
            }
            Message::Tray(TrayEvent::Quit) => {
                return iced::exit();
            }
            Message::TabSelected(tab) => {
                self.active_tab = tab;
            }
            Message::TokenInputChanged(value) => {
                self.token_input_value = value;
            }
            Message::AutoConnectToggled(value) => {
                self.auto_connect_value = value;
                if let Err(e) = self.prefs.set_auto_connect(value) {
                    log_warn!("gui", "Failed to save auto-connect preference: {}", e);
                }
            }
            Message::MinimizeToTrayToggled(value) => {
                self.minimize_to_tray_value = value;
                if let Err(e) = self.prefs.set_minimize_to_tray(value) {
                    log_warn!("gui", "Failed to save minimize-to-tray preference: {}", e);
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
                            return Task::none();
                        }
                    }
                } else {
                    return Task::none();
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
                    settings: self.chat.clone(),
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
                self.chat.np_command = value;
                self.chat_settings_changed();
            }
            Message::NpFormatChanged(value) => {
                self.chat.np_format = value;
                self.chat_settings_changed();
            }
            Message::ResetNpCommand => {
                self.chat.np_command = DEFAULT_NP_COMMAND.to_string();
                self.chat_settings_changed();
            }
            Message::ResetNpFormat => {
                self.chat.np_format = DEFAULT_NP_FORMAT.to_string();
                self.chat_settings_changed();
            }
            Message::PpCommandChanged(value) => {
                self.chat.pp_command = value;
                self.chat_settings_changed();
            }
            Message::PpFormatChanged(value) => {
                self.chat.pp_format = value;
                self.chat_settings_changed();
            }
            Message::ResetPpCommand => {
                self.chat.pp_command = DEFAULT_PP_COMMAND.to_string();
                self.chat_settings_changed();
            }
            Message::ResetPpFormat => {
                self.chat.pp_format = DEFAULT_PP_FORMAT.to_string();
                self.chat_settings_changed();
            }
            Message::PpScoreFormatChanged(value) => {
                self.chat.pp_score_format = value;
                self.chat_settings_changed();
            }
            Message::ResetPpScoreFormat => {
                self.chat.pp_score_format = DEFAULT_PP_SCORE_FORMAT.to_string();
                self.chat_settings_changed();
            }
            Message::CooldownChanged(value) => {
                self.cooldown_input = value.chars().filter(char::is_ascii_digit).take(4).collect();
                if let Ok(secs) = self.cooldown_input.parse() {
                    self.chat.cooldown_secs = secs;
                    self.chat_settings_changed();
                }
            }
            Message::ResetCooldown => {
                self.chat.cooldown_secs = DEFAULT_COOLDOWN_SECS;
                self.cooldown_input = DEFAULT_COOLDOWN_SECS.to_string();
                self.chat_settings_changed();
            }
            Message::OsuEvent(event) => match event {
                MemoryEvent::StatusChanged(ref status) => {
                    match status {
                        OsuStatus::Connected(client) => {
                            log_info!("osu", "Connected to {}", client);
                        }
                        OsuStatus::Disconnected => {
                            if matches!(self.osu_status, OsuStatus::Connected(_)) {
                                log_info!("osu", "Disconnected");
                            }
                        }
                        _ => {}
                    }
                    self.osu_status = status.clone();
                }
                MemoryEvent::BeatmapChanged(beatmap) => {
                    self.cached_map = beatmap.as_ref().and_then(|b| analyze_map(b).ok());
                    self.current_beatmap = beatmap;
                }
                MemoryEvent::BeatmapDataResponse(_) => {}
            },
            Message::TwitchEvent(event) => match event {
                TwitchEvent::Connected(ref username) => {
                    log_info!("twitch", "Connected as {}", username);
                    self.twitch_status = TwitchStatus::Connected(username.clone());
                }
                TwitchEvent::Disconnected => {
                    log_info!("twitch", "Disconnected");
                    self.twitch_status = TwitchStatus::Disconnected;
                }
                TwitchEvent::Error(ref e) => {
                    if is_invalid_access_token_error(e) || is_transient_network_error(e) {
                        log_warn!("twitch", "Connection error: {}", e);
                    } else {
                        log_error!("twitch", "Connection error: {}", e);
                    }
                    let status_msg = if is_invalid_access_token_error(e) {
                        INVALID_ACCESS_TOKEN_STATUS.to_string()
                    } else {
                        e.clone()
                    };
                    self.twitch_status = TwitchStatus::Error(status_msg);
                }
            },
            Message::LogEvent(entry) => {
                self.log_entries.push_back(entry);
                if self.log_entries.len() > MAX_LOG_ENTRIES {
                    self.log_entries.pop_front();
                }
            }
            Message::LinkClicked(url) => {
                let _ = open::that(url);
            }
            Message::CopyPlaceholder(placeholder) => {
                self.placeholder_copied = true;
                self.placeholder_copies += 1;
                let copy = self.placeholder_copies;

                return Task::batch([
                    clipboard::write(placeholder.to_string()),
                    Task::perform(tokio::time::sleep(PLACEHOLDER_COPIED_NOTICE), move |()| {
                        Message::PlaceholderCopyExpired(copy)
                    }),
                ]);
            }
            Message::PlaceholderCopyExpired(copy) => {
                if copy == self.placeholder_copies {
                    self.placeholder_copied = false;
                }
            }
            Message::OpenLogsClicked => {
                crate::logging::open_log_dir();
            }
            Message::OpenConfigClicked => {
                crate::preferences::open_config_dir();
            }
        }

        Task::none()
    }
}
