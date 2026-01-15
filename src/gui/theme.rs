use std::sync::OnceLock;

use iced::{Color, Theme, color};

static THEME_OVERRIDE: OnceLock<ThemeOverride> = OnceLock::new();

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
        ThemeOverride::System => detect_system_theme(),
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
    pub bg_secondary: Color,
    pub bg_tertiary: Color,
    pub bg_elevated: Color,
    pub bg_input: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_on_accent: Color,

    pub accent: Color,
    pub accent_alt: Color,

    pub border_subtle: Color,
    pub border_muted: Color,
    pub border_default: Color,

    pub status_success: Color,
    pub status_warning: Color,
    pub status_error: Color,
    pub status_info: Color,
    pub status_module: Color,
}

pub fn dark_palette() -> ColorPalette {
    ColorPalette {
        bg_primary: color!(0x1a1a1a),
        bg_secondary: color!(0x2a2a2a),
        bg_tertiary: color!(0x3a3a3a),
        bg_elevated: color!(0x4a4a4a),
        bg_input: color!(0x1b1b26),

        text_primary: color!(0xf5f5f6),
        text_secondary: color!(0x888888),
        text_muted: color!(0x666666),
        text_on_accent: color!(0x1b1b26),

        accent: color!(0x969eff),
        accent_alt: color!(0xb08af5),

        border_subtle: color!(0x333333),
        border_muted: color!(0x444444),
        border_default: color!(0x666666),

        status_success: color!(0x4caf50),
        status_warning: color!(0xffc107),
        status_error: color!(0xf44336),
        status_info: color!(0x2196f3),
        status_module: color!(0x69b4ff),
    }
}

pub fn light_palette() -> ColorPalette {
    ColorPalette {
        bg_primary: color!(0xffffff),
        bg_secondary: color!(0xf0f0f0),
        bg_tertiary: color!(0xe5e5e5),
        bg_elevated: color!(0xd8d8d8),
        bg_input: color!(0xffffff),

        text_primary: color!(0x1a1a1a),
        text_secondary: color!(0x666666),
        text_muted: color!(0x999999),
        text_on_accent: color!(0xffffff),

        accent: color!(0x969eff),
        accent_alt: color!(0x7e57c2),

        border_subtle: color!(0xdddddd),
        border_muted: color!(0xcccccc),
        border_default: color!(0xaaaaaa),

        status_success: color!(0x388e3c),
        status_warning: color!(0xf57c00),
        status_error: color!(0xd32f2f),
        status_info: color!(0x1976d2),
        status_module: color!(0x1565c0),
    }
}

pub fn palette(theme: &Theme) -> ColorPalette {
    if theme.extended_palette().is_dark {
        dark_palette()
    } else {
        light_palette()
    }
}
