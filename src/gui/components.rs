use iced::border::Radius;
use iced::widget::{button, container, text_input};
use iced::{Border, Color, Font, Theme, color};

// #969EFF
pub const ACCENT_COLOR: Color = Color {
    r: 0.588,
    g: 0.620,
    b: 1.0,
    a: 1.0,
};

pub const BOLD_FONT: Font = Font {
    weight: iced::font::Weight::Bold,
    family: iced::font::Family::SansSerif,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

pub fn primary_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(ACCENT_COLOR)),
        text_color: Color {
            r: 0.11,
            g: 0.11,
            b: 0.15,
            a: 1.0,
        },
        border: iced::Border {
            radius: Radius::new(20.0),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Default::default(),
        snap: true,
    }
}

pub fn primary_text_input(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: iced::Background::Color(Color {
            r: 0.11,
            g: 0.11,
            b: 0.15,
            a: 1.0,
        }),
        border: iced::Border {
            radius: Radius::new(8.0),
            width: 1.5,
            color: ACCENT_COLOR,
        },
        icon: Color {
            r: 0.95,
            g: 0.95,
            b: 0.96,
            a: 1.0,
        },
        placeholder: Color {
            r: 0.95,
            g: 0.95,
            b: 0.96,
            a: 0.5,
        },
        value: Color {
            r: 0.95,
            g: 0.95,
            b: 0.96,
            a: 1.0,
        },
        selection: Color {
            r: 0.588,
            g: 0.620,
            b: 1.0,
            a: 0.3,
        },
    }
}

pub fn tab_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(color!(0x3a3a3a).into()),
        text_color: color!(0xaaaaaa),
        border: Border {
            color: color!(0x444444),
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

pub fn tab_button_active(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(color!(0x4a4a4a).into()),
        text_color: color!(0xffffff),
        border: Border {
            color: color!(0x666666),
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

pub fn code_block_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(color!(0x1a1a1a).into()),
        border: Border {
            color: color!(0x333333),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}
