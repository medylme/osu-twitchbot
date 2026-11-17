use crate::osu::core::{BeatmapData, UserProfile};
use crate::osu::pp::{MapStats, PpValues, ScorePp, decimal_text};

const STAT_PLACEHOLDERS: [&str; 9] = [
    "{stars}",
    "{bpm}",
    "{length}",
    "{cs}",
    "{ar}",
    "{od}",
    "{hp}",
    "{max_combo}",
    "{objects}",
];

const STAT_UNAVAILABLE: &str = "?";

#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    pub id: Option<String>,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub diff: Option<String>,
    pub creator: Option<String>,
    pub status: Option<String>,
    pub link: Option<String>,
    pub mods: Option<String>,
    pub state: Option<String>,

    pub stars: Option<String>,
    pub bpm: Option<String>,
    pub length: Option<String>,
    pub cs: Option<String>,
    pub ar: Option<String>,
    pub od: Option<String>,
    pub hp: Option<String>,
    pub max_combo: Option<String>,
    pub objects: Option<String>,

    pub pp_95: Option<String>,
    pub pp_97: Option<String>,
    pub pp_98: Option<String>,
    pub pp_99: Option<String>,
    pub pp_100: Option<String>,

    pub acc: Option<String>,
    pub combo: Option<String>,
    pub misses: Option<String>,
    pub pp: Option<String>,

    pub username: Option<String>,
    pub rank: Option<String>,
    pub playcount: Option<String>,
    pub level: Option<String>,
}

pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut text = String::new();

    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }

    text
}

impl Placeholders {
    pub fn from_beatmap(beatmap: &BeatmapData) -> Self {
        let mods = beatmap
            .mods
            .as_ref()
            .map(|m| format!("+{}", m.mods_string))
            .unwrap_or_default();

        let link = if beatmap.id <= 0 {
            String::new()
        } else {
            format!("https://osu.ppy.sh/b/{}", beatmap.id)
        };

        Self {
            id: Some(beatmap.id.to_string()),
            artist: Some(beatmap.artist.clone()),
            title: Some(beatmap.title.clone()),
            diff: Some(beatmap.difficulty_name.clone()),
            creator: Some(beatmap.creator.clone()),
            status: Some(beatmap.status.to_string()),
            link: Some(link),
            mods: Some(mods),
            state: Some(beatmap.state.to_string()),
            ..Default::default()
        }
    }

    pub fn from_profile(profile: &UserProfile) -> Self {
        let rank = match profile.rank {
            0 => STAT_UNAVAILABLE.to_string(),
            rank => grouped(u64::from(rank)),
        };

        Self {
            username: Some(profile.username.clone()),
            rank: Some(rank),
            pp: Some(grouped(profile.pp.round() as u64)),
            acc: Some(decimal_text(profile.accuracy, 2)),
            playcount: Some(grouped(u64::from(profile.play_count))),
            level: Some(profile.level.to_string()),
            ..Default::default()
        }
    }

    pub fn sample_rank() -> Self {
        Self::from_profile(&UserProfile {
            username: "Player".to_string(),
            rank: 12345,
            pp: 8123.0,
            accuracy: 98.76,
            play_count: 54321,
            level: 100,
        })
    }

    pub fn apply_rank(&self, format: &str) -> String {
        let mut result = format.to_string();
        Self::replace(&mut result, "{username}", &self.username);
        Self::replace(&mut result, "{rank}", &self.rank);
        Self::replace(&mut result, "{pp}", &self.pp);
        Self::replace(&mut result, "{acc}", &self.acc);
        Self::replace(&mut result, "{playcount}", &self.playcount);
        Self::replace(&mut result, "{level}", &self.level);
        Self::trim(result)
    }

    pub fn uses_stats(format: &str) -> bool {
        STAT_PLACEHOLDERS.iter().any(|p| format.contains(p))
    }

    pub fn with_stats(mut self, stats: Option<&MapStats>) -> Self {
        let text = |f: fn(&MapStats) -> String| {
            Some(stats.map_or_else(|| STAT_UNAVAILABLE.to_string(), f))
        };

        self.stars = text(MapStats::stars_text);
        self.bpm = text(MapStats::bpm_text);
        self.length = text(MapStats::length_text);
        self.cs = text(|s| MapStats::setting_text(s.cs));
        self.ar = text(|s| MapStats::setting_text(s.ar));
        self.od = text(|s| MapStats::setting_text(s.od));
        self.hp = text(|s| MapStats::setting_text(s.hp));
        self.max_combo = text(|s| s.max_combo.to_string());
        self.objects = text(|s| s.objects.to_string());
        self
    }

    pub fn with_pp(mut self, pp: &PpValues) -> Self {
        self.pp_95 = Some(format!("{:.0}", pp.pp_95));
        self.pp_97 = Some(format!("{:.0}", pp.pp_97));
        self.pp_98 = Some(format!("{:.0}", pp.pp_98));
        self.pp_99 = Some(format!("{:.0}", pp.pp_99));
        self.pp_100 = Some(format!("{:.0}", pp.pp_100));
        self
    }

    pub fn with_score(mut self, score: &ScorePp) -> Self {
        self.acc = Some(decimal_text(score.accuracy, 2));
        self.combo = Some(score.combo.to_string());
        self.misses = Some(score.misses.to_string());
        self.pp = Some(format!("{:.0}", score.pp));
        self
    }

    fn sample_stats() -> MapStats {
        MapStats {
            stars: 5.67,
            bpm: 180.0,
            length_secs: 215.0,
            ar: 9.3,
            od: 8.5,
            cs: 4.0,
            hp: 5.0,
            max_combo: 1234,
            objects: 987,
        }
    }

    pub fn sample() -> Self {
        Self {
            id: Some("123456".to_string()),
            artist: Some("Artist".to_string()),
            title: Some("Title".to_string()),
            diff: Some("Difficulty".to_string()),
            creator: Some("Creator".to_string()),
            status: Some("Ranked".to_string()),
            link: Some("https://osu.ppy.sh/b/123456".to_string()),
            mods: Some("+NoMod".to_string()),
            state: Some("Playing".to_string()),
            ..Default::default()
        }
        .with_stats(Some(&Self::sample_stats()))
    }

    pub fn sample_pp() -> Self {
        Self {
            mods: Some("+NoMod".to_string()),
            pp_95: Some("350".to_string()),
            pp_97: Some("400".to_string()),
            pp_98: Some("450".to_string()),
            pp_99: Some("500".to_string()),
            pp_100: Some("550".to_string()),
            ..Default::default()
        }
    }

    pub fn sample_pp_score() -> Self {
        Self {
            mods: Some("+HDDT".to_string()),
            ..Default::default()
        }
        .with_score(&ScorePp {
            pp: 312.0,
            accuracy: 98.5,
            combo: 800,
            misses: 2,
        })
    }

    fn replace(result: &mut String, placeholder: &str, value: &Option<String>) {
        if let Some(v) = value {
            *result = result.replace(placeholder, v);
        }
    }

    pub fn apply_pp_score(&self, format: &str) -> String {
        let mut result = format.to_string();
        Self::replace(&mut result, "{mods}", &self.mods);
        Self::replace(&mut result, "{acc}", &self.acc);
        Self::replace(&mut result, "{combo}", &self.combo);
        Self::replace(&mut result, "{misses}", &self.misses);
        Self::replace(&mut result, "{pp}", &self.pp);
        Self::trim(result)
    }

    fn replace_stats(&self, result: &mut String) {
        let values = [
            &self.stars,
            &self.bpm,
            &self.length,
            &self.cs,
            &self.ar,
            &self.od,
            &self.hp,
            &self.max_combo,
            &self.objects,
        ];

        for (placeholder, value) in STAT_PLACEHOLDERS.iter().zip(values) {
            Self::replace(result, placeholder, value);
        }
    }

    fn trim(s: String) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn apply_np(&self, format: &str) -> String {
        let mut result = format.to_string();
        Self::replace(&mut result, "{id}", &self.id);
        Self::replace(&mut result, "{artist}", &self.artist);
        Self::replace(&mut result, "{title}", &self.title);
        Self::replace(&mut result, "{diff}", &self.diff);
        Self::replace(&mut result, "{creator}", &self.creator);
        Self::replace(&mut result, "{status}", &self.status);
        Self::replace(&mut result, "{link}", &self.link);
        Self::replace(&mut result, "{mods}", &self.mods);
        Self::replace(&mut result, "{state}", &self.state);
        self.replace_stats(&mut result);
        Self::trim(result)
    }

    pub fn apply_pp(&self, format: &str) -> String {
        let mut result = format.to_string();
        Self::replace(&mut result, "{mods}", &self.mods);
        Self::replace(&mut result, "{pp_95}", &self.pp_95);
        Self::replace(&mut result, "{pp_97}", &self.pp_97);
        Self::replace(&mut result, "{pp_98}", &self.pp_98);
        Self::replace(&mut result, "{pp_99}", &self.pp_99);
        Self::replace(&mut result, "{pp_100}", &self.pp_100);
        Self::trim(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> MapStats {
        MapStats {
            stars: 6.789,
            bpm: 262.5,
            length_secs: 83.4,
            ar: 10.33,
            od: 9.75,
            cs: 4.0,
            hp: 5.0,
            max_combo: 1500,
            objects: 1000,
        }
    }

    const FORMAT: &str =
        "{stars}* {bpm}bpm {length} CS{cs} AR{ar} OD{od} HP{hp} {max_combo}x {objects}";

    #[test]
    fn stats_fill_the_np_format() {
        let placeholders = Placeholders::default().with_stats(Some(&stats()));

        assert_eq!(
            placeholders.apply_np(FORMAT),
            "6.79* 262bpm 1:23 CS4 AR10.3 OD9.8 HP5 1500x 1000"
        );
    }

    #[test]
    fn stats_are_not_part_of_the_pp_format() {
        let placeholders = Placeholders::default().with_stats(Some(&stats()));

        assert_eq!(
            placeholders.apply_pp("{stars}* {pp_100}"),
            "{stars}* {pp_100}"
        );
    }

    #[test]
    fn unavailable_stats_show_a_question_mark() {
        let placeholders = Placeholders::default().with_stats(None);

        assert_eq!(placeholders.apply_np("{stars}* {bpm}bpm"), "?* ?bpm");
    }

    #[test]
    fn numbers_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn profile_fills_the_rank_format() {
        let placeholders = Placeholders::sample_rank();

        assert_eq!(
            placeholders.apply_rank("{username} #{rank} {pp}pp {acc}% {playcount} plays lv{level}"),
            "Player #12,345 8,123pp 98.76% 54,321 plays lv100"
        );
    }

    #[test]
    fn an_unranked_profile_has_no_rank() {
        let profile = UserProfile {
            username: "Player".to_string(),
            rank: 0,
            pp: 0.0,
            accuracy: 0.0,
            play_count: 0,
            level: 1,
        };

        assert_eq!(
            Placeholders::from_profile(&profile).apply_rank("#{rank}"),
            "#?"
        );
    }

    #[test]
    fn score_fills_the_pp_score_format() {
        let placeholders = Placeholders::sample_pp_score();

        assert_eq!(
            placeholders.apply_pp_score("{acc}% {combo}x {misses} miss: {pp}pp ({mods})"),
            "98.5% 800x 2 miss: 312pp (+HDDT)"
        );
    }

    #[test]
    fn stats_are_only_needed_when_the_format_uses_them() {
        assert!(Placeholders::uses_stats("{title} {stars}*"));
        assert!(!Placeholders::uses_stats("{artist} - {title} {mods}"));
    }
}
