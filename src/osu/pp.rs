use std::path::Path;

use rosu_pp::{Beatmap, Performance};
use thiserror::Error;

use super::core::GameplayMods;

#[derive(Debug, Error)]
pub enum PpError {
    #[error("Failed to parse beatmap: {0}")]
    Parse(String),
    #[error("Failed to read local file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Beatmap file not found: {0}")]
    FileNotFound(String),
}

#[derive(Debug, Clone)]
pub struct PpValues {
    pub pp_95: f64,
    pub pp_97: f64,
    pub pp_98: f64,
    pub pp_99: f64,
    pub pp_100: f64,
}

fn load_beatmap(local_path: Option<&str>, songs_folder: Option<&str>) -> Result<Vec<u8>, PpError> {
    let (Some(rel_path), Some(songs)) = (local_path, songs_folder) else {
        return Err(PpError::FileNotFound(format!(
            "local_path={:?}, songs_folder={:?}",
            local_path, songs_folder
        )));
    };

    let full_path = Path::new(songs).join(rel_path);

    if !full_path.exists() {
        return Err(PpError::FileNotFound(full_path.display().to_string()));
    }

    Ok(std::fs::read(&full_path)?)
}

fn mods_to_bitflag(mods: &Option<GameplayMods>) -> u32 {
    let Some(gameplay_mods) = mods else {
        return 0;
    };

    let mut bits = 0u32;

    for mod_info in &gameplay_mods.mods {
        bits |= match mod_info.acronym.as_str() {
            "NF" => 1 << 0,           // NoFail
            "EZ" => 1 << 1,           // Easy
            "TD" => 1 << 2,           // TouchDevice
            "HD" => 1 << 3,           // Hidden
            "HR" => 1 << 4,           // HardRock
            "SD" => 1 << 5,           // SuddenDeath
            "DT" => 1 << 6,           // DoubleTime
            "RX" => 1 << 7,           // Relax
            "HT" => 1 << 8,           // HalfTime
            "NC" => 1 << 6 | 1 << 9,  // Nightcore (/DT)
            "FL" => 1 << 10,          // Flashlight
            "SO" => 1 << 12,          // SpunOut
            "AP" => 1 << 13,          // Autopilot
            "PF" => 1 << 5 | 1 << 14, // Perfect (/SD)
            _ => 0,
        };
    }

    bits
}

pub fn get_pp_spread(
    mods: &Option<GameplayMods>,
    local_path: Option<&str>,
    songs_folder: Option<&str>,
) -> Result<PpValues, PpError> {
    let osu_file = load_beatmap(local_path, songs_folder)?;
    let beatmap = Beatmap::from_bytes(&osu_file).map_err(|e| PpError::Parse(e.to_string()))?;

    let mod_bits = mods_to_bitflag(mods);

    let pp_95 = Performance::new(&beatmap)
        .mods(mod_bits)
        .accuracy(95.0)
        .calculate()
        .pp();

    let pp_97 = Performance::new(&beatmap)
        .mods(mod_bits)
        .accuracy(97.0)
        .calculate()
        .pp();

    let pp_98 = Performance::new(&beatmap)
        .mods(mod_bits)
        .accuracy(98.0)
        .calculate()
        .pp();

    let pp_99 = Performance::new(&beatmap)
        .mods(mod_bits)
        .accuracy(99.0)
        .calculate()
        .pp();

    let pp_100 = Performance::new(&beatmap)
        .mods(mod_bits)
        .accuracy(100.0)
        .calculate()
        .pp();

    Ok(PpValues {
        pp_95,
        pp_97,
        pp_98,
        pp_99,
        pp_100,
    })
}
