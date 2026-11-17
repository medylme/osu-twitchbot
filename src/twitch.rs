use std::fmt::Display;
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::time::{self, Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::osu::core::{BeatmapData, MemoryEvent, OsuCommand};
use crate::osu::pp::{PpArgs, analyze_map, calculate_score_pp};
use crate::placeholders::Placeholders;
use crate::{log_debug, log_error, log_info, log_warn};

pub const DEFAULT_NP_COMMAND: &str = "!np";
pub const DEFAULT_NP_FORMAT: &str =
    "{artist} - {title} [{diff}] ({creator}) {mods} | {status} {link}";
pub const DEFAULT_PP_COMMAND: &str = "!pp";
pub const DEFAULT_PP_FORMAT: &str =
    "95%: {pp_95}pp | 97%: {pp_97}pp | 98%: {pp_98}pp | 99%: {pp_99}pp | 100%: {pp_100}pp ({mods})";
pub const DEFAULT_PP_SCORE_FORMAT: &str = "{acc}% | {combo}x | {misses} miss | {pp}pp ({mods})";
pub const DEFAULT_COOLDOWN_SECS: u64 = 1;
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

pub const INVALID_ACCESS_TOKEN_STATUS: &str = "Please regenerate your token!";

pub fn is_invalid_access_token_error(message: &str) -> bool {
    message.contains("Invalid OAuth token")
}

pub fn is_transient_network_error(message: &str) -> bool {
    message.contains("Keepalive timeout")
        || message.contains("Server requested reconnect")
        || message.contains("os error 10053")
        || message.contains("os error 10054")
        || message.contains("os error 10060")
        || message.contains("ConnectionReset")
}

#[derive(Debug, Clone)]
pub enum TwitchCommand {
    Connect {
        token: String,
        settings: ChatSettings,
    },
    Disconnect,
    UpdateSettings(ChatSettings),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatSettings {
    pub np_command: String,
    pub np_format: String,
    pub pp_command: String,
    pub pp_format: String,
    pub pp_score_format: String,
    pub cooldown_secs: u64,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            np_command: DEFAULT_NP_COMMAND.to_string(),
            np_format: DEFAULT_NP_FORMAT.to_string(),
            pp_command: DEFAULT_PP_COMMAND.to_string(),
            pp_format: DEFAULT_PP_FORMAT.to_string(),
            pp_score_format: DEFAULT_PP_SCORE_FORMAT.to_string(),
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
        }
    }
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

#[derive(Debug, Deserialize)]
struct HelixErrorBody {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    status: Option<u16>,
    #[serde(default)]
    message: Option<String>,
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

pub struct TwitchClient {
    client_id: String,
    pub user: TwitchUser,
    session: Session,
    access_token: String,
    http_client: reqwest::Client,
    settings: Mutex<ChatSettings>,
}

impl TwitchClient {
    pub async fn new(access_token: &str, settings: ChatSettings) -> Result<Self, BoxError> {
        log_debug!("twitch", "Creating new TwitchClient");
        let client_id = env!("TWITCH_CLIENT_ID");

        let http_client = reqwest::Client::new();

        log_debug!("twitch", "Validating access token scopes");
        validate_token_scopes(&http_client, access_token).await?;

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
            settings: Mutex::new(settings),
        })
    }

    pub async fn update_settings(&self, settings: ChatSettings) {
        log_debug!("twitch", "Updated chat settings: {:?}", settings);
        *self.settings.lock().await = settings;
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

        let mut latest_beatmap: Option<BeatmapData> = None;
        let _ = osu_tx.send(OsuCommand::RequestBeatmapData).await;

        let mut last_command_time: Option<Instant> = None;

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
                                        &latest_beatmap,
                                        &mut last_command_time,
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

                Some(osu_event) = osu_rx.recv() => {
                    drop(read);

                    match osu_event {
                        MemoryEvent::BeatmapDataResponse(beatmap)
                        | MemoryEvent::BeatmapChanged(beatmap) => {
                            latest_beatmap = beatmap;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn build_reply(
        settings: &ChatSettings,
        command_type: &CommandType,
        beatmap: &BeatmapData,
        args: &str,
    ) -> String {
        match command_type {
            CommandType::NowPlaying => {
                let mut placeholders = Placeholders::from_beatmap(beatmap);

                if Placeholders::uses_stats(&settings.np_format) {
                    let analysis = analyze_map(beatmap)
                        .inspect_err(|e| log_debug!("twitch", "map stats not available: {}", e))
                        .ok();
                    placeholders = placeholders.with_stats(analysis.as_ref().map(|a| &a.stats));
                }

                placeholders.apply_np(&settings.np_format)
            }
            CommandType::PerformancePoints => {
                let args = PpArgs::parse(args);
                let beatmap = &BeatmapData {
                    mods: args.mods.clone().or_else(|| beatmap.mods.clone()),
                    ..beatmap.clone()
                };
                let placeholders = Placeholders::from_beatmap(beatmap);

                let reply = if args.is_score() {
                    calculate_score_pp(beatmap, &args).map(|score| {
                        placeholders
                            .with_score(&score)
                            .apply_pp_score(&settings.pp_score_format)
                    })
                } else {
                    analyze_map(beatmap).map(|analysis| {
                        placeholders
                            .with_pp(&analysis.pp)
                            .apply_pp(&settings.pp_format)
                    })
                };

                reply.unwrap_or_else(|e| {
                    log_debug!("twitch", "pp not available: {}", e);
                    "pp calculation currently not available".to_string()
                })
            }
        }
    }

    async fn handle_eventsub_message(
        &self,
        message: &str,
        latest_beatmap: &Option<BeatmapData>,
        last_command_time: &mut Option<Instant>,
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
                        let settings = self.settings.lock().await.clone();
                        let np_command = settings.np_command.trim();
                        let pp_command = settings.pp_command.trim();
                        let text = event.message.text.trim();
                        let (first_word, args) =
                            text.split_once(char::is_whitespace).unwrap_or((text, ""));

                        let command_type = if !np_command.is_empty()
                            && first_word.eq_ignore_ascii_case(np_command)
                        {
                            Some(CommandType::NowPlaying)
                        } else if !pp_command.is_empty()
                            && first_word.eq_ignore_ascii_case(pp_command)
                        {
                            Some(CommandType::PerformancePoints)
                        } else {
                            None
                        };

                        if let Some(cmd_type) = command_type {
                            let now = Instant::now();

                            let cooldown = Duration::from_secs(settings.cooldown_secs);
                            if let Some(last_time) = last_command_time
                                && now.duration_since(*last_time) < cooldown
                            {
                                log_debug!("twitch", "On cooldown, ignoring command");
                                return Ok(());
                            }

                            log_debug!(
                                "twitch",
                                "Received {} request from {}",
                                cmd_type,
                                event.chatter_user_name
                            );

                            let reply = match latest_beatmap {
                                Some(beatmap) => {
                                    Self::build_reply(&settings, &cmd_type, beatmap, args)
                                }
                                None => "No beatmap currently selected".to_string(),
                            };

                            if let Err(e) = self
                                .send_chat_message(&self.user.id, &reply, Some(&event.message_id))
                                .await
                            {
                                if is_transient_network_error(&e.to_string()) {
                                    log_warn!("twitch", "Failed to send chat message: {}", e);
                                } else {
                                    log_error!("twitch", "Failed to send chat message: {}", e);
                                }
                            }
                            *last_command_time = Some(now);
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

        let response_body: serde_json::Value = response.json().await.unwrap_or_default();
        let sent_message = response_body.get("data").and_then(|d| d.get(0));
        if let Some(sent_message) = sent_message
            && sent_message.get("is_sent").and_then(|v| v.as_bool()) == Some(false)
        {
            let drop_reason = sent_message
                .get("drop_reason")
                .and_then(|r| r.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown reason");
            log_warn!("twitch", "Chat message was dropped: {}", drop_reason);
            return Ok(());
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

const REQUIRED_TOKEN_SCOPES: [&str; 2] = ["user:read:chat", "user:write:chat"];

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    #[serde(default)]
    scopes: Vec<String>,
}

async fn validate_token_scopes(
    http_client: &reqwest::Client,
    access_token: &str,
) -> Result<(), BoxError> {
    let response = http_client
        .get("https://id.twitch.tv/oauth2/validate")
        .header("Authorization", format!("OAuth {}", access_token))
        .send()
        .await?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Invalid OAuth token".into());
    }

    let parsed = if status.is_success() {
        response.json::<ValidateResponse>().await.ok()
    } else {
        None
    };

    let Some(parsed) = parsed else {
        log_warn!(
            "twitch",
            "Token validation inconclusive ({}), skipping scope check",
            status
        );
        return Ok(());
    };

    let missing: Vec<&str> = REQUIRED_TOKEN_SCOPES
        .iter()
        .filter(|required| !parsed.scopes.iter().any(|s| s == *required))
        .copied()
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Token is missing the {} scope(s) - please regenerate your token",
            missing.join(", ")
        )
        .into())
    }
}

async fn get_user_id_from_access_token(
    http_client: &reqwest::Client,
    client_id: &str,
    access_token: &str,
) -> Result<TwitchUser, BoxError> {
    log_debug!("twitch", "Getting user data from access token");
    let response = http_client
        .get("https://api.twitch.tv/helix/users")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Client-Id", client_id)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        let detail = serde_json::from_str::<HelixErrorBody>(&body).ok();
        let msg = match detail {
            Some(e) => {
                let mut parts = Vec::new();
                if let Some(m) = e.message.filter(|s| !s.is_empty()) {
                    parts.push(m);
                } else if let Some(err) = e.error.filter(|s| !s.is_empty()) {
                    parts.push(err);
                }
                if parts.is_empty() {
                    body
                } else {
                    parts.join(": ")
                }
            }
            None => body,
        };
        return Err(format!("Twitch API {}: {}", status, msg).into());
    }

    let parsed: TwitchResponse = serde_json::from_str(&body).map_err(|e| {
        format!(
            "Unexpected Twitch /users response ({}): {} — body: {}",
            status, e, body
        )
    })?;

    if let Some(user) = parsed.data.first() {
        log_debug!("twitch", "Got user: {}", user.display_name);
        Ok(user.clone())
    } else {
        log_debug!("twitch", "No user data in response");
        Err("Failed to get user data (empty data array)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osu::core::{BeatmapStatus, GameplayMods, ModInfo, OsuClient};

    const MAP: &str = "osu file format v14

[General]
Mode: 0

[Difficulty]
HPDrainRate:5
CircleSize:4
OverallDifficulty:8
ApproachRate:9
SliderMultiplier:1.4
SliderTickRate:1

[TimingPoints]
0,500,4,2,0,100,1,0

[HitObjects]
64,64,0,1,0,0:0:0:0
192,64,250,1,0,0:0:0:0
320,64,500,1,0,0:0:0:0
448,64,750,1,0,0:0:0:0
";

    fn pp_reply(name: &str, args: &str) -> String {
        let dir =
            std::env::temp_dir().join(format!("osu-twitchbot-reply-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("map.osu"), MAP).unwrap();

        let beatmap = BeatmapData {
            client: OsuClient::Lazer,
            id: 0,
            artist: "artist".to_string(),
            title: "title".to_string(),
            difficulty_name: "diff".to_string(),
            creator: "creator".to_string(),
            status: BeatmapStatus::NotSubmitted,
            mods: Some(GameplayMods {
                mods: vec![ModInfo {
                    acronym: "HD".to_string(),
                    settings: None,
                }],
                mods_string: "HD".to_string(),
            }),
            osu_file_path: Some("map.osu".to_string()),
            songs_folder: dir.to_str().map(str::to_string),
            file_stamp: None,
        };

        let settings = ChatSettings {
            pp_format: "spread {pp_100} {mods}".to_string(),
            pp_score_format: "score {acc}% {combo}x {misses}m {pp} {mods}".to_string(),
            ..ChatSettings::default()
        };
        let reply =
            TwitchClient::build_reply(&settings, &CommandType::PerformancePoints, &beatmap, args);

        std::fs::remove_dir_all(&dir).unwrap();
        reply
    }

    #[test]
    fn plain_pp_uses_the_selected_mods() {
        let reply = pp_reply("plain", "");

        assert!(reply.starts_with("spread "), "{reply}");
        assert!(reply.ends_with(" +HD"), "{reply}");
    }

    #[test]
    fn mods_alone_override_the_spread() {
        let hd = pp_reply("mods-hd", "");
        let dt = pp_reply("mods-dt", "+DT");

        assert!(dt.starts_with("spread "), "{dt}");
        assert!(dt.ends_with(" +DT"), "{dt}");
        assert_ne!(hd, dt);
    }

    #[test]
    fn score_arguments_use_the_score_format() {
        let reply = pp_reply("score", "75 3x 1m +nm");

        assert!(reply.starts_with("score 75% 3x 1m "), "{reply}");
        assert!(reply.ends_with(" +NoMod"), "{reply}");
    }
}
