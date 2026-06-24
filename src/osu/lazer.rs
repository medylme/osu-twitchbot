use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

use super::core::{
    BeatmapData, BeatmapStatus, DATA_POLLING_INTERVAL_MS, GameplayMods, MemoryError, MemoryEvent,
    ModInfo, OsuCommand, OsuStatus, ProcessMemory, order_mods, parse_pattern, privilege_hint,
};
use crate::{log_debug, log_error, log_info, log_warn};

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
    tx: &mut mpsc::Sender<MemoryEvent>,
    cmd_rx: &mut mpsc::Receiver<OsuCommand>,
    forward_tx: &mut mpsc::Sender<MemoryEvent>,
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

    let mut reader = tokio::task::spawn_blocking(move || {
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
                    let mut r = reader.clone();
                    tokio::task::spawn_blocking(move || {
                        let res = r.read_beatmap()
                            .map_err(|e| MemoryError::ReadFailed(e.to_string()));
                        (res, r.mod_vtable_map)
                    })
                    .await
                };

                match result {
                    Ok((Ok(beatmap), vtable_map)) => {
                        if reader.mod_vtable_map.is_empty() && !vtable_map.is_empty() {
                            reader.mod_vtable_map = vtable_map;
                        }
                        let mods_changed = current_beatmap.as_ref().map(|b| &b.mods) != Some(&beatmap.mods);
                        let beatmap_changed = last_beatmap_id != Some(beatmap.id);

                        if beatmap_changed || mods_changed {
                            last_beatmap_id = Some(beatmap.id);
                            *current_beatmap = Some(beatmap.clone());
                            let _ = tx.send(MemoryEvent::BeatmapChanged(Some(beatmap))).await;
                        }
                    }
                    Ok((Err(e), _)) => {
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

            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    OsuCommand::RequestBeatmapData => {
                        let event = MemoryEvent::BeatmapDataResponse(current_beatmap.clone());
                        let _ = tx.send(event.clone()).await;
                        let _ = forward_tx.send(event).await;
                    }
                    OsuCommand::UpdateEventForwardSender(new_sender) => {
                        *forward_tx = new_sender;
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
    #[serde(default)]
    submitting_player: SubmittingPlayer,
    #[serde(default)]
    song_select: SongSelect,
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
    ruleset: usize,
    #[serde(default)]
    storage: usize,
    #[serde(default)]
    selected_mods: usize,
    #[serde(default)]
    available_mods: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct SubmittingPlayer {
    beatmap: usize,
    ruleset: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct SongSelect {
    game_ref: usize,
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
    mod_vtable_map: HashMap<usize, String>,
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
                log_warn!("memory-lazer", "Failed to open process: {}", e);
                log_warn!("memory-lazer", "{}", privilege_hint());
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

        let process = Box::leak(Box::new(process));

        Ok(Self {
            offsets,
            process,
            game_base,
            mod_vtable_map: HashMap::new(),
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

        let mut mods: Vec<ModInfo> = match serde_json::from_str(&mods_json) {
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

        let mods_string = order_mods(&mut mods);

        Some(GameplayMods { mods, mods_string })
    }

    fn is_player_screen(&self, screen: usize) -> bool {
        let sp = &self.offsets.submitting_player;
        let gb = &self.offsets.osu_game_base;

        if sp.beatmap == 0 || sp.ruleset == 0 || gb.ruleset == 0 {
            return false;
        }

        let screen_beatmap = self.process.read_ptr(screen + sp.beatmap).unwrap_or(0);
        let game_beatmap = self
            .process
            .read_ptr(self.game_base + gb.beatmap)
            .unwrap_or(0);
        let screen_ruleset = self.process.read_ptr(screen + sp.ruleset).unwrap_or(0);
        let game_ruleset = self
            .process
            .read_ptr(self.game_base + gb.ruleset)
            .unwrap_or(0);

        screen_beatmap != 0
            && screen_beatmap == game_beatmap
            && screen_ruleset != 0
            && screen_ruleset == game_ruleset
    }

    fn is_song_select_screen(&self, screen: usize) -> bool {
        if self.offsets.song_select.game_ref == 0 {
            return false;
        }

        let screen_ref = self
            .process
            .read_ptr(screen + self.offsets.song_select.game_ref)
            .unwrap_or(0);

        screen_ref != 0 && screen_ref == self.game_base
    }

    fn has_song_select_in_stack(&self) -> bool {
        let screen_stack = self
            .process
            .read_ptr(self.game_base + self.offsets.osu_game.screen_stack)
            .unwrap_or(0);
        if screen_stack == 0 {
            return false;
        }

        let stack = self
            .process
            .read_ptr(screen_stack + self.offsets.screen_stack.stack)
            .unwrap_or(0);
        if stack == 0 {
            return false;
        }

        let count = self.process.read_i32(stack + 0x10).unwrap_or(0);
        let items = self.process.read_ptr(stack + 0x8).unwrap_or(0);
        if count <= 0 || items == 0 {
            return false;
        }

        for i in 0..count as usize {
            let screen = self.process.read_ptr(items + 0x10 + 0x8 * i).unwrap_or(0);
            if screen != 0 && self.is_song_select_screen(screen) {
                return true;
            }
        }
        false
    }

    fn read_mods_for_screen(&mut self) -> Option<GameplayMods> {
        let screen = self.get_current_screen()?;

        if self.is_player_screen(screen)
            && let Some(score_info) = self.try_get_score_info_from_player(screen)
            && let Some(mods) = self.read_mods_from_score_info(score_info)
        {
            return Some(mods);
        }

        if self.has_song_select_in_stack() {
            return self.read_selected_mods();
        }

        None
    }

    fn read_selected_mods(&self) -> Option<GameplayMods> {
        if self.offsets.osu_game_base.selected_mods == 0 {
            return None;
        }

        let bindable = self
            .process
            .read_ptr(self.game_base + self.offsets.osu_game_base.selected_mods)
            .ok()?;
        if bindable == 0 {
            return None;
        }

        // bindable.value is the list of selected mods
        let list = self.process.read_ptr(bindable + 0x20).ok()?;
        if list == 0 {
            return None;
        }

        let (items, size) = read_list_or_array(self.process, list)?;

        let mut mods = Vec::new();
        for i in 0..size {
            let mod_ptr = self.process.read_ptr(items + 0x10 + 0x8 * i).ok()?;
            if mod_ptr == 0 {
                continue;
            }
            let vtable = self.process.read_ptr(mod_ptr).ok()?;
            if let Some(acronym) = self.mod_vtable_map.get(&vtable) {
                mods.push(ModInfo {
                    acronym: acronym.clone(),
                    settings: None,
                });
            } else {
                log_debug!(
                    "memory-lazer",
                    "unmatched mod vtable 0x{:X} (map has {} entries)",
                    vtable,
                    self.mod_vtable_map.len()
                );
            }
        }

        let mods_string = order_mods(&mut mods);
        Some(GameplayMods { mods, mods_string })
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

        if self.mod_vtable_map.is_empty() {
            self.mod_vtable_map = build_mod_mapping(self.process, self.game_base, &self.offsets);
        }

        let mods = self.read_mods_for_screen();

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

fn read_list_or_array(process: &ProcessMemory, ptr: usize) -> Option<(usize, usize)> {
    let val_at_10 = process.read_i32(ptr + 0x10).ok()?;

    if !(0..=10_000_000).contains(&val_at_10) {
        let size = process.read_i32(ptr + 0x8).ok()?;
        if !(0..=1000).contains(&size) {
            return None;
        }
        Some((ptr, size as usize))
    } else {
        let items = process.read_ptr(ptr + 0x8).ok()?;
        if items == 0 {
            return None;
        }
        if !(0..=1000).contains(&val_at_10) {
            return None;
        }
        Some((items, val_at_10 as usize))
    }
}

#[rustfmt::skip]
const MOD_ORDERS: &[&[&str]] = &[
    &["EZ", "NF", "HT", "DC"],
    &["HR", "SD", "PF", "DT", "NC", "HD", "FL", "BL", "ST", "AC"],
    &["TP", "DA", "CL", "RD", "MR", "AL", "SG"],
    &["AT", "CN", "RX", "AP", "SO"],
    &["TR", "WG", "SI", "GR", "DF", "WU", "WD", "TC", "BR", "AD",
      "MU", "NS", "MG", "RP", "AS", "FR", "BU", "SY", "DP", "BM"],
    &["TD", "SV2"],
];

fn build_mod_mapping(
    process: &ProcessMemory,
    game_base: usize,
    offsets: &Offsets,
) -> HashMap<usize, String> {
    let mut map = HashMap::new();

    if offsets.osu_game_base.available_mods == 0 {
        return map;
    }

    let _result = (|| -> Option<()> {
        let bindable = process
            .read_ptr(game_base + offsets.osu_game_base.available_mods)
            .ok()?;
        if bindable == 0 {
            return None;
        }

        let dict = process.read_ptr(bindable + 0x20).ok()?;
        if dict == 0 {
            return None;
        }

        let entries = process.read_ptr(dict + 0x10).ok()?;
        if entries == 0 {
            return None;
        }

        let count = process.read_i32(dict + 0x38).ok()?;
        if count <= 0 || count > 20 {
            return None;
        }

        for cat_idx in 0..(count as usize) {
            let acronyms = match MOD_ORDERS.get(cat_idx) {
                Some(a) => a,
                None => continue,
            };

            let entry_base = entries + 0x10 + 0x18 * cat_idx;
            let mod_list = process.read_ptr(entry_base).ok()?;
            if mod_list == 0 {
                continue;
            }

            collect_vtables_from_list(process, mod_list, acronyms, &mut map);
        }

        Some(())
    })();

    if !map.is_empty() {
        log_debug!(
            "memory-lazer",
            "Built mod vtable mapping with {} entries",
            map.len()
        );
    }

    map
}

fn collect_vtables_from_list(
    process: &ProcessMemory,
    list: usize,
    acronyms: &[&str],
    map: &mut HashMap<usize, String>,
) {
    let Some((items, size)) = read_list_or_array(process, list) else {
        log_debug!(
            "memory-lazer",
            "mod mapping: failed to read list/array at 0x{:X}",
            list
        );
        return;
    };

    let mut acronym_idx = 0;

    for i in 0..size {
        let mod_ptr = match process.read_ptr(items + 0x10 + 0x8 * i) {
            Ok(p) if p != 0 => p,
            _ => continue,
        };

        let vtable = match process.read_ptr(mod_ptr) {
            Ok(v) if v != 0 => v,
            _ => continue,
        };

        if is_multi_mod(process, vtable) {
            let sub_list = match process.read_ptr(mod_ptr + 0x10) {
                Ok(p) if p != 0 => p,
                _ => continue,
            };
            let sub_items = match read_array_info(process, sub_list) {
                Some(info) => info,
                None => continue,
            };

            for j in 0..sub_items.1 {
                let sub_mod = match process.read_ptr(sub_items.0 + 0x10 + 0x8 * j) {
                    Ok(p) if p != 0 => p,
                    _ => continue,
                };
                let sub_vtable = match process.read_ptr(sub_mod) {
                    Ok(v) if v != 0 => v,
                    _ => continue,
                };
                if let Some(acronym) = acronyms.get(acronym_idx) {
                    map.insert(sub_vtable, acronym.to_string());
                }
                acronym_idx += 1;
            }
        } else {
            if let Some(acronym) = acronyms.get(acronym_idx) {
                map.insert(vtable, acronym.to_string());
            }
            acronym_idx += 1;
        }
    }
}

fn is_multi_mod(process: &ProcessMemory, vtable: usize) -> bool {
    let a = process.read_i32(vtable).unwrap_or(0);
    let b = process.read_i32(vtable + 3).unwrap_or(0);
    a == 0x0100_0000 && b == 8193
}

fn read_array_info(process: &ProcessMemory, array: usize) -> Option<(usize, usize)> {
    let length = process.read_i32(array + 0x8).ok()?;
    if length <= 0 || length > 1000 {
        return None;
    }
    Some((array, length as usize))
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
