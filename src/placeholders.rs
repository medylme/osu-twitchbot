use crate::osu::core::BeatmapData;
use crate::osu::pp::PpValues;

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

    pub pp_95: Option<String>,
    pub pp_97: Option<String>,
    pub pp_98: Option<String>,
    pub pp_99: Option<String>,
    pub pp_100: Option<String>,
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
            ..Default::default()
        }
    }

    pub fn with_pp(mut self, pp: &PpValues) -> Self {
        self.pp_95 = Some(format!("{:.0}", pp.pp_95));
        self.pp_97 = Some(format!("{:.0}", pp.pp_97));
        self.pp_98 = Some(format!("{:.0}", pp.pp_98));
        self.pp_99 = Some(format!("{:.0}", pp.pp_99));
        self.pp_100 = Some(format!("{:.0}", pp.pp_100));
        self
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
            ..Default::default()
        }
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

    fn replace(result: &mut String, placeholder: &str, value: &Option<String>) {
        if let Some(v) = value {
            *result = result.replace(placeholder, v);
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
