//! The interface, laid out like a KDE application: a global toolbar, a
//! System Settings–style page sidebar, Kirigami form layouts, inline messages
//! and a status bar.
use iced::{
    Alignment, Element, Font, Length, Padding,
    font::Weight,
    widget::{
        Column, Row, Text, button, center, checkbox, column, combo_box, container, mouse_area,
        responsive, row, rule, scrollable, slider, space, text,
        text_editor, text_input, tooltip,
    },
};

use super::*;
use crate::{icons::Icon, style};

type El<'a> = Element<'a, Message>;

const SIDEBAR_WIDTH: f32 = 196.0;
const LABEL_WIDTH: f32 = 150.0;

fn t<'a>(l: Look, content: impl iced::widget::text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(l.size).font(l.font)
}

fn muted<'a>(l: Look, content: impl iced::widget::text::IntoFragment<'a>) -> Text<'a> {
    t(l, content).color(l.c.muted)
}

fn small<'a>(l: Look, content: impl iced::widget::text::IntoFragment<'a>) -> Text<'a> {
    text(content)
        .size((l.size - 1.0).max(10.0))
        .font(l.font)
        .color(l.c.muted)
}

fn bold(l: Look) -> Font {
    Font {
        weight: Weight::Bold,
        ..l.font
    }
}

fn heading<'a>(l: Look, content: impl iced::widget::text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(l.size * 1.6).font(bold(l))
}

fn hrule<'a>(l: Look) -> El<'a> {
    rule::horizontal(1).style(style::separator(l.c)).into()
}

fn vrule<'a>(l: Look) -> El<'a> {
    rule::vertical(1).style(style::separator(l.c)).into()
}

/// A separator between toolbar items, which cannot fill an unbounded height.
fn vsep<'a>(l: Look) -> El<'a> {
    container(vrule(l)).height(20).padding([0, 4]).into()
}

impl ClickerApp {
    pub fn view(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let content = column![self.messages(), self.page_view()]
            .width(Length::Fill)
            .height(Length::Fill);
        let body = row![self.sidebar(), vrule(l), content].height(Length::Fill);
        container(column![self.toolbar(), hrule(l), body, hrule(l), self.status_bar()])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| style::window(c))
            .into()
    }

    /// An icon followed by a label, dimmed when disabled.
    fn labelled<'a>(&'a self, icon: Icon, label: impl Into<String>, enabled: bool) -> El<'a> {
        let l = self.look;
        let mut content = Row::new().spacing(6).align_y(Alignment::Center);
        let glyph = if enabled {
            self.icons.view(icon, 18.0)
        } else {
            self.icons.view_dim(icon, 18.0, 0.45)
        };
        if let Some(glyph) = glyph {
            content = content.push(glyph);
        }
        content
            .push(t(l, label.into()).wrapping(iced::widget::text::Wrapping::None))
            .into()
    }

    fn tip<'a>(
        &self,
        content: impl Into<El<'a>>,
        tip: impl iced::widget::text::IntoFragment<'a>,
    ) -> El<'a> {
        let l = self.look;
        let c = l.c;
        tooltip(
            content,
            container(t(l, tip))
                .padding([4, 8])
                .style(move |_| style::tooltip(c)),
            tooltip::Position::Bottom,
        )
        .gap(4)
        .into()
    }

    /// A Breeze tool button: flat until hovered.
    fn tool_button<'a>(
        &'a self,
        icon: Icon,
        label: &'a str,
        message: Message,
        enabled: bool,
        tip: &'a str,
    ) -> El<'a> {
        let c = self.look.c;
        let button = button(self.labelled(icon, label, enabled))
            .padding([5, 10])
            .on_press_maybe(enabled.then_some(message))
            .style(style::button_style(c, true));
        self.tip(button, tip)
    }

    /// The binding's key, or a prompt while a new one is being chosen.
    fn hotkey_label(&self, index: usize) -> String {
        if self.selecting_hotkey == Some(index) {
            "Press a key…".into()
        } else {
            self.settings
                .active()
                .bindings
                .get(index)
                .map_or_else(String::new, |binding| key_name(binding.hotkey))
        }
    }

    /// A button that shows a binding's key and changes it when clicked.
    fn hotkey_button(&self, index: usize, flat: bool) -> El<'_> {
        let c = self.look.c;
        let mouse = self
            .settings
            .active()
            .bindings
            .get(index)
            .is_some_and(|binding| !model::is_keyboard(binding.hotkey));
        let icon = if mouse { Icon::Mouse } else { Icon::Keyboard };
        let button = button(self.labelled(icon, self.hotkey_label(index), true))
            .padding([5, 10])
            .on_press_maybe(
                (self.selecting_hotkey == Some(index) || self.idle())
                    .then_some(Message::SelectHotkey(index)),
            )
            .style(style::button_style(c, flat));
        self.tip(
            button,
            "Global shortcut. Click, then press a key or a side mouse button.",
        )
    }

    fn toolbar(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let running = self.running();
        let can_start = self.idle();
        container(
            row![
                self.tool_button(
                    Icon::Start,
                    "Start",
                    Message::Start,
                    can_start,
                    "Start the macro shown on the Sequence page",
                ),
                self.tool_button(Icon::Stop, "Stop", Message::Stop, running, "Stop every macro"),
                space().width(Length::Fill),
                muted(l, format!("{} · Start / stop key", self.settings.active().name)),
                self.hotkey_button(self.settings.binding, true),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |_| style::toolbar(c))
        .into()
    }

    fn sidebar(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let item = |page: Page, icon: Icon, label: &'static str| -> El<'_> {
            let selected = self.page == page;
            let glyph = if selected {
                self.icons.view_on_accent(icon, 22.0)
            } else {
                self.icons.view(icon, 22.0)
            };
            let mut content = Row::new().spacing(10).align_y(Alignment::Center);
            if let Some(glyph) = glyph {
                content = content.push(glyph);
            }
            button(content.push(t(l, label)))
                .padding([9, 12])
                .width(Length::Fill)
                .on_press(Message::Navigate(page))
                .style(style::nav_style(c, selected))
                .into()
        };
        container(
            column![
                column![
                    text("Clicker").size(l.size * 1.5).font(bold(l)),
                    small(l, "Auto clicker"),
                ]
                .spacing(2)
                .padding(Padding {
                    top: 8.0,
                    right: 12.0,
                    bottom: 14.0,
                    left: 12.0,
                }),
                item(Page::Profiles, Icon::Keyboard, "Profiles"),
                item(Page::Sequence, Icon::Sequence, "Sequence"),
                item(Page::Playback, Icon::Settings, "Playback"),
                item(Page::Test, Icon::Test, "Test"),
            ]
            .spacing(2),
        )
        .padding(8)
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill)
        .style(move |_| style::sidebar(c))
        .into()
    }

    /// Kirigami `InlineMessage` banners above the page.
    fn messages(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let mut messages = Column::new();
        if let Some(error) = &self.error {
            messages = messages.push(self.banner(
                Icon::Error,
                c.error,
                t(l, error.as_str()).into(),
                true,
            ));
        }
        if self.input.status().connected == 0 {
            messages = messages.push(self.banner(
                Icon::Warning,
                c.neutral,
                t(
                    l,
                    "No readable input devices. See the Input permissions section of README.md.",
                )
                .into(),
                false,
            ));
        }
        messages.into()
    }

    fn banner<'a>(&'a self, icon: Icon, tone: iced::Color, body: El<'a>, close: bool) -> El<'a> {
        let c = self.look.c;
        let mut line = Row::new().spacing(10).align_y(Alignment::Center);
        if let Some(glyph) = self.icons.view(icon, 22.0) {
            line = line.push(glyph);
        }
        line = line.push(container(body).width(Length::Fill));
        if close {
            line = line.push(self.tool_button(
                Icon::Close,
                "Dismiss",
                Message::DismissError,
                true,
                "Dismiss this message",
            ));
        }
        container(
            container(line)
                .padding([6, 10])
                .style(move |_| style::inline_message(c, tone)),
        )
        .padding(Padding {
            top: 12.0,
            right: 16.0,
            bottom: 0.0,
            left: 16.0,
        })
        .into()
    }

    fn page_view(&self) -> El<'_> {
        match self.page {
            Page::Profiles => self.profiles_page(),
            Page::Sequence => self.sequence_page(),
            Page::Playback => self.playback_page(),
            Page::Test => self.test_page(),
        }
    }

    fn page_header<'a>(&self, title: &'a str, subtitle: String) -> El<'a> {
        let l = self.look;
        column![heading(l, title), muted(l, subtitle)]
            .spacing(2)
            .into()
    }

    // ---------------------------------------------------------------- Profiles

    fn profiles_page(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let editable = self.idle();
        let mut profiles = Row::new().spacing(4);
        for (index, profile) in self.settings.profiles.iter().enumerate() {
            let active = index == self.settings.profile;
            profiles = profiles.push(
                button(t(l, profile.name.as_str()))
                    .padding([4, 10])
                    .on_press_maybe(editable.then_some(Message::SelectProfile(index)))
                    .style(style::nav_style(c, active)),
            );
        }
        let profile_actions = row![
            self.tool_button(Icon::Add, "Add", Message::AddProfile, editable, "Add an empty profile"),
            self.tool_button(
                Icon::Delete,
                "Delete",
                Message::DeleteProfile,
                editable && self.settings.profiles.len() > 1,
                "Delete the active profile",
            ),
        ]
        .spacing(2);
        let name = text_input("Profile name", &self.settings.active().name)
            .on_input(Message::RenameProfile)
            .size(l.size)
            .font(l.font)
            .padding([4, 8])
            .width(240)
            .style(style::text_input_style(c));
        let bindings = &self.settings.active().bindings;
        let mut keys = Column::new();
        for (index, binding) in bindings.iter().enumerate() {
            let running = self.engines.get(index).is_some_and(Engine::running);
            let steps = match binding.steps.len() {
                1 => "1 step".to_string(),
                n => format!("{n} steps"),
            };
            let hold = checkbox(binding.hold)
                .label("Hold to play")
                .on_toggle_maybe(editable.then_some(move |hold| Message::SetHold(index, hold)))
                .size(16)
                .text_size(l.size)
                .font(l.font)
                .style(style::checkbox_style(c));
            let (toggle_icon, toggle_label, toggle_tip) = if running {
                (Icon::Stop, "Stop", "Stop this macro")
            } else {
                (Icon::Start, "Start", "Start this macro")
            };
            let line = row![
                container(self.hotkey_button(index, false)).width(170),
                container(hold).width(130),
                small(l, steps),
                space().width(Length::Fill),
                self.tool_button(
                    toggle_icon,
                    toggle_label,
                    Message::ToggleBinding(index),
                    running || (!self.recording && self.selecting_hotkey.is_none()),
                    toggle_tip,
                ),
                self.tool_button(
                    Icon::Edit,
                    "Edit",
                    Message::EditBinding(index),
                    editable,
                    "Record or edit this key's sequence",
                ),
                self.tool_button(
                    Icon::Delete,
                    "Remove",
                    Message::RemoveBinding(index),
                    editable && bindings.len() > 1,
                    "Remove this key and its sequence",
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            keys = keys.push(
                container(line)
                    .padding([4, 10])
                    .width(Length::Fill)
                    .style(move |_| style::row(c, false, false, index % 2 == 1)),
            );
        }
        let keys = container(scrollable(keys).style(style::scrollable_style(c)))
            .padding(1)
            .width(Length::Fill)
            .style(move |_| style::view_area(c));
        let form = column![
            self.form_section("Profile"),
            self.form_row("Active profile:", column![profiles.wrap(), profile_actions].spacing(6)),
            self.form_row("Name:", name),
            self.form_hint("Only the active profile's keys are listened for."),
            space().height(8),
            self.form_section("Keys"),
        ]
        .spacing(10)
        .max_width(640);
        column![
            self.page_header(
                "Profiles",
                "Give each key its own macro, and keep several sets of keys".into(),
            ),
            space().height(8),
            form,
            keys,
            row![
                self.tool_button(Icon::Add, "Add key", Message::AddBinding, editable, "Add a key with its own macro"),
                small(
                    l,
                    "Press a key once to start its macro and again to stop it, or tick Hold to play only while it is held.",
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(10)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // ---------------------------------------------------------------- Sequence

    fn sequence_page(&self) -> El<'_> {
        let editable = self.idle();
        let steps = self.settings.current().steps.len();
        let (record_icon, record_label) = if self.recording {
            (Icon::Stop, "Finish")
        } else {
            (Icon::Record, "Record")
        };
        let actions = row![
            self.tool_button(
                record_icon,
                record_label,
                Message::Record,
                editable || self.recording,
                "Record starts a new sequence.",
            ),
            self.tool_button(Icon::Clear, "Clear", Message::Clear, editable, "Remove every step"),
        ]
        .spacing(2)
        .height(Length::Shrink)
        .align_y(Alignment::Center);
        let key = key_name(self.settings.current().hotkey);
        let subtitle = match steps {
            0 => format!("Macro for {key} · No steps"),
            1 => format!("Macro for {key} · 1 step · Click a key or delay to edit"),
            n => format!("Macro for {key} · {n} steps · Click a key or delay to edit"),
        };
        let mut page = Column::new()
            .spacing(10)
            .push(self.page_header("Sequence", subtitle));
        let bindings = &self.settings.active().bindings;
        if bindings.len() > 1 {
            let c = self.look.c;
            let mut keys = Row::new().spacing(4);
            for (index, binding) in bindings.iter().enumerate() {
                let shown = index == self.settings.binding;
                keys = keys.push(
                    button(t(self.look, key_name(binding.hotkey)))
                        .padding([4, 10])
                        .on_press_maybe((editable || shown).then_some(Message::EditBinding(index)))
                        .style(style::nav_style(c, shown)),
                );
            }
            page = page.push(keys.wrap());
        }
        let mut page = page.push(actions);
        if self.recording {
            page = page.push(self.recording_banner());
        }
        page.push(self.sequence_list())
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn recording_banner(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let mut line = Row::new().spacing(10).align_y(Alignment::Center);
        if let Some(glyph) = self.icons.view(Icon::Record, 18.0) {
            line = line.push(glyph);
        }
        container(line.push(t(
            l,
            "Recording. Keep the pointer over the list below, then type or click.",
        )))
        .padding([6, 10])
        .width(Length::Fill)
        .style(move |_| style::inline_message(c, c.focus))
        .into()
    }

    fn sequence_list(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let steps = self.settings.current().steps.len();
        let body: El = if steps == 0 {
            self.placeholder()
        } else {
            let header = container(
                row![
                    small(l, "#").width(44),
                    small(l, "Action").width(100),
                    small(l, "Key or delay"),
                ]
                .spacing(8),
            )
            .padding([5, 10])
            .width(Length::Fill)
            .style(move |_| style::row(c, false, false, true));
            let list = responsive(move |size| {
                // The stored offset can overshoot when the list shrinks or the
                // window grows, which would leave blank rows at the top.
                let max_scroll = (steps as f32 * ROW_HEIGHT - size.height).max(0.0);
                let first = ((self.scroll_y.min(max_scroll) / ROW_HEIGHT) as usize)
                    .saturating_sub(4)
                    .min(steps);
                let last = (first + (size.height / ROW_HEIGHT).ceil() as usize + 9).min(steps);
                let mut rows = Column::new()
                    .push(space().height(first as f32 * ROW_HEIGHT))
                    .width(Length::Fill);
                for index in first..last {
                    rows = rows.push(self.step_row(index, &self.settings.current().steps[index]));
                }
                rows = rows.push(space().height((steps - last) as f32 * ROW_HEIGHT));
                scrollable(rows)
                    .id(iced::widget::Id::new(STEPS_ID))
                    .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new().width(8).scroller_width(6).margin(2),
                    ))
                    .style(style::scrollable_style(c))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            });
            column![header, list].into()
        };
        // Always wrapped, so pointer tracking survives layout changes.
        mouse_area(
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(1)
                .style(move |_| style::view_area(c)),
        )
        .on_enter(Message::RecordAreaEntered)
        .on_exit(Message::RecordAreaExited)
        .into()
    }

    /// Kirigami `PlaceholderMessage`.
    fn placeholder(&self) -> El<'_> {
        let l = self.look;
        let (title, body) = if self.recording {
            (
                "Listening…",
                "Type or click here. Each press and release is recorded with its delay.",
            )
        } else {
            (
                "No steps yet",
                "Record keys and clicks, or add steps by hand.",
            )
        };
        let mut content = Column::new().spacing(8).align_x(Alignment::Center);
        if let Some(glyph) = self.icons.view_dim(Icon::Sequence, 64.0, 0.5) {
            content = content.push(glyph);
        }
        content = content
            .push(text(title).size(l.size * 1.5).font(l.font))
            .push(muted(l, body));
        if !self.recording {
            content = content.push(space().height(6)).push(
                button(self.labelled(Icon::Record, "Record", self.idle()))
                    .padding([6, 14])
                    .on_press_maybe(self.idle().then_some(Message::Record))
                    .style(style::button_style(l.c, false)),
            );
        }
        center(content).into()
    }

    fn step_row<'a>(&'a self, index: usize, step: &Step) -> El<'a> {
        let l = self.look;
        let c = l.c;
        let selected = self.selected == Some(index);
        let hovered = self.hovered_row == Some(index);
        let edit = self.editing.as_ref().filter(|edit| edit.index == index);
        let glyph = |icon: Icon| -> El<'a> {
            if selected {
                self.icons.view_on_accent(icon, 16.0)
            } else {
                self.icons.view(icon, 16.0)
            }
            .unwrap_or_else(|| space().width(16).into())
        };
        let clickable = |content: El<'a>, message: Message| -> El<'a> {
            mouse_area(content)
                .on_press(message)
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        };
        let (kind, detail): (El<'a>, El<'a>) = match step {
            Step::Press(code) | Step::Release(code) => {
                let mouse = (KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code()).contains(code);
                let name = if matches!(step, Step::Press(_)) { "Press" } else { "Release" };
                let kind = clickable(
                    container(t(l, name)).width(100).into(),
                    Message::ToggleKind(index),
                );
                let detail: El<'a> = match edit {
                    Some(edit) => {
                        let picker = combo_box(
                            &edit.keys,
                            "Search keys…",
                            edit.key.as_ref(),
                            Message::EditKey,
                        )
                        .size(l.size)
                        .font(l.font)
                        .padding([2, 6])
                        .width(220)
                        .input_style(style::text_input_style(c))
                        .menu_style(style::menu_style(c));
                        let capture = if edit.capture { "Listening…" } else { "Capture" };
                        row![
                            picker,
                            button(self.labelled(Icon::Keyboard, capture, true))
                                .padding([1, 8])
                                .on_press(Message::EditCapture)
                                .style(style::button_style(c, false)),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .into()
                    }
                    None => clickable(
                        row![
                            glyph(if mouse { Icon::Mouse } else { Icon::Keyboard }),
                            t(l, key_name(*code)),
                        ]
                        .spacing(8)
                        .into(),
                        Message::EditField(index, Field::Key),
                    ),
                };
                (kind, detail)
            }
            Step::Delay(us) => {
                let detail: El<'a> = match edit {
                    Some(edit) => row![
                        text_input("0", &edit.delay_text)
                            .id(iced::widget::Id::new(EDIT_ID))
                            .on_input(Message::EditDelayText)
                            .on_submit(Message::EndEdit)
                            .size(l.size)
                            .font(l.font)
                            .padding([2, 6])
                            .width(100)
                            .align_x(Alignment::End)
                            .style(style::text_input_style(c)),
                        t(l, "ms"),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into(),
                    None => clickable(
                        row![space().width(24), t(l, format!("{} ms", format_ms(*us)))].into(),
                        Message::EditField(index, Field::Delay),
                    ),
                };
                (container(t(l, "Delay")).width(100).into(), detail)
            }
        };
        let number = text(format!("{:03}", index + 1)).size(l.size).font(l.mono);
        let number = if selected { number } else { number.color(c.muted) };
        let line = container(
            row![number.width(44), kind, detail]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([0, 10])
        .height(ROW_HEIGHT)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |_| style::row(c, selected, hovered, index % 2 == 1));
        mouse_area(line)
            .on_press(Message::SelectRow(index))
            .on_double_click(Message::EditRow(index))
            .on_enter(Message::HoverRow(index))
            .on_exit(Message::LeaveRow(index))
            .into()
    }

    // ---------------------------------------------------------------- Playback

    fn form_row<'a>(&self, label: &'a str, control: impl Into<El<'a>>) -> El<'a> {
        let l = self.look;
        row![
            container(t(l, label)).width(LABEL_WIDTH).align_x(Alignment::End),
            control.into(),
        ]
        .spacing(14)
        .align_y(Alignment::Center)
        .into()
    }

    /// A hint aligned under a form control.
    fn form_hint<'a>(&self, hint: &'a str) -> El<'a> {
        let l = self.look;
        row![space().width(LABEL_WIDTH), small(l, hint)]
            .spacing(14)
            .into()
    }

    fn form_section<'a>(&self, title: &'a str) -> El<'a> {
        let l = self.look;
        row![
            container(text(title).size(l.size).font(bold(l)))
                .width(LABEL_WIDTH)
                .align_x(Alignment::End),
            container(hrule(l)).width(Length::Fill).align_y(Alignment::Center),
        ]
        .spacing(14)
        .align_y(Alignment::Center)
        .into()
    }

    fn number_field<'a>(
        &self,
        value: &'a str,
        enabled: bool,
        on_input: fn(String) -> Message,
        unit: &'a str,
    ) -> El<'a> {
        let l = self.look;
        let mut line = Row::new().spacing(6).align_y(Alignment::Center).push(
            text_input("", value)
                .on_input_maybe(enabled.then_some(on_input))
                .size(l.size)
                .font(l.font)
                .padding([4, 8])
                .width(64)
                .align_x(Alignment::End)
                .style(style::text_input_style(l.c)),
        );
        if !unit.is_empty() {
            line = line.push(muted(l, unit));
        }
        line.into()
    }

    fn playback_page(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let editable = !self.running() && !self.recording;
        let loop_slider = slider(
            0..=2000u32,
            self.settings.loop_delay_ms as u32,
            move |value| {
                if editable {
                    Message::SetLoopDelay(value)
                } else {
                    Message::Noop
                }
            },
        )
        .width(240)
        .style(style::slider_style(c, editable));
        let devices_enabled = editable && self.settings.parallel;
        let devices_slider = slider(
            1..=50u32,
            self.settings.devices as u32,
            move |value| {
                if devices_enabled {
                    Message::SetDevices(value)
                } else {
                    Message::Noop
                }
            },
        )
        .width(240)
        .style(style::slider_style(c, devices_enabled));
        let parallel = checkbox(self.settings.parallel)
            .label("Click with several virtual devices at once")
            .on_toggle_maybe(editable.then_some(Message::SetParallel))
            .size(18)
            .text_size(l.size)
            .font(l.font)
            .style(style::checkbox_style(c));
        let form = column![
            self.form_section("Timing"),
            self.form_row(
                "Loop delay:",
                row![
                    loop_slider,
                    self.number_field(&self.loop_text, editable, Message::LoopDelayText, "ms"),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            ),
            self.form_hint("Pause after each complete run of the sequence."),
            space().height(8),
            self.form_section("Devices"),
            self.form_row("Parallel mode:", parallel),
            self.form_row(
                "Devices:",
                row![
                    devices_slider,
                    self.number_field(&self.devices_text, devices_enabled, Message::DevicesText, ""),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            ),
            self.form_hint("Each virtual device plays the sequence independently."),
            space().height(8),
            self.form_section("Shortcut"),
            self.form_row("Start / stop key:", self.hotkey_button(self.settings.binding, false)),
            self.form_hint("For the shown macro. Set up more keys on the Profiles page."),
        ]
        .spacing(10)
        .max_width(640);
        column![
            self.page_header("Playback", "How every macro is played back".into()),
            space().height(8),
            form,
        ]
        .spacing(4)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // -------------------------------------------------------------------- Test

    fn card<'a>(&self, title: &'a str, action: Option<El<'a>>, body: El<'a>) -> El<'a> {
        let l = self.look;
        let c = l.c;
        let mut header = Row::new()
            .align_y(Alignment::Center)
            .push(text(title).size(l.size).font(bold(l)))
            .push(space().width(Length::Fill));
        if let Some(action) = action {
            header = header.push(action);
        }
        container(column![container(header).height(32).align_y(Alignment::Center), body].spacing(6))
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| style::card(c))
            .into()
    }

    fn test_page(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let now = Instant::now();
        let overruns = self.test_overruns();
        let key = key_name(self.settings.current().hotkey);
        let hint = if overruns > 0 {
            text(format!("{overruns} input overruns: CPS is a lower bound"))
                .size(l.size - 1.0)
                .font(l.font)
                .color(c.error)
        } else if self.in_pad {
            small(
                l,
                format!(
                    "{key} to start / stop. Measures device clicks. Move outside Clicker for desktop clicking."
                ),
            )
        } else {
            small(l, format!("Device input · {key} to start / stop"))
        };
        let text_test = self.card(
            "Text test",
            Some(self.tool_button(
                Icon::Reset,
                "Reset",
                Message::ResetLps,
                true,
                "Clear the text and reset the counters",
            )),
            column![
                row![
                    text(format!("{} LPS", self.lps.current))
                        .size(l.size * 1.8)
                        .font(bold(l)),
                    muted(
                        l,
                        format!(
                            "Peak {} · Total {} · Avg {:.1}",
                            self.lps.peak,
                            self.lps.total,
                            self.lps.average(now)
                        )
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                text_editor(&self.test_text)
                    .placeholder("Type to test the macro…")
                    .on_action(Message::TextTest)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .font(l.mono)
                    .size(l.size)
                    .padding(8)
                    .height(Length::Fill)
                    .style(style::text_editor_style(c)),
                small(l, "Letters per second, counted as characters typed into this field"),
            ]
            .spacing(8)
            .height(Length::Fill)
            .into(),
        );
        let pad_hovered = self.in_pad;
        let pad = mouse_area(
            container(text("Click to test").size(l.size).font(l.font))
                .center(Length::Fill)
                .style(move |_| {
                    let mut s = style::view_area(c);
                    s.background = Some(c.button.into());
                    s.text_color = Some(c.button_text);
                    if pad_hovered {
                        s.border.color = c.hover;
                    }
                    s
                }),
        )
        .on_enter(Message::PadEntered)
        .on_exit(Message::PadExited)
        .on_press(Message::PadPress)
        .on_right_press(Message::PadPress)
        .on_middle_press(Message::PadPress);
        let click_test = self.card(
            "Click test",
            Some(self.tool_button(
                Icon::Reset,
                "Reset",
                Message::ResetCps,
                true,
                "Reset the counters",
            )),
            column![
                row![
                    text(format!("{} CPS", self.cps.current))
                        .size(l.size * 1.8)
                        .font(bold(l)),
                    muted(
                        l,
                        format!(
                            "Peak {} · Total {} · Avg {:.1}",
                            self.cps.peak,
                            self.cps.total,
                            self.cps.average(now)
                        )
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                container(pad).height(Length::Fill),
                hint,
            ]
            .spacing(8)
            .height(Length::Fill)
            .into(),
        );
        column![
            self.page_header("Test", "Check what your macro does, and how fast".into()),
            row![text_test, click_test].spacing(12).height(Length::Fill),
        ]
        .spacing(10)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // --------------------------------------------------------------- Status bar

    fn status_bar(&self) -> El<'_> {
        let l = self.look;
        let c = l.c;
        let running: Vec<_> = self.engines.iter().filter(|engine| engine.running()).collect();
        let (status, tone) = if self.recording {
            ("Recording", c.error)
        } else if running.is_empty() {
            ("Idle", c.muted)
        } else if running.iter().all(|engine| engine.state.cancel.is_stopped()) {
            ("Stopping", c.neutral)
        } else if running.iter().any(|engine| {
            engine.state.ready.load(Ordering::Acquire) && !engine.state.cancel.is_stopped()
        }) {
            ("Running", c.positive)
        } else {
            ("Starting", c.neutral)
        };
        let keys = self
            .settings
            .active()
            .bindings
            .iter()
            .map(|binding| key_name(binding.hotkey))
            .collect::<Vec<_>>()
            .join(", ");
        let devices = self.input.status();
        let loops = self.loops();
        let detail = format!(
            "{} input devices, {} inaccessible · Wayland",
            devices.connected, devices.denied
        );
        container(
            row![
                text(status).size(l.size - 1.0).font(bold(l)).color(tone),
                small(l, format!("{} · {keys}", self.settings.active().name)),
                space().width(Length::Fill),
                small(l, format!("{loops} loops")),
                vsep(l),
                small(l, format!("{} input devices", devices.connected)),
                vrule(l),
                self.tip(small(l, self.theme_source()), detail),
            ]
            .spacing(10)
            .height(18)
            .align_y(Alignment::Center),
        )
        .padding([4, 12])
        .width(Length::Fill)
        .style(move |_| style::status_bar(c))
        .into()
    }

    fn theme_source(&self) -> String {
        self.appearance.source.clone()
    }
}
