use std::collections::HashMap;

use iced::futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::time::{self, Duration};

use super::core::{
    BeatmapData, BeatmapStatus, DATA_POLLING_INTERVAL_MS, GameplayMods, MemoryError, MemoryEvent,
    ModInfo, OsuCommand, OsuStatus, ProcessMemory, order_mods, parse_pattern,
};
use crate::{log_debug, log_error, log_info, log_warn};

// compares version strings in hashmap to get latest
fn get_latest_version(offsets_map: &HashMap<String, Offsets>) -> Option<&str> {
    offsets_map
        .keys()
        .max_by(|a, b| {
            let parse_version =
                |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
            parse_version(a).cmp(&parse_version(b))
        })
        .map(|s| s.as_str())
}

pub async fn run_lazer_reader(
    pid: u32,
    version: Option<String>,
    tx: &mut iced::futures::channel::mpsc::Sender<MemoryEvent>,
    cmd_rx: &mut iced::futures::channel::mpsc::Receiver<OsuCommand>,
    forward_tx: &mut iced::futures::channel::mpsc::Sender<MemoryEvent>,
    current_beatmap: &mut Option<BeatmapData>,
) -> Result<(), MemoryError> {
    log_debug!(
        "memory-lazer",
        "Starting lazer reader with version: {:?}",
        version
    );

    let _ = tx
        .send(MemoryEvent::StatusChanged(OsuStatus::Initializing))
        .await;

    let all_offsets_json = include_str!("../../offsets/lazer.json");
    let offsets_map: HashMap<String, Offsets> =
        serde_json::from_str(all_offsets_json).map_err(|e| {
            log_error!("memory-lazer", "Failed to parse offsets file: {}", e);
            MemoryError::ReadFailed(format!("Failed to parse offsets: {}", e))
        })?;

    let (used_version, offsets_json) = match &version {
        Some(v) if offsets_map.contains_key(v) => {
            log_info!("memory-lazer", "Using offsets for version {}", v);
            (v.clone(), serde_json::to_string(&offsets_map[v]).unwrap())
        }
        Some(v) => {
            let latest = get_latest_version(&offsets_map).unwrap_or("unknown");
            log_warn!(
                "memory-lazer",
                "Version {} not found in offsets, using latest ({})",
                v,
                latest
            );
            (
                latest.to_string(),
                serde_json::to_string(&offsets_map[latest]).unwrap(),
            )
        }
        None => {
            let latest = get_latest_version(&offsets_map).unwrap_or("unknown");
            log_info!(
                "memory-lazer",
                "Version not detected, using latest offsets ({})",
                latest
            );
            (
                latest.to_string(),
                serde_json::to_string(&offsets_map[latest]).unwrap(),
            )
        }
    };

    let reader = tokio::task::spawn_blocking(move || {
        LazerReader::new(pid, &offsets_json).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| MemoryError::ReadFailed(format!("Task panic: {}", e)))?
    .map_err(MemoryError::ReadFailed)?;

    let _ = tx
        .send(MemoryEvent::StatusChanged(OsuStatus::Connected(format!(
            "Lazer {} (pid {})",
            used_version, pid
        ))))
        .await;

    let mut interval = time::interval(Duration::from_millis(DATA_POLLING_INTERVAL_MS));
    let mut last_beatmap_id: Option<i32> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let result = {
                    let mut reader = reader.clone();
                    tokio::task::spawn_blocking(move || {
                        reader
                            .read_beatmap()
                            .map_err(|e| MemoryError::ReadFailed(e.to_string()))
                    })
                    .await
                };

                match result {
                    Ok(Ok(beatmap)) => {
                        let mods_changed = current_beatmap.as_ref().map(|b| &b.mods) != Some(&beatmap.mods);
                        let beatmap_changed = last_beatmap_id != Some(beatmap.id);

                        if beatmap_changed || mods_changed {
                            last_beatmap_id = Some(beatmap.id);
                            *current_beatmap = Some(beatmap.clone());
                            let _ = tx.send(MemoryEvent::BeatmapChanged(Some(beatmap))).await;
                        }
                    }
                    Ok(Err(e)) => {
                        let error_str = e.to_string();

                        if error_str.contains("no beatmap")
                            || error_str.contains("not initialized")
                            || error_str.contains("null")
                            || error_str.contains("invalid")
                        {
                            if current_beatmap.is_some() {
                                *current_beatmap = None;
                                let _ = tx.send(MemoryEvent::BeatmapChanged(None)).await;
                                last_beatmap_id = None;
                            }
                            continue;
                        }

                        return Err(e);
                    }
                    Err(e) => {
                        return Err(MemoryError::ReadFailed(format!("Task panic: {}", e)));
                    }
                }
            }

            Some(cmd) = cmd_rx.next() => {
                match cmd {
                    OsuCommand::RequestBeatmapData => {
                        let event = MemoryEvent::BeatmapDataResponse(current_beatmap.clone());
                        let _ = tx.send(event.clone()).await;
                        let _ = forward_tx.send(event).await;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Offsets {
    patterns: Patterns,
    base: BaseOffsets,
    external_link_opener: ExternalLinkOpener,
    api_access: ApiAccess,
    osu_game: OsuGame,
    screen_stack: ScreenStack,
    osu_game_base: OsuGameBase,
    working_beatmap: WorkingBeatmap,
    beatmap_info: BeatmapInfo,
    beatmap_metadata: BeatmapMetadata,
    realm_user: RealmUser,
    player: Player,
    score_info: ScoreInfo,
    #[serde(default)]
    storage: StorageOffsets,
    #[serde(default)]
    wrapped_storage: WrappedStorageOffsets,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Patterns {
    base: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BaseOffsets {
    external_link_opener: isize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ExternalLinkOpener {
    api: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ApiAccess {
    game: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OsuGame {
    screen_stack: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ScreenStack {
    stack: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OsuGameBase {
    beatmap: usize,
    #[serde(default)]
    storage: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WorkingBeatmap {
    beatmap_info: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BeatmapInfo {
    online_id: usize,
    metadata: usize,
    difficulty_name: usize,
    status: usize,
    #[serde(default)]
    hash: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BeatmapMetadata {
    title: usize,
    artist: usize,
    author: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RealmUser {
    username: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Player {
    score: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ScoreInfo {
    mods_json: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct StorageOffsets {
    base_path: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct WrappedStorageOffsets {
    underlying_storage: usize,
}

#[derive(Clone)]
pub struct LazerReader<'a> {
    offsets: Offsets,
    process: &'a ProcessMemory,
    game_base: usize,
}

impl<'a> LazerReader<'a> {
    pub fn new(
        pid: u32,
        offsets_json: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let offsets: Offsets = serde_json::from_str(offsets_json).map_err(|e| {
            log_error!("memory-lazer", "Failed to parse offsets JSON: {}", e);
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;

        let process = match ProcessMemory::new(pid) {
            Ok(p) => p,
            Err(e) => {
                log_error!("memory-lazer", "Failed to open process: {}", e);
                log_error!("memory-lazer", "Try running with admin/root privileges.");
                return Err(Box::new(e));
            }
        };

        log_debug!("memory-lazer", "Scanning for base address...");

        let (pattern, mask) = parse_pattern(&offsets.patterns.base);

        let scaling_container_target_draw_size = match process.pattern_scan(&pattern, &mask) {
            Ok(addr) => {
                log_debug!("memory-lazer", "Found pattern at: 0x{:X}", addr);
                addr
            }
            Err(e) => {
                log_error!("memory-lazer", "Failed to find pattern: {}", e);
                return Err(Box::new(e));
            }
        };

        // from scaling container draw size pattern, we read a static reference to ExternalLinkOpener
        let external_link_opener_addr = (scaling_container_target_draw_size as isize
            + offsets.base.external_link_opener) as usize;
        log_debug!(
            "memory-lazer",
            "ExternalLinkOpener address: 0x{:X}",
            external_link_opener_addr
        );
        let external_link_opener = match process.read_ptr(external_link_opener_addr) {
            Ok(ptr) => {
                if ptr == 0 {
                    log_error!("memory-lazer", "ExternalLinkOpener pointer is null");
                    return Err("ExternalLinkOpener pointer is null".into());
                }
                log_debug!("memory-lazer", "ExternalLinkOpener value: 0x{:X}", ptr);
                ptr
            }
            Err(e) => {
                log_error!("memory-lazer", "Failed to read ExternalLinkOpener: {}", e);
                log_error!(
                    "memory-lazer",
                    "This might mean the pattern offset is incorrect."
                );
                return Err(Box::new(e));
            }
        };

        // we then traverse to the API object
        let api_ptr_addr = external_link_opener + offsets.external_link_opener.api;
        log_debug!("memory-lazer", "API pointer address: 0x{:X}", api_ptr_addr);
        let api = match process.read_ptr(api_ptr_addr) {
            Ok(ptr) => {
                if ptr == 0 {
                    log_error!("memory-lazer", "API pointer is null");
                    return Err("API pointer is null".into());
                }
                log_debug!("memory-lazer", "API value: 0x{:X}", ptr);
                ptr
            }
            Err(e) => {
                log_error!("memory-lazer", "Failed to read API: {}", e);
                return Err(Box::new(e));
            }
        };

        // and finally find the GameBase
        let game_base_addr = api + offsets.api_access.game;
        log_debug!(
            "memory-lazer",
            "Game base address location: 0x{:X}",
            game_base_addr
        );
        let game_base = match process.read_ptr(game_base_addr) {
            Ok(ptr) => {
                if ptr == 0 {
                    log_error!("memory-lazer", "Game base pointer is null");
                    return Err("Game base pointer is null".into());
                }
                log_debug!("memory-lazer", "Game base value: 0x{:X}", ptr);

                if process.read_ptr(ptr).is_err() {
                    log_warn!(
                        "memory-lazer",
                        "Cannot read vtable at game base, address might be invalid"
                    );
                }

                ptr
            }
            Err(e) => {
                log_error!("memory-lazer", "Failed to read game base: {}", e);
                return Err(Box::new(e));
            }
        };

        log_debug!("memory-lazer", "Found game base at: 0x{:X}", game_base);

        Ok(Self {
            offsets,
            process: Box::leak(Box::new(process)),
            game_base,
        })
    }

    fn get_current_screen(&self) -> Option<usize> {
        let screen_stack = self
            .process
            .read_ptr(self.game_base + self.offsets.osu_game.screen_stack)
            .ok()?;

        if screen_stack == 0 {
            return None;
        }

        let stack = self
            .process
            .read_ptr(screen_stack + self.offsets.screen_stack.stack)
            .ok()?;

        if stack == 0 {
            return None;
        }

        let count = self.process.read_i32(stack + 0x10).ok()?;
        if count <= 0 {
            return None;
        }

        let items = self.process.read_ptr(stack + 0x8).ok()?;
        if items == 0 {
            return None;
        }

        let screen = self
            .process
            .read_ptr(items + 0x10 + 0x8 * (count as usize - 1))
            .ok()?;

        if screen == 0 { None } else { Some(screen) }
    }

    fn try_get_score_info_from_player(&self, screen: usize) -> Option<usize> {
        let score = self
            .process
            .read_ptr(screen + self.offsets.player.score)
            .ok()?;

        if score == 0 {
            return None;
        }

        let score_info = self.process.read_ptr(score + 0x8).ok()?;

        if score_info == 0 {
            None
        } else {
            Some(score_info)
        }
    }

    fn read_mods_from_score_info(&self, score_info: usize) -> Option<GameplayMods> {
        let mods_json_addr = score_info + self.offsets.score_info.mods_json;
        let mods_json = read_csharp_string(self.process, mods_json_addr).ok()?;

        if mods_json.is_empty() || mods_json == "[]" {
            return Some(GameplayMods {
                mods: vec![],
                mods_string: "NoMod".to_string(),
            });
        }

        let mods: Vec<ModInfo> = match serde_json::from_str(&mods_json) {
            Ok(m) => m,
            Err(e) => {
                log_debug!(
                    "memory-lazer",
                    "Failed to parse mods JSON: {} - raw: '{}'",
                    e,
                    mods_json
                );
                return None;
            }
        };

        let mods_string = if mods.is_empty() {
            "NoMod".to_string()
        } else {
            let unsorted = mods
                .iter()
                .map(|m| m.acronym.clone())
                .collect::<Vec<_>>()
                .join("");

            order_mods(&unsorted)
        };

        Some(GameplayMods { mods, mods_string })
    }

    pub fn read_gameplay_mods(&self) -> Option<GameplayMods> {
        let current_screen = self.get_current_screen()?;
        let score_info = self.try_get_score_info_from_player(current_screen)?;
        self.read_mods_from_score_info(score_info)
    }

    pub fn read_beatmap(&mut self) -> Result<BeatmapData, MemoryError> {
        let unknown_data = BeatmapData {
            id: 0,
            artist: "?".to_string(),
            title: "?".to_string(),
            difficulty_name: "?".to_string(),
            creator: "?".to_string(),
            status: BeatmapStatus::Unknown,
            mods: None,
            osu_file_path: None,
            songs_folder: None,
        };

        if self.game_base == 0 {
            return Err(MemoryError::ReadFailed("Game base not set.".to_string()));
        }

        let beatmap_bindable = match self
            .process
            .read_ptr(self.game_base + self.offsets.osu_game_base.beatmap)
        {
            Ok(ptr) => {
                if ptr == 0 {
                    return Ok(unknown_data);
                }
                ptr
            }
            Err(e) => {
                return Err(MemoryError::ReadFailed(format!(
                    "Failed to read beatmap bindable: {}",
                    e
                )));
            }
        };

        let working_beatmap = match self.process.read_ptr(beatmap_bindable + 0x20) {
            Ok(ptr) => {
                if ptr == 0 {
                    return Ok(unknown_data);
                }
                ptr
            }
            Err(e) => {
                return Err(MemoryError::ReadFailed(format!(
                    "Failed to read working beatmap: {}",
                    e
                )));
            }
        };

        let beatmap_info = match self
            .process
            .read_ptr(working_beatmap + self.offsets.working_beatmap.beatmap_info)
        {
            Ok(ptr) => {
                if ptr == 0 {
                    return Ok(unknown_data);
                }
                ptr
            }
            Err(e) => {
                return Err(MemoryError::ReadFailed(format!(
                    "Failed to read beatmap info: {}",
                    e
                )));
            }
        };

        let metadata = self
            .process
            .read_ptr(beatmap_info + self.offsets.beatmap_info.metadata)
            .unwrap_or_default();

        let author = match metadata {
            0 => 0,
            _ => self
                .process
                .read_ptr(metadata + self.offsets.beatmap_metadata.author)
                .unwrap_or(0),
        };

        let id = self
            .process
            .read_i32(beatmap_info + self.offsets.beatmap_info.online_id)
            .unwrap_or(0);

        let status_int = self
            .process
            .read_i32(beatmap_info + self.offsets.beatmap_info.status)
            .unwrap_or(-1);

        let status = match status_int {
            -4 => BeatmapStatus::NotSubmitted,
            -2 => BeatmapStatus::Graveyard,
            -1 => BeatmapStatus::Wip,
            0 => BeatmapStatus::Pending,
            1 => BeatmapStatus::Ranked,
            2 => BeatmapStatus::Approved,
            3 => BeatmapStatus::Qualified,
            4 => BeatmapStatus::Loved,
            _ => BeatmapStatus::Unknown,
        };

        let title = if metadata != 0 {
            read_csharp_string(self.process, metadata + self.offsets.beatmap_metadata.title)
                .unwrap_or_else(|_| "?".to_string())
        } else {
            "?".to_string()
        };

        let artist = if metadata != 0 {
            read_csharp_string(
                self.process,
                metadata + self.offsets.beatmap_metadata.artist,
            )
            .unwrap_or_else(|_| "?".to_string())
        } else {
            "?".to_string()
        };

        let difficulty_name = read_csharp_string(
            self.process,
            beatmap_info + self.offsets.beatmap_info.difficulty_name,
        )
        .unwrap_or_else(|_| "?".to_string());

        let creator = if author != 0 {
            read_csharp_string(self.process, author + self.offsets.realm_user.username)
                .unwrap_or_else(|_| "?".to_string())
        } else {
            "?".to_string()
        };

        let mods = self.read_gameplay_mods();

        let (osu_file_path, songs_folder) = self.read_beatmap_file_info(beatmap_info);

        Ok(BeatmapData {
            id,
            artist,
            title,
            difficulty_name,
            creator,
            status,
            mods,
            osu_file_path,
            songs_folder,
        })
    }

    fn read_beatmap_file_info(&self, beatmap_info: usize) -> (Option<String>, Option<String>) {
        let hash = if self.offsets.beatmap_info.hash != 0 {
            read_csharp_string(self.process, beatmap_info + self.offsets.beatmap_info.hash).ok()
        } else {
            None
        };

        let base_path = if self.offsets.osu_game_base.storage != 0 {
            self.read_storage_base_path()
        } else {
            None
        };

        match (hash, base_path) {
            (Some(h), Some(base)) if h.len() >= 2 => {
                let file_path = format!("{}/{}/{}", &h[0..1], &h[0..2], &h);
                let files_folder = format!("{}/files", base);
                (Some(file_path), Some(files_folder))
            }
            _ => (None, None),
        }
    }

    fn read_storage_base_path(&self) -> Option<String> {
        let storage = self
            .process
            .read_ptr(self.game_base + self.offsets.osu_game_base.storage)
            .ok()?;

        if storage == 0 {
            return None;
        }

        // unwrap WrappedStorage or return directly
        let underlying = if self.offsets.wrapped_storage.underlying_storage != 0 {
            self.process
                .read_ptr(storage + self.offsets.wrapped_storage.underlying_storage)
                .unwrap_or(storage)
        } else {
            storage
        };

        if underlying == 0 {
            return None;
        }

        if self.offsets.storage.base_path != 0 {
            read_csharp_string(self.process, underlying + self.offsets.storage.base_path).ok()
        } else {
            None
        }
    }
}

fn read_csharp_string(process: &ProcessMemory, addr: usize) -> Result<String, MemoryError> {
    let str_ptr = process.read_ptr(addr)?;
    if str_ptr == 0 {
        return Ok(String::new());
    }

    let length = process.read_i32(str_ptr + 0x8)? as usize;

    if length == 0 || length > 10000 {
        return Ok(String::new());
    }

    let mut buffer = vec![0u16; length];
    for (i, item) in buffer.iter_mut().enumerate().take(length) {
        *item = process.read_u16(str_ptr + 0xC + (i * 2))?;
    }

    String::from_utf16(&buffer).map_err(|_| MemoryError::InvalidString)
}
