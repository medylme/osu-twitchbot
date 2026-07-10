use std::path::Path;

use rosu_mods::serde::GameModSeed;
use rosu_mods::{Acronym, GameMod, GameModIntermode, GameMode, GameMods};
use rosu_pp::{Beatmap, Difficulty, Performance};
use serde::de::DeserializeSeed;
use thiserror::Error;

use super::core::GameplayMods;
use crate::log_warn;

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

fn build_mods(mods: &Option<GameplayMods>, mode: GameMode) -> GameMods {
    let mut game_mods = GameMods::default();

    let Some(gameplay_mods) = mods else {
        return game_mods;
    };

    for mod_info in &gameplay_mods.mods {
        let Ok(acronym) = mod_info.acronym.parse::<Acronym>() else {
            log_warn!(
                "osu",
                "Ignoring invalid mod acronym in pp calculation: {}",
                mod_info.acronym
            );
            continue;
        };

        if matches!(
            GameModIntermode::from_acronym(acronym),
            GameModIntermode::Unknown(_)
        ) {
            log_warn!(
                "osu",
                "Ignoring unknown mod in pp calculation: {}",
                mod_info.acronym
            );
            continue;
        }

        let gamemod = mod_info
            .settings
            .as_ref()
            .and_then(|settings| {
                let value = serde_json::json!({
                    "acronym": mod_info.acronym,
                    "settings": settings,
                });
                GameModSeed::Mode {
                    mode,
                    deny_unknown_fields: false,
                }
                .deserialize(value)
                    .inspect_err(|e| {
                        log_warn!(
                            "osu",
                            "Ignoring settings of mod {} in pp calculation: {}",
                            mod_info.acronym,
                            e
                        );
                    })
                    .ok()
            })
            .unwrap_or_else(|| GameMod::new(&mod_info.acronym, mode));

        game_mods.insert(gamemod);
    }

    game_mods
}

fn game_mode(beatmap: &Beatmap) -> GameMode {
    match beatmap.mode as u8 {
        1 => GameMode::Taiko,
        2 => GameMode::Catch,
        3 => GameMode::Mania,
        _ => GameMode::Osu,
    }
}

pub fn get_pp_spread(
    mods: &Option<GameplayMods>,
    local_path: Option<&str>,
    songs_folder: Option<&str>,
) -> Result<PpValues, PpError> {
    let osu_file = load_beatmap(local_path, songs_folder)?;
    let beatmap = Beatmap::from_bytes(&osu_file).map_err(|e| PpError::Parse(e.to_string()))?;

    let game_mods = build_mods(mods, game_mode(&beatmap));
    let difficulty = Difficulty::new().mods(game_mods);

    let pp_at = |accuracy: f64| {
        Performance::new(&beatmap)
            .difficulty(difficulty.clone())
            .accuracy(accuracy)
            .calculate()
            .pp()
    };

    Ok(PpValues {
        pp_95: pp_at(95.0),
        pp_97: pp_at(97.0),
        pp_98: pp_at(98.0),
        pp_99: pp_at(99.0),
        pp_100: pp_at(100.0),
    })
}

#[cfg(test)]
mod tests {
    use super::super::core::ModInfo;
    use super::*;

    const TEST_MAP: &str = "osu file format v14

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
64,192,1000,1,0,0:0:0:0
192,192,1250,1,0,0:0:0:0
320,192,1500,1,0,0:0:0:0
448,192,1750,1,0,0:0:0:0
";

    fn pp_with_mods(mods: Vec<ModInfo>) -> f64 {
        let beatmap = Beatmap::from_bytes(TEST_MAP.as_bytes()).unwrap();
        let mods = Some(GameplayMods {
            mods,
            mods_string: String::new(),
        });

        Performance::new(&beatmap)
            .difficulty(Difficulty::new().mods(build_mods(&mods, game_mode(&beatmap))))
            .accuracy(100.0)
            .calculate()
            .pp()
    }

    fn pp_with(acronyms: &[&str]) -> f64 {
        pp_with_mods(
            acronyms
                .iter()
                .map(|a| ModInfo {
                    acronym: (*a).to_string(),
                    settings: None,
                })
                .collect(),
        )
    }

    #[test]
    fn double_time_raises_pp() {
        assert!(pp_with(&["DT"]) > pp_with(&[]));
    }

    #[test]
    fn daycore_lowers_pp() {
        assert!(pp_with(&["DC"]) < pp_with(&[]));
    }

    #[test]
    fn unknown_mods_are_ignored() {
        assert_eq!(pp_with(&["ZZ"]), pp_with(&[]));
    }

    #[test]
    fn custom_speed_multiplier_is_honored() {
        let dt_1_25 = pp_with_mods(vec![ModInfo {
            acronym: "DT".to_string(),
            settings: Some(serde_json::json!({ "speed_change": 1.25 })),
        }]);

        assert!(dt_1_25 > pp_with(&[]));
        assert!(dt_1_25 < pp_with(&["DT"]));
    }

    #[test]
    fn invalid_settings_fall_back_to_defaults() {
        let dt_bad = pp_with_mods(vec![ModInfo {
            acronym: "DT".to_string(),
            settings: Some(serde_json::json!({ "speed_change": "fast" })),
        }]);

        assert_eq!(dt_bad, pp_with(&["DT"]));
    }
}
