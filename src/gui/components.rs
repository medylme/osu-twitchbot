use iced::border::Radius;
use iced::widget::{button, container, text_input};
use iced::{Border, Color, Font, Theme};

use super::theme::palette;

pub const BOLD_FONT: Font = Font {
    weight: iced::font::Weight::Bold,
    family: iced::font::Family::SansSerif,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

pub const MONO_BOLD_FONT: Font = Font {
    weight: iced::font::Weight::Bold,
    family: iced::font::Family::Monospace,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

fn tint(base: Color, alpha: f32) -> Color {
    Color { a: alpha, ..base }
}

pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => p.accent_alt,
        button::Status::Disabled => tint(p.accent, 0.4),
        _ => p.accent,
    };
    button::Style {
        background: Some(background.into()),
        text_color: p.text_on_accent,
        border: Border {
            radius: Radius::new(999.0),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Default::default(),
        snap: true,
    }
}

pub fn ghost_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| tint(p.text_primary, 0.05).into()),
        text_color: if hovered {
            p.text_primary
        } else {
            p.text_secondary
        },
        border: Border {
            radius: Radius::new(999.0),
            width: 1.0,
            color: p.border_default,
        },
        ..Default::default()
    }
}

pub fn nav_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| tint(p.text_primary, 0.04).into()),
        text_color: if hovered {
            p.text_primary
        } else {
            p.text_secondary
        },
        border: Border {
            radius: Radius::new(9.0),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

pub fn nav_button_active(theme: &Theme, _status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(tint(p.accent, 0.14).into()),
        text_color: p.text_primary,
        border: Border {
            radius: Radius::new(9.0),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

pub fn primary_text_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => p.accent,
        text_input::Status::Hovered => tint(p.accent, 0.55),
        _ => p.border_default,
    };
    text_input::Style {
        background: p.bg_input.into(),
        border: Border {
            radius: Radius::new(9.0),
            width: 1.0,
            color: border_color,
        },
        icon: p.text_primary,
        placeholder: p.text_muted,
        value: p.text_primary,
        selection: tint(p.accent, 0.3),
    }
}

pub fn card_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_secondary.into()),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

pub fn rail_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_rail.into()),
        ..Default::default()
    }
}

pub fn window_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_primary.into()),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn separator(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.border_subtle.into()),
        ..Default::default()
    }
}
