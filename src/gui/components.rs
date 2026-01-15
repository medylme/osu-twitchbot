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

pub fn primary_button(theme: &Theme, _status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(iced::Background::Color(p.accent)),
        text_color: p.text_on_accent,
        border: Border {
            radius: Radius::new(20.0),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Default::default(),
        snap: true,
    }
}

pub fn primary_text_input(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    text_input::Style {
        background: iced::Background::Color(p.bg_input),
        border: Border {
            radius: Radius::new(8.0),
            width: 1.5,
            color: p.accent,
        },
        icon: p.text_primary,
        placeholder: Color {
            a: 0.5,
            ..p.text_primary
        },
        value: p.text_primary,
        selection: Color { a: 0.3, ..p.accent },
    }
}

pub fn tab_button(theme: &Theme, _status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(p.bg_tertiary.into()),
        text_color: p.text_secondary,
        border: Border {
            color: p.border_muted,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

pub fn tab_button_active(theme: &Theme, _status: button::Status) -> button::Style {
    let p = palette(theme);
    button::Style {
        background: Some(p.bg_elevated.into()),
        text_color: p.text_primary,
        border: Border {
            color: p.border_default,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

pub fn code_block_container(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_tertiary.into()),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}
