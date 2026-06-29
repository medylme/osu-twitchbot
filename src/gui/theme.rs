use std::sync::OnceLock;

use iced::{Color, Theme, color};

static THEME_OVERRIDE: OnceLock<ThemeOverride> = OnceLock::new();
static SYSTEM_THEME: OnceLock<Theme> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeOverride {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeOverride {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "system" | "auto" => Some(Self::System),
            _ => None,
        }
    }
}

pub fn set_theme_override(theme_override: ThemeOverride) {
    let _ = THEME_OVERRIDE.set(theme_override);
}

pub fn get_current_theme() -> Theme {
    let override_setting = THEME_OVERRIDE.get().copied().unwrap_or_default();
    match override_setting {
        ThemeOverride::Light => Theme::Light,
        ThemeOverride::Dark => Theme::Dark,
        // detection does a blocking round-trip (d-bus on linux) and this is
        // called every redraw, so detect once and cache
        ThemeOverride::System => SYSTEM_THEME.get_or_init(detect_system_theme).clone(),
    }
}

fn detect_system_theme() -> Theme {
    match dark_light::detect() {
        dark_light::Mode::Dark => Theme::Dark,
        dark_light::Mode::Light => Theme::Light,
        dark_light::Mode::Default => Theme::Dark,
    }
}

pub struct ColorPalette {
    pub bg_primary: Color,
    pub bg_rail: Color,
    pub bg_secondary: Color,
    pub bg_input: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_on_accent: Color,

    pub accent: Color,
    pub accent_alt: Color,

    pub border_subtle: Color,
    pub border_default: Color,

    pub status_success: Color,
    pub status_warning: Color,
    pub status_error: Color,
    pub status_info: Color,
    pub status_module: Color,
}

pub fn dark_palette() -> ColorPalette {
    ColorPalette {
        bg_primary: color!(0x14141c),
        bg_rail: color!(0x181820),
        bg_secondary: color!(0x1c1c27),
        bg_input: color!(0x23232f),

        text_primary: color!(0xf4f4f7),
        text_secondary: color!(0x8a8aa0),
        text_muted: color!(0x5b5b72),
        text_on_accent: color!(0x16121f),

        accent: color!(0x969eff),
        accent_alt: color!(0xb694f8),

        border_subtle: color!(0x222230),
        border_default: color!(0x2a2a3a),

        status_success: color!(0x63d2a0),
        status_warning: color!(0xffc66d),
        status_error: color!(0xff6b8a),
        status_info: color!(0x69b4ff),
        status_module: color!(0x969eff),
    }
}

pub fn light_palette() -> ColorPalette {
    ColorPalette {
        bg_primary: color!(0xfafafc),
        bg_rail: color!(0xf1f1f6),
        bg_secondary: color!(0xffffff),
        bg_input: color!(0xffffff),

        text_primary: color!(0x1d1d26),
        text_secondary: color!(0x5f5f74),
        text_muted: color!(0x9797a8),
        text_on_accent: color!(0x16121f),

        accent: color!(0x6b74e0),
        accent_alt: color!(0x8a63d2),

        border_subtle: color!(0xe6e6ee),
        border_default: color!(0xd8d8e4),

        status_success: color!(0x2f9e6c),
        status_warning: color!(0xc07c1d),
        status_error: color!(0xd4486a),
        status_info: color!(0x1976d2),
        status_module: color!(0x6b74e0),
    }
}

pub fn palette(theme: &Theme) -> ColorPalette {
    if theme.extended_palette().is_dark {
        dark_palette()
    } else {
        light_palette()
    }
}
