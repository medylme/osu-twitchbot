use std::path::Path;

use rosu_mods::serde::GameModSeed;
use rosu_mods::{Acronym, GameMod, GameModIntermode, GameMode, GameMods};
use rosu_pp::model::hit_object::HitObjectKind;
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

#[derive(Debug, Clone)]
pub struct MapStats {
    pub stars: f64,
    pub bpm: f64,
    pub length_secs: f64,
    pub ar: f64,
    pub od: f64,
    pub cs: f64,
    pub hp: f64,
    pub max_combo: u32,
    pub objects: usize,
}

impl MapStats {
    pub fn stars_text(&self) -> String {
        format!("{:.2}", self.stars)
    }

    pub fn bpm_text(&self) -> String {
        format!("{:.0}", self.bpm)
    }

    pub fn length_text(&self) -> String {
        let secs = self.length_secs.round() as u64;
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    // one decimal, without a trailing .0
    pub fn setting_text(value: f64) -> String {
        let text = format!("{value:.1}");
        text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
    }
}

#[derive(Debug, Clone)]
pub struct MapAnalysis {
    pub stats: MapStats,
    pub pp: PpValues,
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

// slider end times are not stored, so derive the last one from its length and velocity
fn last_object_end(beatmap: &Beatmap) -> Option<f64> {
    let last = beatmap.hit_objects.last()?;
    let start = last.start_time;

    let duration = match &last.kind {
        HitObjectKind::Circle => 0.0,
        HitObjectKind::Spinner(spinner) => spinner.duration,
        HitObjectKind::Hold(hold) => hold.duration,
        HitObjectKind::Slider(slider) => {
            let active = |time: f64| time <= start;
            let beat_len = beatmap
                .timing_points
                .iter()
                .rfind(|point| active(point.time))
                .or(beatmap.timing_points.first())
                .map(|point| point.beat_len);
            let slider_velocity = beatmap
                .difficulty_points
                .iter()
                .rfind(|point| active(point.time))
                .map_or(1.0, |point| point.slider_velocity);
            let velocity = beat_len
                .map(|beat_len| 100.0 * beatmap.slider_multiplier * slider_velocity / beat_len);

            match (slider.expected_dist, velocity) {
                (Some(dist), Some(velocity)) if velocity > 0.0 => {
                    (slider.repeats + 1) as f64 * dist / velocity
                }
                _ => 0.0,
            }
        }
    };

    Some(start + duration)
}

fn analyze(beatmap: &Beatmap, mods: &Option<GameplayMods>) -> MapAnalysis {
    let difficulty = Difficulty::new().mods(build_mods(mods, game_mode(beatmap)));
    let attributes = difficulty.calculate(beatmap);

    let map_attributes = beatmap.attributes().difficulty(&difficulty).build();
    let clock_rate = map_attributes.clock_rate();
    let adjusted = map_attributes.apply_clock_rate();

    let length_ms = match (beatmap.hit_objects.first(), last_object_end(beatmap)) {
        (Some(first), Some(end)) => (end - first.start_time).max(0.0),
        _ => 0.0,
    };

    let stats = MapStats {
        stars: attributes.stars(),
        bpm: beatmap.bpm() * clock_rate,
        length_secs: length_ms / clock_rate / 1000.0,
        ar: adjusted.ar,
        od: adjusted.od,
        cs: f64::from(adjusted.cs),
        hp: f64::from(adjusted.hp),
        max_combo: attributes.max_combo(),
        objects: beatmap.hit_objects.len(),
    };

    let pp_at = |accuracy: f64| {
        Performance::new(attributes.clone())
            .difficulty(difficulty.clone())
            .accuracy(accuracy)
            .calculate()
            .pp()
    };

    MapAnalysis {
        stats,
        pp: PpValues {
            pp_95: pp_at(95.0),
            pp_97: pp_at(97.0),
            pp_98: pp_at(98.0),
            pp_99: pp_at(99.0),
            pp_100: pp_at(100.0),
        },
    }
}

pub fn analyze_map(
    mods: &Option<GameplayMods>,
    local_path: Option<&str>,
    songs_folder: Option<&str>,
) -> Result<MapAnalysis, PpError> {
    let osu_file = load_beatmap(local_path, songs_folder)?;
    let beatmap = Beatmap::from_bytes(&osu_file).map_err(|e| PpError::Parse(e.to_string()))?;

    Ok(analyze(&beatmap, mods))
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

    fn analyze_with(map: &str, acronyms: &[&str]) -> MapAnalysis {
        let beatmap = Beatmap::from_bytes(map.as_bytes()).unwrap();
        let mods = Some(GameplayMods {
            mods: acronyms
                .iter()
                .map(|a| ModInfo {
                    acronym: (*a).to_string(),
                    settings: None,
                })
                .collect(),
            mods_string: String::new(),
        });

        analyze(&beatmap, &mods)
    }

    #[test]
    fn stats_describe_the_map() {
        let stats = analyze_with(TEST_MAP, &[]).stats;

        assert!(stats.stars > 0.0);
        assert_eq!(stats.bpm_text(), "120");
        assert_eq!(stats.length_text(), "0:02");
        assert_eq!(stats.objects, 8);
        assert_eq!(stats.max_combo, 8);
        assert_eq!(MapStats::setting_text(stats.cs), "4");
        assert_eq!(MapStats::setting_text(stats.ar), "9");
        assert_eq!(MapStats::setting_text(stats.od), "8");
        assert_eq!(MapStats::setting_text(stats.hp), "5");
    }

    #[test]
    fn double_time_adjusts_stats() {
        let nomod = analyze_with(TEST_MAP, &[]).stats;
        let dt = analyze_with(TEST_MAP, &["DT"]).stats;

        assert!(dt.stars > nomod.stars);
        assert_eq!(dt.bpm_text(), "180");
        assert!((dt.length_secs - nomod.length_secs / 1.5).abs() < 1e-9);
        assert_eq!(MapStats::setting_text(dt.ar), "10.3");
        assert_eq!(MapStats::setting_text(dt.od), "9.8");
        assert_eq!(dt.cs, nomod.cs);
    }

    #[test]
    fn hard_rock_adjusts_stats() {
        let hr = analyze_with(TEST_MAP, &["HR"]).stats;

        assert_eq!(MapStats::setting_text(hr.cs), "5.2");
        assert_eq!(MapStats::setting_text(hr.ar), "10");
        assert_eq!(MapStats::setting_text(hr.hp), "7");
    }

    #[test]
    fn length_includes_the_last_slider() {
        // 280px slider at 140px per beat lasts two beats
        let map = TEST_MAP.replace(
            "448,192,1750,1,0,0:0:0:0",
            "100,192,1750,2,0,L|380:192,1,280",
        );
        let stats = analyze_with(&map, &[]).stats;

        assert!((stats.length_secs - 2.75).abs() < 1e-9);
    }

    #[test]
    fn analysis_pp_matches_a_direct_calculation() {
        let analysis = analyze_with(TEST_MAP, &["HD", "DT"]);

        assert_eq!(analysis.pp.pp_100, pp_with(&["HD", "DT"]));
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
