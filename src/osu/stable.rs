use iced::futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::time::{self, Duration};

use super::core::{
    BeatmapData, BeatmapStatus, DATA_POLLING_INTERVAL_MS, GameplayMods, MemoryError, MemoryEvent,
    ModInfo, OsuCommand, OsuStatus, ProcessMemory, order_mods, parse_pattern,
};
use crate::{log_debug, log_error};

pub async fn run_stable_reader(
    pid: u32,
    songs_folder: Option<String>,
    tx: &mut iced::futures::channel::mpsc::Sender<MemoryEvent>,
    cmd_rx: &mut iced::futures::channel::mpsc::Receiver<OsuCommand>,
    forward_tx: &mut iced::futures::channel::mpsc::Sender<MemoryEvent>,
    current_beatmap: &mut Option<BeatmapData>,
) -> Result<(), MemoryError> {
    log_debug!(
        "memory-stable",
        "Starting stable reader with songs_folder: {:?}",
        songs_folder
    );

    let _ = tx
        .send(MemoryEvent::StatusChanged(OsuStatus::Initializing))
        .await;

    let offsets_json = include_str!("../../offsets/stable.json").to_string();

    let reader = tokio::task::spawn_blocking(move || {
        StableReader::new(pid, &offsets_json).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| MemoryError::ReadFailed(format!("Task panic: {}", e)))?
    .map_err(MemoryError::ReadFailed)?;

    let _ = tx
        .send(MemoryEvent::StatusChanged(OsuStatus::Connected(format!(
            "Stable (pid {})",
            pid
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
                    Ok(Ok(mut beatmap)) => {
                        beatmap.songs_folder = songs_folder.clone();

                        let mods_changed =
                            current_beatmap.as_ref().map(|b| &b.mods) != Some(&beatmap.mods);
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
                    OsuCommand::UpdateEventForwardSender(new_sender) => {
                        *forward_tx = new_sender;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct Offsets {
    patterns: Patterns,
    base: BaseOffsets,
    beatmap: BeatmapOffsets,
    ruleset: RulesetOffsets,
    status: StatusOffsets,
}

#[derive(Debug, Deserialize, Clone)]
struct Patterns {
    base: String,
    ruleset: String,
}

#[derive(Debug, Deserialize, Clone)]
struct StatusOffsets {
    base_offset: isize,
}

#[derive(Debug, Deserialize, Clone)]
struct BaseOffsets {
    beatmap_ptr: isize,
}

#[derive(Debug, Deserialize, Clone)]
struct BeatmapOffsets {
    artist: usize,
    title: usize,
    creator: usize,
    difficulty: usize,
    map_id: usize,
    ranked_status: usize,
    folder: usize,
    file: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct RulesetOffsets {
    ptr_offset: usize,
    ptr_deref_offset: usize,
    play_container: usize,
    mods_base: usize,
    mods_ptr: usize,
    mods_xor1: usize,
    mods_xor2: usize,
}

#[derive(Clone)]
pub struct StableReader<'a> {
    offsets: Offsets,
    process: &'a ProcessMemory,
    base_addr: usize,
    ruleset_addr: usize,
}

impl<'a> StableReader<'a> {
    pub fn new(
        pid: u32,
        offsets_json: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let offsets: Offsets = serde_json::from_str(offsets_json).map_err(|e| {
            log_error!("memory-stable", "Failed to parse offsets JSON: {}", e);
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;

        let process = match ProcessMemory::new(pid) {
            Ok(p) => p,
            Err(e) => {
                log_error!("memory-stable", "Failed to open process: {}", e);
                log_error!("memory-stable", "Try running with admin/root privileges.");
                return Err(Box::new(e));
            }
        };

        log_debug!("memory-stable", "Scanning for base address pattern...");

        let (base_pattern, base_mask) = parse_pattern(&offsets.patterns.base);
        let base_addr = match process.pattern_scan(&base_pattern, &base_mask) {
            Ok(addr) => {
                log_debug!("memory-stable", "Found base pattern at: 0x{:X}", addr);
                addr
            }
            Err(e) => {
                log_error!("memory-stable", "Failed to find base pattern: {}", e);
                return Err(Box::new(e));
            }
        };

        log_debug!("memory-stable", "Scanning for ruleset pattern...");

        let (ruleset_pattern, ruleset_mask) = parse_pattern(&offsets.patterns.ruleset);
        let ruleset_addr = match process.pattern_scan(&ruleset_pattern, &ruleset_mask) {
            Ok(addr) => {
                log_debug!("memory-stable", "Found ruleset pattern at: 0x{:X}", addr);
                addr
            }
            Err(e) => {
                log_error!("memory-stable", "Failed to find ruleset pattern: {}", e);
                return Err(Box::new(e));
            }
        };

        log_debug!(
            "memory-stable",
            "Found base at: 0x{:X}, ruleset at: 0x{:X}",
            base_addr,
            ruleset_addr
        );

        Ok(Self {
            offsets,
            process: Box::leak(Box::new(process)),
            base_addr,
            ruleset_addr,
        })
    }

    fn read_status(&self) -> Option<u32> {
        let status_ptr_addr = (self.base_addr as isize + self.offsets.status.base_offset) as usize;
        let status_ptr = self.process.read_ptr32(status_ptr_addr).ok()?;
        if status_ptr == 0 {
            return None;
        }
        self.process.read_i32(status_ptr).ok().map(|v| v as u32)
    }

    fn read_mods(&self) -> Option<GameplayMods> {
        let status = self.read_status()?;

        // only read gameplay mods when playing
        if status != 2 {
            return None;
        }

        // we first find the ruleset pointer
        let ruleset_ptr_addr = self.ruleset_addr + self.offsets.ruleset.ptr_offset;
        let ruleset_ptr = self.process.read_ptr32(ruleset_ptr_addr).ok()?;
        if ruleset_ptr == 0 {
            return None;
        }
        let ruleset_base = self
            .process
            .read_ptr32(ruleset_ptr + self.offsets.ruleset.ptr_deref_offset)
            .ok()?;
        if ruleset_base == 0 {
            return None;
        }

        // then traverse to the PlayContainer
        let play_container = self
            .process
            .read_ptr32(ruleset_base + self.offsets.ruleset.play_container)
            .ok()?;
        if play_container == 0 {
            return None;
        }

        // then to the score
        let score = self
            .process
            .read_ptr32(play_container + self.offsets.ruleset.mods_base)
            .ok()?;
        if score == 0 {
            return None;
        }

        // and then we can find the mods
        let mods_xor_base = self
            .process
            .read_ptr32(score + self.offsets.ruleset.mods_ptr)
            .ok()?;
        if mods_xor_base == 0 {
            return None;
        }

        // we read two values and XOR them to decode
        let xor1 = self
            .process
            .read_i32(mods_xor_base + self.offsets.ruleset.mods_xor1)
            .ok()?;
        let xor2 = self
            .process
            .read_i32(mods_xor_base + self.offsets.ruleset.mods_xor2)
            .ok()?;

        let mods_value = xor1 ^ xor2;

        if mods_value == 0 {
            return Some(GameplayMods {
                mods: vec![],
                mods_string: "NoMod".to_string(),
            });
        }

        let mods = parse_stable_mods(mods_value as u32);
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

        if self.base_addr == 0 {
            return Err(MemoryError::ReadFailed("Base address not set.".to_string()));
        }

        let beatmap_ptr_addr = (self.base_addr as isize + self.offsets.base.beatmap_ptr) as usize;
        let beatmap_ptr = match self.process.read_ptr32(beatmap_ptr_addr) {
            Ok(ptr) => {
                if ptr == 0 {
                    return Ok(unknown_data);
                }
                ptr
            }
            Err(e) => {
                return Err(MemoryError::ReadFailed(format!(
                    "Failed to read beatmap pointer: {}",
                    e
                )));
            }
        };

        let beatmap = match self.process.read_ptr32(beatmap_ptr) {
            Ok(ptr) => {
                if ptr == 0 {
                    return Ok(unknown_data);
                }
                ptr
            }
            Err(e) => {
                return Err(MemoryError::ReadFailed(format!(
                    "Failed to read beatmap: {}",
                    e
                )));
            }
        };

        let id = self
            .process
            .read_i32(beatmap + self.offsets.beatmap.map_id)
            .unwrap_or(0);

        let status_int = self
            .process
            .read_i32(beatmap + self.offsets.beatmap.ranked_status)
            .unwrap_or(-3);

        let status = match status_int {
            0 => BeatmapStatus::Unknown,
            1 => BeatmapStatus::NotSubmitted,
            2 => BeatmapStatus::StablePending,
            3 => BeatmapStatus::Unknown,
            4 => BeatmapStatus::Ranked,
            5 => BeatmapStatus::Approved,
            6 => BeatmapStatus::Qualified,
            7 => BeatmapStatus::Loved,
            _ => BeatmapStatus::Unknown,
        };

        let artist = read_stable_string(self.process, beatmap + self.offsets.beatmap.artist)
            .unwrap_or_else(|_| "?".to_string());

        let title = read_stable_string(self.process, beatmap + self.offsets.beatmap.title)
            .unwrap_or_else(|_| "?".to_string());

        let difficulty_name =
            read_stable_string(self.process, beatmap + self.offsets.beatmap.difficulty)
                .unwrap_or_else(|_| "?".to_string());

        let creator = read_stable_string(self.process, beatmap + self.offsets.beatmap.creator)
            .unwrap_or_else(|_| "?".to_string());

        let folder = read_stable_string(self.process, beatmap + self.offsets.beatmap.folder).ok();
        let file = read_stable_string(self.process, beatmap + self.offsets.beatmap.file).ok();

        let osu_file_path = match (folder, file) {
            (Some(f), Some(n)) if !f.is_empty() && !n.is_empty() => {
                use std::path::Path;
                let path = Path::new(&f).join(&n);
                path.to_str().map(|s| s.to_string())
            }
            _ => None,
        };

        let mods = self.read_mods();

        Ok(BeatmapData {
            id,
            artist,
            title,
            difficulty_name,
            creator,
            status,
            mods,
            osu_file_path,
            songs_folder: None,
        })
    }
}

fn read_stable_string(process: &ProcessMemory, addr: usize) -> Result<String, MemoryError> {
    let str_ptr = process.read_ptr32(addr)?;
    if str_ptr == 0 {
        return Ok(String::new());
    }

    let length = process.read_i32(str_ptr + 0x4)? as usize;

    if length == 0 || length > 10000 {
        return Ok(String::new());
    }

    let mut buffer = vec![0u16; length];
    for (i, item) in buffer.iter_mut().enumerate().take(length) {
        *item = process.read_u16(str_ptr + 0x8 + (i * 2))?;
    }

    String::from_utf16(&buffer).map_err(|_| MemoryError::InvalidString)
}

fn parse_stable_mods(mods: u32) -> Vec<ModInfo> {
    const NONE: u32 = 0;
    const NO_FAIL: u32 = 1;
    const EASY: u32 = 2;
    const TOUCH_DEVICE: u32 = 4;
    const HIDDEN: u32 = 8;
    const HARD_ROCK: u32 = 16;
    const SUDDEN_DEATH: u32 = 32;
    const DOUBLE_TIME: u32 = 64;
    const RELAX: u32 = 128;
    const HALF_TIME: u32 = 256;
    const NIGHTCORE: u32 = 512;
    const FLASHLIGHT: u32 = 1024;
    const SPUN_OUT: u32 = 4096;
    const AUTOPILOT: u32 = 8192;
    const PERFECT: u32 = 16384;

    let mut result = Vec::new();

    let mod_checks: &[(u32, &str)] = &[
        (EASY, "EZ"),
        (NO_FAIL, "NF"),
        (HALF_TIME, "HT"),
        (HARD_ROCK, "HR"),
        (SUDDEN_DEATH, "SD"),
        (PERFECT, "PF"),
        (DOUBLE_TIME, "DT"),
        (NIGHTCORE, "NC"),
        (HIDDEN, "HD"),
        (FLASHLIGHT, "FL"),
        (RELAX, "RX"),
        (AUTOPILOT, "AP"),
        (SPUN_OUT, "SO"),
        (TOUCH_DEVICE, "TD"),
    ];

    if mods == NONE {
        return result;
    }

    for &(flag, acronym) in mod_checks {
        if mods & flag != 0 {
            if flag == NIGHTCORE && mods & DOUBLE_TIME != 0 {
                continue;
            }
            if flag == PERFECT && mods & SUDDEN_DEATH != 0 {
                continue;
            }

            result.push(ModInfo {
                acronym: acronym.to_string(),
                settings: None,
            });
        }
    }

    result
}
