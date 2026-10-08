//! Breeze-like widget styles drawn from the desktop's KDE color roles.
use iced::{
    Background, Border, Color, Shadow, Theme, Vector,
    border::Radius,
    widget::{button, checkbox, container, rule, scrollable, slider, text_editor, text_input},
    overlay::menu,
};

use crate::theme::{Colors, RADIUS, mix};

pub fn alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

fn border(color: Color, width: f32) -> Border {
    Border {
        color,
        width,
        radius: Radius::from(RADIUS),
    }
}

/// Breeze draws every control with the same dim outline.
fn outline(c: Colors) -> Color {
    c.border
}

pub fn window(c: Colors) -> container::Style {
    container::Style {
        background: Some(c.window.into()),
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A list/view surface: View color with a thin outline.
pub fn view_area(c: Colors) -> container::Style {
    container::Style {
        background: Some(c.base.into()),
        text_color: Some(c.text),
        border: border(outline(c), 1.0),
        ..Default::default()
    }
}

/// Kirigami-style card.
pub fn card(c: Colors) -> container::Style {
    container::Style {
        background: Some(c.base.into()),
        text_color: Some(c.text),
        border: border(outline(c), 1.0),
        ..Default::default()
    }
}

/// The sidebar sits on the darker View color, like System Settings.
pub fn sidebar(c: Colors) -> container::Style {
    container::Style {
        background: Some(c.base.into()),
        text_color: Some(c.text),
        ..Default::default()
    }
}

pub fn status_bar(c: Colors) -> container::Style {
    window(c)
}

pub fn toolbar(c: Colors) -> container::Style {
    window(c)
}

pub fn row(c: Colors, selected: bool, hovered: bool, odd: bool) -> container::Style {
    let (background, text_color) = if selected {
        (Some(c.selection), c.selection_text)
    } else if hovered {
        (Some(alpha(c.hover, 0.18)), c.text)
    } else if odd {
        (Some(c.alt_base), c.text)
    } else {
        (None, c.text)
    };
    container::Style {
        background: background.map(Background::from),
        text_color: Some(text_color),
        border: Border {
            radius: Radius::from(3.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Kirigami `InlineMessage`: a tinted banner with a colored outline.
pub fn inline_message(c: Colors, tone: Color) -> container::Style {
    container::Style {
        background: Some(mix(c.window, tone, 0.18).into()),
        text_color: Some(c.text),
        border: border(tone, 1.0),
        ..Default::default()
    }
}

pub fn tooltip(c: Colors) -> container::Style {
    container::Style {
        background: Some(c.window.into()),
        text_color: Some(c.text),
        border: border(outline(c), 1.0),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..Default::default()
    }
}

pub fn separator(c: Colors) -> impl Fn(&Theme) -> rule::Style {
    move |_| rule::Style {
        color: outline(c),
        radius: Radius::from(0.0),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// A push button. `flat` buttons (toolbar, sidebar) only show a frame when
/// hovered or pressed, as Breeze tool buttons do.
pub fn button_style(c: Colors, flat: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (background, border_color, text) = match status {
            button::Status::Active => (
                (!flat).then_some(c.button),
                if flat { Color::TRANSPARENT } else { outline(c) },
                c.button_text,
            ),
            button::Status::Hovered => (
                Some(if flat { alpha(c.hover, 0.16) } else { mix(c.button, c.hover, 0.12) }),
                c.hover,
                c.button_text,
            ),
            button::Status::Pressed => (
                Some(mix(c.button, c.focus, 0.3)),
                c.focus,
                c.button_text,
            ),
            button::Status::Disabled => (
                (!flat).then_some(mix(c.window, c.button, 0.5)),
                if flat { Color::TRANSPARENT } else { alpha(outline(c), 0.5) },
                c.muted,
            ),
        };
        button::Style {
            background: background.map(Background::from),
            text_color: text,
            border: border(border_color, 1.0),
            ..Default::default()
        }
    }
}

/// An item in the page sidebar.
pub fn nav_style(c: Colors, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (background, text) = if selected {
            (Some(c.selection), c.selection_text)
        } else {
            match status {
                button::Status::Hovered => (Some(alpha(c.hover, 0.18)), c.text),
                button::Status::Pressed => (Some(alpha(c.focus, 0.3)), c.text),
                _ => (None, c.text),
            }
        };
        button::Style {
            background: background.map(Background::from),
            text_color: text,
            border: border(Color::TRANSPARENT, 0.0),
            ..Default::default()
        }
    }
}

pub fn text_input_style(c: Colors) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let border_color = match status {
            text_input::Status::Active => outline(c),
            text_input::Status::Hovered => c.hover,
            text_input::Status::Focused { .. } => c.focus,
            text_input::Status::Disabled => alpha(outline(c), 0.5),
        };
        text_input::Style {
            background: c.base.into(),
            border: border(border_color, 1.0),
            icon: c.muted,
            placeholder: c.muted,
            value: if matches!(status, text_input::Status::Disabled) { c.muted } else { c.text },
            selection: alpha(c.selection, 0.55),
        }
    }
}

pub fn text_editor_style(c: Colors) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, status| {
        let border_color = match status {
            text_editor::Status::Active => outline(c),
            text_editor::Status::Hovered => c.hover,
            text_editor::Status::Focused { .. } => c.focus,
            text_editor::Status::Disabled => alpha(outline(c), 0.5),
        };
        text_editor::Style {
            background: c.base.into(),
            border: border(border_color, 1.0),
            placeholder: c.muted,
            value: c.text,
            selection: alpha(c.selection, 0.55),
        }
    }
}

pub fn slider_style(c: Colors, enabled: bool) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let filled = if enabled { c.focus } else { c.muted };
        let groove = mix(c.window, c.text, if c.dark { 0.18 } else { 0.22 });
        let handle_border = match status {
            _ if !enabled => alpha(outline(c), 0.6),
            slider::Status::Active => outline(c),
            slider::Status::Hovered => c.hover,
            slider::Status::Dragged => c.focus,
        };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (filled.into(), groove.into()),
                width: 6.0,
                border: Border {
                    radius: Radius::from(3.0),
                    ..Default::default()
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 8.0 },
                background: if enabled { c.button } else { mix(c.window, c.button, 0.5) }.into(),
                border_width: 1.0,
                border_color: handle_border,
            },
        }
    }
}

pub fn checkbox_style(c: Colors) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    move |_, status| {
        let (checked, hovered, disabled) = match status {
            checkbox::Status::Active { is_checked } => (is_checked, false, false),
            checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
            checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
        };
        let accent = if disabled { c.muted } else { c.focus };
        checkbox::Style {
            background: if checked { accent } else { c.base }.into(),
            icon_color: c.selection_text,
            border: Border {
                color: if checked || hovered { accent } else { outline(c) },
                width: 1.0,
                radius: Radius::from(3.0),
            },
            text_color: Some(if disabled { c.muted } else { c.text }),
        }
    }
}

pub fn scrollable_style(c: Colors) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |_, status| {
        let (hovered, dragged) = match status {
            scrollable::Status::Active { .. } => (false, false),
            scrollable::Status::Hovered {
                is_vertical_scrollbar_hovered,
                ..
            } => (is_vertical_scrollbar_hovered, false),
            scrollable::Status::Dragged {
                is_vertical_scrollbar_dragged,
                ..
            } => (false, is_vertical_scrollbar_dragged),
        };
        let scroller = if dragged {
            c.focus
        } else if hovered {
            c.hover
        } else {
            alpha(c.text, 0.35)
        };
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: scroller.into(),
                border: Border {
                    radius: Radius::from(3.0),
                    ..Default::default()
                },
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: c.window.into(),
                border: border(outline(c), 1.0),
                shadow: Shadow::default(),
                icon: c.text,
            },
        }
    }
}

pub fn menu_style(c: Colors) -> impl Fn(&Theme) -> menu::Style {
    move |_| menu::Style {
        background: c.window.into(),
        border: border(outline(c), 1.0),
        text_color: c.text,
        selected_text_color: c.selection_text,
        selected_background: c.selection.into(),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
    }
}
