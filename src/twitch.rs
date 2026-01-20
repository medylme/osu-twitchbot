use std::fmt::Display;
use std::sync::Arc;

use iced::futures::channel::mpsc;
use iced::futures::stream::{SplitSink, SplitStream};
use iced::futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{self, Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::osu::core::{MemoryEvent, OsuCommand};
use crate::osu::pp::get_pp_spread;
use crate::placeholders::Placeholders;
use crate::{log_debug, log_error, log_info, log_warn};

pub const DEFAULT_NP_COMMAND: &str = "!np";
pub const DEFAULT_NP_FORMAT: &str =
    "{artist} - {title} [{diff}] ({creator}) {mods} | {status} {link}";
pub const DEFAULT_PP_COMMAND: &str = "!pp";
pub const DEFAULT_PP_FORMAT: &str =
    "95%: {pp_95}pp | 97%: {pp_97}pp | 98%: {pp_98}pp | 99%: {pp_99}pp | 100%: {pp_100}pp {mods}";
const SOCKET_KEEPALIVE_SECONDS: u64 = 30;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Default, Clone)]
pub enum TwitchStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected(String),
    Error(String),
}

impl Display for TwitchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TwitchStatus::Disconnected => write!(f, "Disconnected"),
            TwitchStatus::Connecting => write!(f, "Connecting..."),
            TwitchStatus::Connected(user) => write!(f, "Connected as {}", user),
            TwitchStatus::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TwitchCommand {
    Connect {
        token: String,
        np_command: String,
        np_format: String,
        pp_command: String,
        pp_format: String,
    },
    Disconnect,
    UpdatePreferences {
        np_command: Option<String>,
        np_format: Option<String>,
        pp_command: Option<String>,
        pp_format: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum TwitchEvent {
    Connected(String),
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CommandType {
    NowPlaying,
    PerformancePoints,
}

impl Display for CommandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandType::NowPlaying => write!(f, "np"),
            CommandType::PerformancePoints => write!(f, "pp"),
        }
    }
}

struct PendingRequest {
    message_id: String,
    command_type: CommandType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageEvent {
    pub badges: Vec<Badge>,
    pub broadcaster_user_id: String,
    pub broadcaster_user_login: String,
    pub broadcaster_user_name: String,
    pub channel_points_animation_id: Option<String>,
    pub channel_points_custom_reward_id: Option<String>,
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    pub chatter_user_name: String,
    pub cheer: Option<Cheer>,
    pub color: String,
    pub is_source_only: Option<bool>,
    pub message: ChatMessage,
    pub message_id: String,
    pub message_type: ChatMessageType,
    pub reply: Option<Reply>,
    pub source_badges: Option<Vec<Badge>>,
    pub source_broadcaster_user_id: Option<String>,
    pub source_broadcaster_user_login: Option<String>,
    pub source_broadcaster_user_name: Option<String>,
    pub source_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Badge {
    pub set_id: String,
    pub id: String,
    pub info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub text: String,
    pub fragments: Vec<ChatMessageFragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageFragment {
    #[serde(rename = "type")]
    pub fragment_type: FragmentType,
    pub text: String,
    pub cheermote: Option<Cheermote>,
    pub emote: Option<Emote>,
    pub mention: Option<Mention>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FragmentType {
    Text,
    Cheermote,
    Emote,
    Mention,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatMessageType {
    Text,
    ChannelPointsHighlighted,
    ChannelPointsSubOnly,
    UserIntro,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cheermote {
    pub prefix: String,
    pub bits: u32,
    pub tier: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emote {
    pub id: String,
    pub emote_set_id: String,
    pub owner_id: String,
    pub format: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mention {
    pub user_id: String,
    pub user_name: String,
    pub user_login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cheer {
    pub bits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub parent_message_id: String,
    pub parent_message_body: String,
    pub parent_user_id: String,
    pub parent_user_name: String,
    pub parent_user_login: String,
    pub thread_message_id: String,
    pub thread_user_id: String,
    pub thread_user_name: String,
    pub thread_user_login: String,
}

#[derive(Debug, Deserialize)]
struct TwitchResponse {
    data: Vec<TwitchUser>,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub struct TwitchUser {
    pub id: String,
    login: String,
    pub display_name: String,
    #[serde(rename = "type")]
    user_type: String,
    broadcaster_type: String,
    description: String,
    profile_image_url: String,
    offline_image_url: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    #[serde(rename = "type")]
    sub_type: String,
    version: String,
    condition: serde_json::Value,
    transport: Transport,
}

#[derive(Debug, Serialize)]
struct Transport {
    method: String,
    session_id: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventMessage {
    metadata: EventMetadata,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventMetadata {
    message_id: String,
    message_type: String,
    message_timestamp: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WelcomePayload {
    session: SessionData,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
struct SessionData {
    id: String,
    keepalive_timeout_seconds: Option<u64>,
}

type WebSocketType = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Session {
    data: SessionData,
    write: Arc<Mutex<SplitSink<WebSocketType, Message>>>,
    read: Arc<Mutex<SplitStream<WebSocketType>>>,
}

pub struct ChatbotPreferences {
    pub np: CommandConfig,
    pub pp: CommandConfig,
}

pub(crate) struct CommandConfig {
    pub command: Arc<Mutex<String>>,
    pub format: Arc<Mutex<String>>,
}

pub(crate) struct CommandConfigInit {
    pub command: String,
    pub format: String,
}

impl ChatbotPreferences {
    pub fn new(np: CommandConfigInit, pp: CommandConfigInit) -> Self {
        Self {
            np: CommandConfig {
                command: Arc::new(Mutex::new(np.command)),
                format: Arc::new(Mutex::new(np.format)),
            },
            pp: CommandConfig {
                command: Arc::new(Mutex::new(pp.command)),
                format: Arc::new(Mutex::new(pp.format)),
            },
        }
    }
}

impl Default for ChatbotPreferences {
    fn default() -> Self {
        Self::new(
            CommandConfigInit {
                command: DEFAULT_NP_COMMAND.to_string(),
                format: DEFAULT_NP_FORMAT.to_string(),
            },
            CommandConfigInit {
                command: DEFAULT_PP_COMMAND.to_string(),
                format: DEFAULT_PP_FORMAT.to_string(),
            },
        )
    }
}

pub struct TwitchClient {
    client_id: String,
    pub user: TwitchUser,
    session: Session,
    access_token: String,
    http_client: reqwest::Client,
    pub chatbot_preferences: ChatbotPreferences,
}

impl TwitchClient {
    pub async fn new(
        access_token: &str,
        np_command: String,
        np_format: String,
        pp_command: String,
        pp_format: String,
    ) -> Result<Self, BoxError> {
        log_debug!("twitch", "Creating new TwitchClient");
        let client_id = env!("TWITCH_CLIENT_ID");

        let http_client = reqwest::Client::new();

        log_debug!("twitch", "Getting user ID from access token");
        let user = get_user_id_from_access_token(&http_client, client_id, access_token).await?;
        log_debug!("twitch", "Got user: {}", user.display_name);

        log_debug!("twitch", "Initializing websocket session");
        let session = init_websocket_session().await?;

        Ok(Self {
            client_id: client_id.to_string(),
            user,
            session,
            access_token: access_token.to_string(),
            http_client,
            chatbot_preferences: ChatbotPreferences::new(
                CommandConfigInit {
                    command: np_command,
                    format: np_format,
                },
                CommandConfigInit {
                    command: pp_command,
                    format: pp_format,
                },
            ),
        })
    }

    pub async fn update_preferences(
        &self,
        np_command: Option<String>,
        np_format: Option<String>,
        pp_command: Option<String>,
        pp_format: Option<String>,
    ) {
        if let Some(cmd) = np_command {
            let mut command = self.chatbot_preferences.np.command.lock().await;
            *command = cmd;
            log_debug!("twitch", "Updated np_command to: {}", *command);
        }
        if let Some(fmt) = np_format {
            let mut format = self.chatbot_preferences.np.format.lock().await;
            *format = fmt;
            log_debug!("twitch", "Updated np_format to: {}", *format);
        }
        if let Some(cmd) = pp_command {
            let mut command = self.chatbot_preferences.pp.command.lock().await;
            *command = cmd;
            log_debug!("twitch", "Updated pp_command to: {}", *command);
        }
        if let Some(fmt) = pp_format {
            let mut format = self.chatbot_preferences.pp.format.lock().await;
            *format = fmt;
            log_debug!("twitch", "Updated pp_format to: {}", *format);
        }
    }

    pub async fn subscribe_to_channel_messages(&self, channel_id: &str) -> Result<(), BoxError> {
        log_debug!(
            "twitch",
            "Initializing chat message eventsub for user {} in channel {}",
            self.user.id,
            channel_id
        );
        let request = SubscriptionRequest {
            sub_type: "channel.chat.message".to_string(),
            version: "1".to_string(),
            condition: serde_json::json!({
                "broadcaster_user_id": channel_id,
                "user_id": self.user.id
            }),
            transport: Transport {
                method: "websocket".to_string(),
                session_id: self.session.data.id.clone(),
            },
        };

        log_debug!("twitch", "Sending eventsub subscription request");
        let response = self
            .http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .header("Client-ID", self.client_id.clone())
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            log_debug!("twitch", "Failed to subscribe: {}", error_text);
            return Err(format!(
                "Failed to subscribe to channel.chat.message for user {} in channel {}: {}",
                self.user.id, channel_id, error_text
            )
            .into());
        }

        log_debug!("twitch", "Successfully initialized chat message eventsub");
        Ok(())
    }

    pub async fn init_websocket_handler(
        &self,
        osu_tx: mpsc::Sender<OsuCommand>,
        mut osu_rx: mpsc::Receiver<MemoryEvent>,
    ) -> Result<(), BoxError> {
        log_debug!("twitch", "Starting websocket session handler");
        let keepalive_duration = Duration::from_secs(SOCKET_KEEPALIVE_SECONDS);
        let mut last_message = Instant::now();

        let mut pending_request: Option<PendingRequest> = None;
        let mut last_command_time: Option<Instant> = None;
        let rate_limit_duration = Duration::from_secs(1);

        loop {
            let mut read = self.session.read.lock().await;
            let timeout = time::timeout(keepalive_duration, read.next());

            tokio::select! {
                result = timeout => {
                    match result {
                        Ok(Some(Ok(msg))) => {
                            log_debug!("twitch", "Received message from server");
                            last_message = Instant::now();

                            match msg {
                                Message::Text(text) => {
                                    log_debug!("twitch", "Processing text message");
                                    drop(read);
                                    if let Err(e) = self.handle_eventsub_message(
                                        &text,
                                        osu_tx.clone(),
                                        &mut pending_request,
                                        &mut last_command_time,
                                        rate_limit_duration,
                                    ).await {
                                        log_warn!("twitch", "Message error: {}", e);
                                    }
                                }
                                Message::Ping(data) => {
                                    log_debug!("twitch", "Received ping, sending pong");
                                    drop(read);
                                    let mut write = self.session.write.lock().await;
                                    write.send(Message::Pong(data)).await?;
                                }
                                Message::Close(_) => {
                                    log_info!("twitch", "Connection closed by server");
                                    drop(read);
                                    return Ok(());
                                }
                                _ => {
                                    log_debug!("twitch", "Received other message type");
                                    drop(read);
                                }
                            }
                        }
                        Ok(Some(Err(e))) => {
                            log_debug!("twitch", "WebSocket error: {}", e);
                            drop(read);
                            return Err(format!("WebSocket error: {}", e).into());
                        }
                        Ok(None) => {
                            log_debug!("twitch", "WebSocket connection closed");
                            drop(read);
                            return Err("WebSocket connection closed".into());
                        }
                        Err(_) => {
                            log_debug!("twitch", "Timeout waiting for message");
                            if last_message.elapsed() > keepalive_duration {
                                log_debug!("twitch", "Keepalive timeout exceeded");
                                drop(read);
                                return Err("Keepalive timeout".into());
                            }
                            drop(read);
                        }
                    }
                }

                Some(osu_event) = osu_rx.next() => {
                    drop(read);

                    match osu_event {
                        MemoryEvent::BeatmapDataResponse(Some(beatmap_data)) => {
                            log_debug!("twitch", "Received beatmap data response for: {} - {}", beatmap_data.artist, beatmap_data.title);

                            if let Some(request) = pending_request.take() {
                                let message = match request.command_type {
                                    CommandType::NowPlaying => {
                                        let format_template = self.chatbot_preferences.np.format.lock().await.clone();
                                        Placeholders::from_beatmap(&beatmap_data).apply_np(&format_template)
                                    }
                                    CommandType::PerformancePoints => {
                                        let pp_format_template = self.chatbot_preferences.pp.format.lock().await.clone();
                                        match get_pp_spread(
                                            &beatmap_data.mods,
                                            beatmap_data.osu_file_path.as_deref(),
                                            beatmap_data.songs_folder.as_deref(),
                                        ) {
                                            Ok(pp_values) => {
                                                Placeholders::from_beatmap(&beatmap_data)
                                                    .with_pp(&pp_values)
                                                    .apply_pp(&pp_format_template)
                                            }
                                            Err(e) => {
                                                log_debug!("twitch", "pp not available: {}", e);
                                                "pp calculation currently not available".to_string()
                                            }
                                        }
                                    }
                                };

                                if let Err(e) = self.send_chat_message(
                                    &self.user.id,
                                    &message,
                                    Some(&request.message_id)
                                ).await {
                                    log_error!("twitch", "Failed to send chat message: {}", e);
                                }
                            }
                        }
                        MemoryEvent::BeatmapDataResponse(None) => {
                            log_debug!("twitch", "No beatmap data available");

                            if let Some(request) = pending_request.take()
                                && let Err(e) = self.send_chat_message(
                                    &self.user.id,
                                    "No beatmap currently selected",
                                    Some(&request.message_id)
                                ).await {
                                    log_error!("twitch", "Failed to send chat message: {}", e);
                                }
                        }
                        MemoryEvent::BeatmapChanged(_) => {
                            // beatmap changes are handled by the GUI, no action needed here
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn handle_eventsub_message(
        &self,
        message: &str,
        mut osu_tx: mpsc::Sender<OsuCommand>,
        pending_request: &mut Option<PendingRequest>,
        last_command_time: &mut Option<Instant>,
        rate_limit_duration: Duration,
    ) -> Result<(), BoxError> {
        let message: EventMessage = serde_json::from_str(message)?;

        match message.metadata.message_type.as_str() {
            "session_keepalive" => {
                // expected, can ignore
            }
            "notification" => {
                log_debug!(
                    "twitch",
                    "Received notification, subscription type: {:?}",
                    message.metadata.subscription_type
                );
                if message.metadata.subscription_type.as_deref() == Some("channel.chat.message") {
                    let event_data: Option<ChatMessageEvent> = message
                        .payload
                        .get("event")
                        .and_then(|v| serde_json::from_value(v.clone()).ok());

                    if let Some(event) = event_data {
                        let np_command = self.chatbot_preferences.np.command.lock().await.clone();
                        let pp_command = self.chatbot_preferences.pp.command.lock().await.clone();
                        let text = event.message.text.trim();

                        let command_type = if text.starts_with(&np_command) {
                            Some(CommandType::NowPlaying)
                        } else if text.starts_with(&pp_command) {
                            Some(CommandType::PerformancePoints)
                        } else {
                            None
                        };

                        if let Some(cmd_type) = command_type {
                            let now = Instant::now();

                            // rate limiting
                            if let Some(last_time) = last_command_time
                                && now.duration_since(*last_time) < rate_limit_duration
                            {
                                log_debug!("twitch", "Rate limit hit, ignoring command");
                                return Ok(());
                            }

                            log_debug!(
                                "twitch",
                                "Received {} request from {}",
                                cmd_type,
                                event.chatter_user_name
                            );

                            let osu_command = OsuCommand::RequestBeatmapData;

                            if let Err(e) = osu_tx.send(osu_command).await {
                                log_error!("twitch", "Failed to send osu command: {}", e);
                            } else {
                                *pending_request = Some(PendingRequest {
                                    message_id: event.message_id.clone(),
                                    command_type: cmd_type,
                                });
                                *last_command_time = Some(now);
                            }
                        }
                    }
                }
            }
            "session_reconnect" => {
                log_debug!("twitch", "Server requested reconnect");
                return Err("Server requested reconnect".into());
            }
            "revocation" => {
                log_warn!(
                    "twitch",
                    "Subscription revoked: {:?}",
                    message.metadata.subscription_type
                );
            }
            _ => {
                log_debug!(
                    "twitch",
                    "Unknown message type: {}",
                    message.metadata.message_type
                );
            }
        }

        Ok(())
    }

    async fn send_chat_message(
        &self,
        channel_id: &str,
        message: &str,
        reply_parent_message_id: Option<&str>,
    ) -> Result<(), BoxError> {
        log_debug!(
            "twitch",
            "Sending chat message to broadcaster: {}",
            channel_id
        );

        let mut body = serde_json::json!({
            "broadcaster_id": channel_id,
            "sender_id": self.user.id,
            "message": message,
        });

        if reply_parent_message_id.is_some() {
            body["reply_parent_message_id"] = serde_json::json!(reply_parent_message_id);
        }

        let response = self
            .http_client
            .post("https://api.twitch.tv/helix/chat/messages")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Client-ID", self.client_id.clone())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            log_debug!("twitch", "Failed to send chat message: {}", error_text);
            return Err(format!("Failed to send chat message: {}", error_text).into());
        }

        log_debug!("twitch", "Sent response to channel '{}'", channel_id);
        Ok(())
    }
}

async fn init_websocket_session() -> Result<Session, BoxError> {
    log_debug!("twitch", "Connecting to Twitch eventsub WebSocket");
    let url = "wss://eventsub.wss.twitch.tv/ws";

    let (ws_stream, _response) = connect_async(url).await?;
    log_debug!("twitch", "WebSocket connected, waiting for welcome message");
    let (mut write, mut read) = ws_stream.split();

    let welcome_text = loop {
        log_debug!("twitch", "Waiting for message from server");
        let msg = read
            .next()
            .await
            .ok_or_else(|| -> BoxError { "Connection closed before welcome".into() })??;

        match msg {
            Message::Text(text) => {
                log_debug!("twitch", "Received text message");
                break text;
            }
            Message::Ping(data) => {
                log_debug!("twitch", "Received ping, sending pong");
                write.send(Message::Pong(data)).await?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .as_ref()
                    .map(|f| format!("code: {}, reason: {}", f.code, f.reason))
                    .unwrap_or_else(|| "unknown".to_string());
                log_debug!("twitch", "WebSocket closed immediately: {}", reason);
                return Err(format!("WebSocket closed immediately: {}", reason).into());
            }
            Message::Pong(_) => {
                log_debug!("twitch", "Received pong");
            }
            other => {
                log_debug!("twitch", "Unexpected message type: {:?}", other);
                return Err(format!("Unexpected message type: {:?}", other).into());
            }
        }
    };

    if welcome_text.is_empty() {
        log_debug!("twitch", "Received empty welcome message");
        return Err("Received empty welcome message".into());
    }

    log_debug!("twitch", "Parsing welcome message");
    let welcome: EventMessage = serde_json::from_str(&welcome_text).map_err(|e| -> BoxError {
        format!(
            "Failed to parse welcome message: {}. Raw: {}",
            e, welcome_text
        )
        .into()
    })?;

    if welcome.metadata.message_type != "session_welcome" {
        log_debug!(
            "twitch",
            "Expected session_welcome, got: {}",
            welcome.metadata.message_type
        );
        return Err("Expected session_welcome message".into());
    }

    let welcome_payload: WelcomePayload = serde_json::from_value(welcome.payload)?;
    log_debug!("twitch", "Welcome message parsed successfully");

    Ok(Session {
        data: welcome_payload.session,
        read: Arc::new(Mutex::new(read)),
        write: Arc::new(Mutex::new(write)),
    })
}

async fn get_user_id_from_access_token(
    http_client: &reqwest::Client,
    client_id: &str,
    access_token: &str,
) -> Result<TwitchUser, BoxError> {
    log_debug!("twitch", "Getting user data from access token");
    let response: TwitchResponse = http_client
        .get("https://api.twitch.tv/helix/users")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Client-Id", client_id)
        .send()
        .await?
        .json()
        .await?;

    if let Some(user) = response.data.first() {
        log_debug!("twitch", "Got user: {}", user.display_name);
        Ok(user.clone())
    } else {
        log_debug!("twitch", "No user data in response");
        Err("Failed to get user data".into())
    }
}
