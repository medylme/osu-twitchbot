use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rosu_mods::serde::GameModSeed;
use rosu_mods::{Acronym, GameMod, GameModIntermode, GameMode, GameMods};
use rosu_pp::any::DifficultyAttributes;
use rosu_pp::model::hit_object::HitObjectKind;
use rosu_pp::model::mode::GameMode as MapMode;
use rosu_pp::{Beatmap, Difficulty, Performance};
use serde::de::DeserializeSeed;
use thiserror::Error;

use super::core::{BeatmapData, GameplayMods, ModInfo, OsuClient, order_mods};
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

    pub fn setting_text(value: f64) -> String {
        decimal_text(value, 1)
    }
}

// fixed decimals without trailing zeros
pub fn decimal_text(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if !text.contains('.') {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PpArgs {
    pub accuracy: Option<f64>,
    pub combo: Option<u32>,
    pub misses: Option<u32>,
    pub mods: Option<GameplayMods>,
}

impl PpArgs {
    pub fn parse(text: &str) -> Self {
        let mut args = Self::default();

        for token in text.split_whitespace() {
            let token = token.to_ascii_lowercase();

            if let Some(mods) = token.strip_prefix('+') {
                args.mods = parse_mods(mods).or(args.mods);
            } else if let Some(misses) = ["misses", "miss", "m"]
                .iter()
                .find_map(|suffix| token.strip_suffix(suffix))
            {
                args.misses = misses.parse().ok().or(args.misses);
            } else if let Some(combo) = token.strip_suffix('x') {
                args.combo = combo.parse().ok().or(args.combo);
            } else {
                let accuracy = token.strip_suffix('%').unwrap_or(&token).replace(',', ".");
                args.accuracy = accuracy
                    .parse()
                    .ok()
                    .filter(|accuracy| (0.0..=100.0).contains(accuracy))
                    .or(args.accuracy);
            }
        }

        args
    }

    pub fn is_score(&self) -> bool {
        self.accuracy.is_some() || self.combo.is_some() || self.misses.is_some()
    }
}

fn parse_mods(text: &str) -> Option<GameplayMods> {
    let text = text.to_ascii_uppercase();
    let mut mods: Vec<ModInfo> = Vec::new();

    if !matches!(text.as_str(), "NM" | "NOMOD") {
        if text.is_empty() || !text.is_ascii() || !text.len().is_multiple_of(2) {
            return None;
        }

        for acronym in text.as_bytes().chunks(2) {
            let acronym = std::str::from_utf8(acronym).ok()?;
            let known = acronym.parse::<Acronym>().is_ok_and(|acronym| {
                !matches!(
                    GameModIntermode::from_acronym(acronym),
                    GameModIntermode::Unknown(_)
                )
            });
            if !known {
                return None;
            }

            if !mods.iter().any(|m| m.acronym == acronym) {
                mods.push(ModInfo {
                    acronym: acronym.to_string(),
                    settings: None,
                });
            }
        }
    }

    let mods_string = order_mods(&mut mods);
    Some(GameplayMods { mods, mods_string })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScorePp {
    pub pp: f64,
    pub accuracy: f64,
    pub combo: u32,
    pub misses: u32,
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

// slider end times are not stored, so derive the last one
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

// stable scores have no slider acc and estimate misses from the combo
fn difficulty_for(beatmap: &Beatmap, mods: &Option<GameplayMods>, client: OsuClient) -> Difficulty {
    Difficulty::new()
        .mods(build_mods(mods, game_mode(beatmap)))
        .lazer(client == OsuClient::Lazer)
}

struct RatedMap {
    difficulty: Difficulty,
    attributes: DifficultyAttributes,
    mode: GameMode,
    analysis: MapAnalysis,
}

#[derive(PartialEq)]
struct RatedMapKey {
    osu_file_path: Option<String>,
    songs_folder: Option<String>,
    file_stamp: Option<(SystemTime, u64)>,
    mods: Option<GameplayMods>,
    ruleset: Option<u8>,
    client: OsuClient,
}

impl RatedMapKey {
    fn new(data: &BeatmapData) -> Self {
        Self {
            osu_file_path: data.osu_file_path.clone(),
            songs_folder: data.songs_folder.clone(),
            file_stamp: data.file_stamp,
            mods: data.mods.clone(),
            ruleset: data.ruleset,
            client: data.client,
        }
    }
}

const RATED_MAP_CACHE_SIZE: usize = 4;

static RATED_MAPS: Mutex<VecDeque<(RatedMapKey, Arc<RatedMap>)>> = Mutex::new(VecDeque::new());

fn rated_map(data: &BeatmapData) -> Result<Arc<RatedMap>, PpError> {
    let key = RatedMapKey::new(data);

    if let Ok(cache) = RATED_MAPS.lock()
        && let Some((_, rated)) = cache.iter().find(|(cached, _)| *cached == key)
    {
        return Ok(Arc::clone(rated));
    }

    let beatmap = parse_beatmap(data)?;
    let rated = Arc::new(rate(&beatmap, &data.mods, data.client));

    if (data.client == OsuClient::Lazer || data.file_stamp.is_some())
        && let Ok(mut cache) = RATED_MAPS.lock()
    {
        cache.push_front((key, Arc::clone(&rated)));
        cache.truncate(RATED_MAP_CACHE_SIZE);
    }

    Ok(rated)
}

fn rate(beatmap: &Beatmap, mods: &Option<GameplayMods>, client: OsuClient) -> RatedMap {
    let difficulty = difficulty_for(beatmap, mods, client);
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

    let analysis = MapAnalysis {
        stats,
        pp: PpValues {
            pp_95: pp_at(95.0),
            pp_97: pp_at(97.0),
            pp_98: pp_at(98.0),
            pp_99: pp_at(99.0),
            pp_100: pp_at(100.0),
        },
    };

    RatedMap {
        mode: game_mode(beatmap),
        difficulty,
        attributes,
        analysis,
    }
}

fn score_pp(rated: &RatedMap, args: &PpArgs) -> ScorePp {
    let max_combo = rated.attributes.max_combo();
    let combo = args.combo.unwrap_or(max_combo).min(max_combo);

    let mut performance = Performance::new(rated.attributes.clone())
        .difficulty(rated.difficulty.clone())
        .combo(combo)
        .misses(args.misses.unwrap_or(0));
    if let Some(accuracy) = args.accuracy {
        performance = performance.accuracy(accuracy);
    }

    let mode = match rated.attributes {
        DifficultyAttributes::Osu(_) => MapMode::Osu,
        DifficultyAttributes::Taiko(_) => MapMode::Taiko,
        DifficultyAttributes::Catch(_) => MapMode::Catch,
        DifficultyAttributes::Mania(_) => MapMode::Mania,
    };
    let state = performance.generate_state();
    let hits = state.total_hits(mode);

    let reachable = if hits == 0 {
        100.0
    } else {
        100.0 * f64::from(hits.saturating_sub(state.misses)) / f64::from(hits)
    };

    ScorePp {
        pp: performance.calculate().pp(),
        accuracy: args
            .accuracy
            .map_or(reachable, |accuracy| accuracy.min(reachable)),
        combo,
        misses: state.misses,
    }
}

// std can be converted to other rulesets
fn convert_to_ruleset(beatmap: &mut Beatmap, ruleset: Option<u8>, mods: &Option<GameplayMods>) {
    let (map_mode, mode) = match ruleset {
        Some(1) => (MapMode::Taiko, GameMode::Taiko),
        Some(2) => (MapMode::Catch, GameMode::Catch),
        Some(3) => (MapMode::Mania, GameMode::Mania),
        _ => return,
    };

    if beatmap.mode != MapMode::Osu {
        return;
    }

    let mods = build_mods(mods, mode);
    if let Err(e) = beatmap.convert_mut(map_mode, &mods.into()) {
        log_warn!(
            "osu",
            "Failed to convert beatmap to ruleset {:?}: {}",
            ruleset,
            e
        );
    }
}

fn parse_beatmap(data: &BeatmapData) -> Result<Beatmap, PpError> {
    let osu_file = load_beatmap(data.osu_file_path.as_deref(), data.songs_folder.as_deref())?;
    let mut beatmap = Beatmap::from_bytes(&osu_file).map_err(|e| PpError::Parse(e.to_string()))?;
    convert_to_ruleset(&mut beatmap, data.ruleset, &data.mods);

    Ok(beatmap)
}

fn mods_fit(mods: &GameplayMods, mode: GameMode) -> bool {
    let game_mods: GameMods = mods
        .mods
        .iter()
        .map(|mod_info| GameMod::new(&mod_info.acronym, mode))
        .collect();

    game_mods.is_valid()
        && !game_mods
            .iter()
            .any(|gamemod| matches!(gamemod.intermode(), GameModIntermode::Unknown(_)))
}

// false when the mods exclude each other or the map's mode has no such mod
pub fn mods_fit_map(data: &BeatmapData, mods: &GameplayMods) -> bool {
    rated_map(data).map_or(true, |rated| mods_fit(mods, rated.mode))
}

pub fn calculate_score_pp(data: &BeatmapData, args: &PpArgs) -> Result<ScorePp, PpError> {
    Ok(score_pp(&*rated_map(data)?, args))
}

pub fn analyze_map(data: &BeatmapData) -> Result<MapAnalysis, PpError> {
    Ok(rated_map(data)?.analysis.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osu::core::{BeatmapStatus, GameState};

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

        rate(&beatmap, &mods, OsuClient::Lazer).analysis
    }

    fn map_on_disk(name: &str) -> BeatmapData {
        let dir =
            std::env::temp_dir().join(format!("osu-twitchbot-pp-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("map.osu"), TEST_MAP).unwrap();

        let mut data = BeatmapData {
            client: OsuClient::Lazer,
            state: GameState::SongSelect,
            ruleset: None,
            id: 0,
            artist: String::new(),
            title: String::new(),
            difficulty_name: String::new(),
            creator: String::new(),
            status: BeatmapStatus::NotSubmitted,
            mods: None,
            osu_file_path: Some("map.osu".to_string()),
            songs_folder: dir.to_str().map(str::to_string),
            file_stamp: None,
        };
        data.stamp_file();
        data
    }

    #[test]
    fn a_map_is_rated_once_until_something_changes() {
        let data = map_on_disk("cache");
        let rated = rated_map(&data).unwrap();

        let playing = BeatmapData {
            state: GameState::Playing,
            ..data.clone()
        };
        assert!(Arc::ptr_eq(&rated, &rated_map(&playing).unwrap()));

        let taiko = BeatmapData {
            ruleset: Some(1),
            ..data.clone()
        };
        assert!(!Arc::ptr_eq(&rated, &rated_map(&taiko).unwrap()));

        let saved = BeatmapData {
            file_stamp: Some((SystemTime::UNIX_EPOCH, 0)),
            ..data.clone()
        };
        assert!(!Arc::ptr_eq(&rated, &rated_map(&saved).unwrap()));
    }

    #[test]
    fn a_stable_map_without_a_stamp_is_not_cached() {
        let data = BeatmapData {
            client: OsuClient::Stable,
            file_stamp: None,
            ..map_on_disk("unstamped")
        };

        assert!(!Arc::ptr_eq(
            &rated_map(&data).unwrap(),
            &rated_map(&data).unwrap()
        ));
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

    fn acronyms(args: &PpArgs) -> Vec<&str> {
        args.mods
            .as_ref()
            .unwrap()
            .mods
            .iter()
            .map(|m| m.acronym.as_str())
            .collect()
    }

    #[test]
    fn args_parse_in_any_order() {
        let args = PpArgs::parse("+hddt 2m 98.5% 800x");

        assert_eq!(args.accuracy, Some(98.5));
        assert_eq!(args.combo, Some(800));
        assert_eq!(args.misses, Some(2));
        assert_eq!(acronyms(&args), ["HD", "DT"]);
        assert_eq!(args.mods.unwrap().mods_string, "HDDT");
        assert!(PpArgs::parse("97").is_score());
    }

    #[test]
    fn args_accept_common_spellings() {
        assert_eq!(PpArgs::parse("98,5").accuracy, Some(98.5));
        assert_eq!(PpArgs::parse("3miss").misses, Some(3));
        assert_eq!(PpArgs::parse("3MISSES").misses, Some(3));
        assert_eq!(PpArgs::parse("1200X").combo, Some(1200));
    }

    #[test]
    fn mods_alone_keep_the_spread() {
        let args = PpArgs::parse("+HR");

        assert!(!args.is_score());
        assert_eq!(acronyms(&args), ["HR"]);
    }

    #[test]
    fn nomod_overrides_with_no_mods() {
        let mods = PpArgs::parse("+NM").mods.unwrap();

        assert!(mods.mods.is_empty());
        assert_eq!(mods.mods_string, "NoMod");
    }

    #[test]
    fn invalid_args_are_ignored() {
        assert_eq!(
            PpArgs::parse("please 150 -3 x m +ZZ +HDD"),
            PpArgs::default()
        );
        assert_eq!(PpArgs::parse("").accuracy, None);
    }

    fn score_with(text: &str) -> ScorePp {
        let beatmap = Beatmap::from_bytes(TEST_MAP.as_bytes()).unwrap();
        let args = PpArgs::parse(text);

        score_pp(&rate(&beatmap, &args.mods, OsuClient::Lazer), &args)
    }

    #[test]
    fn score_pp_follows_the_arguments() {
        let full = score_with("100");

        assert_eq!(full.pp, pp_with(&[]));
        assert_eq!((full.combo, full.misses), (8, 0));
        assert!(score_with("95").pp < full.pp);
        assert!(score_with("100 1m").pp < full.pp);
        assert!(score_with("100 +DT").pp > full.pp);
    }

    fn slider_map() -> Beatmap {
        let map = TEST_MAP
            .replace("192,64,250,1,0,0:0:0:0", "100,64,250,2,0,L|240:64,1,140")
            .replace("448,64,750,1,0,0:0:0:0", "100,128,750,2,0,L|240:128,1,140")
            .replace(
                "192,192,1250,1,0,0:0:0:0",
                "100,192,1250,2,0,L|240:192,1,140",
            );

        Beatmap::from_bytes(map.as_bytes()).unwrap()
    }

    fn converted(map: &str, ruleset: Option<u8>) -> Beatmap {
        let mut beatmap = Beatmap::from_bytes(map.as_bytes()).unwrap();
        convert_to_ruleset(&mut beatmap, ruleset, &None);
        beatmap
    }

    #[test]
    fn standard_maps_convert_to_the_selected_ruleset() {
        assert_eq!(converted(TEST_MAP, None).mode, MapMode::Osu);
        assert_eq!(converted(TEST_MAP, Some(0)).mode, MapMode::Osu);
        assert_eq!(converted(TEST_MAP, Some(1)).mode, MapMode::Taiko);
        assert_eq!(converted(TEST_MAP, Some(2)).mode, MapMode::Catch);
        assert_eq!(converted(TEST_MAP, Some(3)).mode, MapMode::Mania);
    }

    #[test]
    fn converts_are_rated_in_their_ruleset() {
        let standard = rate(&converted(TEST_MAP, None), &None, OsuClient::Lazer).analysis;
        let taiko = rate(&converted(TEST_MAP, Some(1)), &None, OsuClient::Lazer).analysis;

        assert_ne!(standard.stats.stars, taiko.stats.stars);
        assert_ne!(standard.pp.pp_100, taiko.pp.pp_100);
    }

    #[test]
    fn maps_made_for_a_ruleset_are_not_converted() {
        let taiko_map = TEST_MAP.replace("Mode: 0", "Mode: 1");

        assert_eq!(converted(&taiko_map, Some(3)).mode, MapMode::Taiko);
    }

    #[test]
    fn stable_and_lazer_scores_differ() {
        let beatmap = slider_map();
        let stable = rate(&beatmap, &None, OsuClient::Stable).analysis.pp;
        let lazer = rate(&beatmap, &None, OsuClient::Lazer).analysis.pp;

        assert_ne!(stable.pp_100, lazer.pp_100);
        assert_ne!(stable.pp_95, lazer.pp_95);
    }

    #[test]
    fn a_dropped_combo_only_costs_pp_on_stable() {
        let beatmap = slider_map();
        let pp = |client, text| score_pp(&rate(&beatmap, &None, client), &PpArgs::parse(text)).pp;

        assert!(pp(OsuClient::Stable, "90 4x") < pp(OsuClient::Stable, "90"));
        assert_eq!(pp(OsuClient::Lazer, "90 4x"), pp(OsuClient::Lazer, "90"));
    }

    #[test]
    fn score_pp_clamps_to_the_map() {
        let score = score_with("100 9999x 9999m");

        assert_eq!((score.combo, score.misses), (8, 8));
    }

    #[test]
    fn mods_must_suit_each_other_and_the_mode() {
        let fit = |text, mode| mods_fit(&parse_mods(text).unwrap(), mode);

        assert!(fit("HDDT", GameMode::Osu));
        assert!(fit("NM", GameMode::Osu));
        assert!(!fit("DTHT", GameMode::Osu));
        assert!(!fit("EZHR", GameMode::Osu));
        assert!(!fit("FI", GameMode::Osu));
        assert!(fit("FI", GameMode::Mania));
    }

    #[test]
    fn an_accuracy_the_misses_rule_out_is_lowered() {
        assert_eq!(score_with("100 2m").accuracy, 75.0);
        assert_eq!(score_with("50 2m").accuracy, 50.0);
        assert_eq!(score_with("100 2m").pp, score_with("2m").pp);
    }

    #[test]
    fn misses_without_accuracy_lower_the_accuracy() {
        let score = score_with("2m");

        assert_eq!(score.accuracy, 75.0);
        assert!(score.pp < score_with("100").pp);
    }

    #[test]
    fn decimals_are_trimmed() {
        assert_eq!(decimal_text(98.5, 2), "98.5");
        assert_eq!(decimal_text(100.0, 2), "100");
        assert_eq!(decimal_text(99.256, 2), "99.26");
        assert_eq!(decimal_text(120.0, 0), "120");
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
