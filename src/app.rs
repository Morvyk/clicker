mod view;

use std::{
    collections::VecDeque,
    fmt,
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime},
};

use evdev::KeyCode;
use iced::{
    Event, Subscription, Task, Theme, event, keyboard, mouse, time, window,
    theme::{self, Palette},
    widget::{combo_box, operation, scrollable, text_editor},
};

use crate::{
    engine::Engine,
    icons::Icons,
    input::{self, InputMonitor},
    model::{self, Recorder, Settings, Step, format_ms, key_name},
    theme::{Appearance, Look, system_appearance},
};

/// Fixed row height, which lets the sequence list render only visible rows.
const ROW_HEIGHT: f32 = 28.0;
const STEPS_ID: &str = "sequence_steps";
const EDIT_ID: &str = "edit_delay";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Sequence,
    Playback,
    Test,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyChoice {
    code: u16,
    name: String,
}

impl fmt::Display for KeyChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Which part of a row is being edited in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Key,
    Delay,
}

/// In-place edit of one cell. Changes apply to the step as they are made.
struct Edit {
    index: usize,
    delay_text: String,
    keys: combo_box::State<KeyChoice>,
    key: Option<KeyChoice>,
    capture: bool,
}

#[derive(Clone, Debug)]
pub enum Message {
    Tick,
    Noop,
    Close,
    Navigate(Page),
    // Desktop integration.
    PollAppearance,
    Appearance(Box<Appearance>),
    IconsLoaded(Icons),
    SystemTheme(theme::Mode),
    WindowFocus(bool),
    CursorInWindow(bool),
    Escape,
    Enter,
    // Playback.
    Start,
    Stop,
    SetParallel(bool),
    SetLoopDelay(u32),
    LoopDelayText(String),
    SetDevices(u32),
    DevicesText(String),
    SelectHotkey,
    // Sequence.
    Record,
    Clear,
    SelectRow(usize),
    EditRow(usize),
    HoverRow(usize),
    LeaveRow(usize),
    Scrolled(f32),
    RecordAreaEntered,
    RecordAreaExited,
    DismissError,
    // Step dialog.
    EditField(usize, Field),
    ToggleKind(usize),
    EditKey(KeyChoice),
    EditDelayText(String),
    EditCapture,
    EndEdit,
    // Tests.
    TextTest(text_editor::Action),
    ResetLps,
    PadEntered,
    PadExited,
    PadPress,
    ResetCps,
}

#[derive(Default)]
struct CpsMeter {
    batches: VecDeque<(Instant, u64)>,
    current: u64,
    first: Option<Instant>,
    total: u64,
    peak: u64,
}

impl CpsMeter {
    fn prune(&mut self, now: Instant) {
        while self
            .batches
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= Duration::from_secs(1))
        {
            if let Some((_, count)) = self.batches.pop_front() {
                self.current -= count;
            }
        }
    }

    fn record(&mut self, count: u64, now: Instant) {
        self.prune(now);
        if count == 0 {
            return;
        }
        self.first.get_or_insert(now);
        self.batches.push_back((now, count));
        self.current += count;
        self.total += count;
        self.peak = self.peak.max(self.current);
    }

    fn average(&self, now: Instant) -> f64 {
        self.first.map_or(0.0, |first| {
            self.total as f64 / now.duration_since(first).as_secs_f64().max(1.0)
        })
    }
}

pub struct ClickerApp {
    settings: Settings,
    engine: Engine,
    input: InputMonitor,
    page: Page,
    recording: bool,
    recorder: Recorder,
    record_started: Duration,
    selecting_hotkey: bool,
    editing: Option<Edit>,
    selected: Option<usize>,
    hovered_row: Option<usize>,
    scroll_y: f32,
    test_text: text_editor::Content,
    cps: CpsMeter,
    lps: CpsMeter,
    diagnostics: Option<Instant>,
    error: Option<String>,
    key_choices: Vec<KeyChoice>,
    cps_observed: u64,
    loop_text: String,
    devices_text: String,
    // Pointer and focus tracking replaces per-frame immediate-mode input.
    window_focused: bool,
    cursor_in_window: bool,
    in_record_area: bool,
    in_pad: bool,
    // Desktop appearance.
    appearance: Appearance,
    system_dark: bool,
    look: Look,
    theme: Theme,
    icons: Icons,
    icon_key: Option<(String, [u8; 4], [u8; 4])>,
    polling: bool,
    // Persistence.
    dirty: bool,
    last_save: Instant,
}

impl ClickerApp {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self::from_settings(Settings::load());
        let startup = Task::batch([
            iced::system::theme().map(Message::SystemTheme),
            app.poll_appearance(),
        ]);
        (app, startup)
    }

    fn from_settings(mut settings: Settings) -> Self {
        settings.sanitize();
        let engine = Engine::default();
        let input = InputMonitor::new(settings.hotkey, engine.state.clone());
        let key_choices = (1..=model::MAX_KEY)
            .filter(|code| {
                let name = format!("{:?}", KeyCode::new(*code));
                name.starts_with("KEY_") || name.starts_with("BTN_")
            })
            .map(|code| KeyChoice {
                code,
                name: key_name(code),
            })
            .collect();
        let appearance = Appearance::default();
        let look = Look::new(&appearance, true);
        Self {
            loop_text: settings.loop_delay_ms.to_string(),
            devices_text: settings.devices.to_string(),
            settings,
            engine,
            input,
            page: Page::Sequence,
            recording: false,
            recorder: Recorder::default(),
            record_started: Duration::ZERO,
            selecting_hotkey: false,
            editing: None,
            selected: None,
            hovered_row: None,
            scroll_y: 0.0,
            test_text: text_editor::Content::new(),
            cps: CpsMeter::default(),
            lps: CpsMeter::default(),
            diagnostics: std::env::var_os("CLICKER_DIAGNOSTICS").map(|_| Instant::now()),
            error: None,
            key_choices,
            cps_observed: 0,
            window_focused: true,
            cursor_in_window: false,
            in_record_area: false,
            in_pad: false,
            theme: build_theme(&look),
            look,
            appearance,
            system_dark: true,
            icons: Icons::default(),
            icon_key: None,
            polling: false,
            dirty: false,
            last_save: Instant::now(),
        }
    }

    pub fn title(&self) -> String {
        "Clicker".into()
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub fn style(&self, _theme: &Theme) -> theme::Style {
        theme::Style {
            background_color: self.look.c.window,
            text_color: self.look.c.text,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let busy = self.recording || self.engine.running() || self.page == Page::Test;
        let poll = Duration::from_millis(if busy { 16 } else { 50 });
        Subscription::batch([
            time::every(poll).map(|_| Message::Tick),
            time::every(Duration::from_secs(2)).map(|_| Message::PollAppearance),
            window::close_requests().map(|_| Message::Close),
            iced::system::theme_changes().map(Message::SystemTheme),
            event::listen_with(|event, status, _| match event {
                Event::Window(window::Event::Focused) => Some(Message::WindowFocus(true)),
                Event::Window(window::Event::Unfocused) => Some(Message::WindowFocus(false)),
                Event::Mouse(mouse::Event::CursorEntered) => Some(Message::CursorInWindow(true)),
                Event::Mouse(mouse::Event::CursorLeft) => Some(Message::CursorInWindow(false)),
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                    if status == event::Status::Ignored =>
                {
                    match key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            Some(Message::Escape)
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::Enter),
                        _ => None,
                    }
                }
                _ => None,
            }),
        ])
    }

    fn poll_appearance(&self) -> Task<Message> {
        Task::perform(async { system_appearance() }, |appearance| {
            Message::Appearance(Box::new(appearance))
        })
    }

    fn apply_appearance(&mut self, appearance: Appearance) -> Task<Message> {
        self.appearance = appearance;
        self.look = Look::new(&self.appearance, self.system_dark);
        self.theme = build_theme(&self.look);
        let c = self.look.c;
        let key = (
            self.appearance.icon_theme.clone(),
            c.text.into_rgba8(),
            c.selection_text.into_rgba8(),
        );
        if self.icon_key.as_ref() == Some(&key) {
            return Task::none();
        }
        self.icon_key = Some(key.clone());
        let (theme, text, on_accent) = (key.0, c.text, c.selection_text);
        Task::perform(
            async move { Icons::load(&theme, text, on_accent) },
            Message::IconsLoaded,
        )
    }

    fn start(&mut self) {
        if self.recording || self.selecting_hotkey {
            return;
        }
        self.editing = None;
        self.error = self.engine.start(&self.settings).err();
    }

    fn start_recording(&mut self) {
        self.settings.steps.clear();
        self.recorder = Recorder::default();
        self.record_started = timestamp();
        self.recording = true;
        self.selected = None;
        self.scroll_y = 0.0;
        self.error = None;
        self.sync_input_mode();
    }

    fn finish_recording(&mut self) {
        self.recording = false;
        self.recorder.finish(&mut self.settings.steps, timestamp());
        self.sync_input_mode();
    }

    fn sync_input_mode(&self) {
        let mode = if self.selecting_hotkey {
            input::SELECT_HOTKEY
        } else if self.editing.as_ref().is_some_and(|edit| edit.capture) {
            input::SELECT_STEP
        } else if self.recording {
            input::RECORDING
        } else {
            input::IDLE
        };
        self.input.state.mode.store(mode, Ordering::Release);
    }

    /// True while Clicker itself may change the sequence or settings.
    fn idle(&self) -> bool {
        !self.recording && !self.engine.running() && !self.selecting_hotkey
    }

    /// Starts editing a cell in place; returns the focus task for text cells.
    fn begin_edit(&mut self, index: usize, _requested: Field) -> Task<Message> {
        let Some(step) = self.settings.steps.get(index) else {
            return Task::none();
        };
        let (delay_edit, code, delay_text) = match step {
            Step::Delay(us) => (true, None, format_ms(*us)),
            Step::Press(code) | Step::Release(code) => (false, Some(*code), String::new()),
        };
        self.selected = Some(index);
        self.editing = Some(Edit {
            index,
            delay_text,
            keys: combo_box::State::new(self.key_choices.clone()),
            key: code.map(|code| KeyChoice {
                code,
                name: key_name(code),
            }),
            capture: false,
        });
        self.sync_input_mode();
        if delay_edit {
            operation::focus(iced::widget::Id::new(EDIT_ID))
        } else {
            Task::none()
        }
    }

    fn end_edit(&mut self) {
        self.editing = None;
        self.sync_input_mode();
    }

    fn receive_input(&mut self) {
        self.engine
            .state
            .gui_hovered
            .store(self.cursor_in_window, Ordering::Relaxed);
        let testing = self.in_pad && self.page == Page::Test;
        self.engine.state.testing.store(testing, Ordering::Relaxed);
        let observed = self.engine.state.test_clicks.load(Ordering::Relaxed);
        self.cps
            .record(observed.saturating_sub(self.cps_observed), Instant::now());
        self.cps_observed = observed;
        let currently_in_area =
            self.window_focused && self.in_record_area && self.page == Page::Sequence;
        self.input.state.in_area.store(
            self.recording && currently_in_area,
            Ordering::Release,
        );
        self.engine.reap();
        if let Some(error) = self.engine.take_error() {
            self.error = Some(error);
        }
        while let Ok(message) = self.input.messages.try_recv() {
            match message {
                input::Message::Toggle { stop_only } => {
                    if stop_only || self.engine.running() {
                        self.engine.stop();
                    } else {
                        self.start();
                    }
                }
                input::Message::Key {
                    code,
                    pressed,
                    at,
                    mode,
                    in_area,
                } => {
                    if mode == input::SELECT_HOTKEY
                        && self.selecting_hotkey
                        && pressed
                        && model::is_keyboard(code)
                    {
                        self.settings.hotkey = code;
                        self.input.state.hotkey.store(code, Ordering::Relaxed);
                        self.selecting_hotkey = false;
                        self.dirty = true;
                        self.sync_input_mode();
                    } else if mode == input::SELECT_STEP && pressed {
                        if let Some(edit) = self.editing.take_if(|edit| edit.capture)
                            && let Some(step) = self.settings.steps.get_mut(edit.index)
                        {
                            *step = match step {
                                Step::Release(_) => Step::Release(code),
                                _ => Step::Press(code),
                            };
                            self.dirty = true;
                            self.sync_input_mode();
                        }
                    } else if mode == input::RECORDING
                        && self.recording
                        && at >= self.record_started
                        && ((in_area && currently_in_area)
                            || (!pressed && self.recorder.holds(code)))
                    {
                        self.recorder
                            .record(&mut self.settings.steps, code, pressed, at);
                        self.dirty = true;
                        if self.settings.steps.len() >= 100_000 {
                            self.finish_recording();
                            self.error =
                                Some("Recording reached 100,000 steps and was stopped.".into());
                        }
                    }
                }
            }
        }
        if self.input.state.overflow.swap(false, Ordering::AcqRel) {
            if self.recording {
                self.finish_recording();
            }
            self.error = Some(
                "Input queue filled. Recording stopped; please record a shorter sequence.".into(),
            );
        }
    }

    fn tick(&mut self) -> Task<Message> {
        let steps_before = self.settings.steps.len();
        self.receive_input();
        let now = Instant::now();
        self.cps.prune(now);
        self.lps.prune(now);
        if self.dirty && self.last_save.elapsed() >= Duration::from_secs(1) {
            self.save();
        }
        if let Some(last) = &mut self.diagnostics
            && last.elapsed() >= Duration::from_secs(1)
        {
            eprintln!(
                "clicker: received_clicks={} cps={} completed_loops={}",
                self.cps.total,
                self.cps.current,
                self.engine.state.loops.load(Ordering::Relaxed)
            );
            *last = Instant::now();
        }
        // Follow the newest step while recording.
        if self.recording && self.settings.steps.len() != steps_before {
            let end = self.settings.steps.len() as f32 * ROW_HEIGHT;
            self.scroll_y = end;
            return operation::scroll_to(
                iced::widget::Id::new(STEPS_ID),
                scrollable::AbsoluteOffset { x: 0.0, y: end },
            );
        }
        Task::none()
    }

    /// Settings as they should be stored. A recording in progress is balanced
    /// by releasing every key it still holds.
    fn snapshot(&self) -> Settings {
        let mut settings = self.settings.clone();
        if self.recording {
            let mut held = std::collections::BTreeSet::new();
            for step in &settings.steps {
                match step {
                    Step::Press(code) => {
                        held.insert(*code);
                    }
                    Step::Release(code) => {
                        held.remove(code);
                    }
                    _ => {}
                }
            }
            settings.steps.extend(held.into_iter().map(Step::Release));
        }
        settings
    }

    fn save(&mut self) {
        self.dirty = false;
        if cfg!(test) {
            return;
        }
        self.last_save = Instant::now();
        if let Err(error) = self.snapshot().save() {
            eprintln!("clicker: cannot save settings: {error}");
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => return self.tick(),
            Message::Noop => {}
            Message::Close => {
                self.engine.stop();
                self.save();
                return iced::exit();
            }
            Message::Navigate(page) => {
                self.page = page;
                self.in_pad = false;
                self.in_record_area = false;
                self.hovered_row = None;
            }
            Message::PollAppearance => {
                if !self.polling {
                    self.polling = true;
                    return self.poll_appearance();
                }
            }
            Message::Appearance(appearance) => {
                self.polling = false;
                if *appearance != self.appearance {
                    return self.apply_appearance(*appearance);
                }
            }
            Message::IconsLoaded(icons) => self.icons = icons,
            Message::SystemTheme(mode) => {
                self.system_dark = mode != theme::Mode::Light;
                let appearance = self.appearance.clone();
                return self.apply_appearance(appearance);
            }
            Message::WindowFocus(focused) => self.window_focused = focused,
            Message::CursorInWindow(inside) => {
                self.cursor_in_window = inside;
                if !inside {
                    self.in_pad = false;
                    self.in_record_area = false;
                }
            }
            Message::Escape | Message::Enter => self.end_edit(),
            Message::Start => self.start(),
            Message::Stop => self.engine.stop(),
            Message::SetParallel(parallel) => {
                self.settings.parallel = parallel;
                self.dirty = true;
            }
            Message::SetLoopDelay(ms) => {
                self.settings.loop_delay_ms = u64::from(ms);
                self.loop_text = ms.to_string();
                self.dirty = true;
            }
            Message::LoopDelayText(text) => {
                if let Ok(ms) = text.trim().parse::<u64>() {
                    self.settings.loop_delay_ms = ms.min(2000);
                    self.dirty = true;
                }
                self.loop_text = text;
            }
            Message::SetDevices(count) => {
                self.settings.devices = count as usize;
                self.devices_text = count.to_string();
                self.dirty = true;
            }
            Message::DevicesText(text) => {
                if let Ok(count) = text.trim().parse::<usize>() {
                    self.settings.devices = count.clamp(1, 50);
                    self.dirty = true;
                }
                self.devices_text = text;
            }
            Message::SelectHotkey => {
                self.selecting_hotkey = !self.selecting_hotkey;
                self.sync_input_mode();
            }
            Message::Record => {
                if self.recording {
                    self.finish_recording();
                } else if self.idle() {
                    self.start_recording();
                }
                self.dirty = true;
            }
            Message::Clear => {
                self.settings.steps.clear();
                self.selected = None;
                self.scroll_y = 0.0;
                self.dirty = true;
            }
            Message::SelectRow(index) => {
                self.selected = Some(index);
                if self.editing.as_ref().is_some_and(|edit| edit.index != index) {
                    self.end_edit();
                }
            }
            Message::EditRow(index) => {
                if self.idle() {
                    return self.begin_edit(index, Field::Key);
                }
            }
            Message::EditField(index, field) => {
                if self.idle() {
                    return self.begin_edit(index, field);
                }
            }
            Message::ToggleKind(index) => {
                if self.idle()
                    && let Some(step) = self.settings.steps.get_mut(index)
                {
                    *step = match step {
                        Step::Press(code) => Step::Release(*code),
                        Step::Release(code) => Step::Press(*code),
                        Step::Delay(us) => Step::Delay(*us),
                    };
                    self.selected = Some(index);
                    self.dirty = true;
                }
            }
            Message::HoverRow(index) => self.hovered_row = Some(index),
            Message::LeaveRow(index) => {
                if self.hovered_row == Some(index) {
                    self.hovered_row = None;
                }
            }
            Message::Scrolled(y) => self.scroll_y = y,
            Message::RecordAreaEntered => self.in_record_area = true,
            Message::RecordAreaExited => self.in_record_area = false,
            Message::DismissError => self.error = None,
            Message::EditKey(choice) => {
                if let Some(edit) = self.editing.take()
                    && let Some(step) = self.settings.steps.get_mut(edit.index)
                {
                    *step = match step {
                        Step::Release(_) => Step::Release(choice.code),
                        _ => Step::Press(choice.code),
                    };
                    self.dirty = true;
                }
                self.sync_input_mode();
            }
            Message::EditDelayText(text) => {
                if let Some(edit) = &mut self.editing {
                    if let Ok(ms) = text.trim().parse::<f64>()
                        && ms.is_finite()
                        && (0.0..=3_600_000.0).contains(&ms)
                        && let Some(step) = self.settings.steps.get_mut(edit.index)
                    {
                        *step = Step::Delay((ms * 1000.0).round() as u64);
                        self.dirty = true;
                    }
                    edit.delay_text = text;
                }
            }
            Message::EditCapture => {
                if let Some(edit) = &mut self.editing {
                    edit.capture = !edit.capture;
                }
                self.sync_input_mode();
            }
            Message::EndEdit => self.end_edit(),
            Message::TextTest(action) => {
                let typed = match &action {
                    text_editor::Action::Edit(text_editor::Edit::Insert(_))
                    | text_editor::Action::Edit(text_editor::Edit::Enter) => 1,
                    text_editor::Action::Edit(text_editor::Edit::Paste(text)) => {
                        text.chars().count() as u64
                    }
                    _ => 0,
                };
                self.lps.record(typed, Instant::now());
                self.test_text.perform(action);
            }
            Message::ResetLps => {
                self.lps = CpsMeter::default();
                self.test_text = text_editor::Content::new();
            }
            Message::PadEntered => self.in_pad = true,
            Message::PadExited => self.in_pad = false,
            Message::PadPress => self.cps.record(1, Instant::now()),
            Message::ResetCps => {
                self.cps = CpsMeter::default();
                self.cps_observed = self.engine.state.test_clicks.load(Ordering::Relaxed);
                self.engine.state.test_overruns.store(0, Ordering::Relaxed);
            }
        }
        Task::none()
    }
}

impl Drop for ClickerApp {
    fn drop(&mut self) {
        self.engine.stop();
        if self.dirty {
            self.save();
        }
    }
}

fn build_theme(look: &Look) -> Theme {
    let c = look.c;
    Theme::custom(
        "Desktop",
        Palette {
            background: c.window,
            text: c.text,
            primary: c.focus,
            success: c.positive,
            warning: c.neutral,
            danger: c.error,
        },
    )
}

fn timestamp() -> Duration {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cps_counts_bursts_and_expires_after_one_second() {
        let now = Instant::now();
        let mut meter = CpsMeter::default();
        meter.record(1000, now);
        assert_eq!(meter.current, 1000);
        assert_eq!(meter.batches.len(), 1);
        assert_eq!(meter.peak, 1000);
        meter.prune(now + Duration::from_secs(1));
        assert_eq!(meter.current, 0);
        assert_eq!(meter.total, 1000);
        assert_eq!(meter.average(now + Duration::from_secs(2)), 500.0);
    }

    #[test]
    fn pad_presses_are_counted() {
        let mut app = ClickerApp::from_settings(Settings::default());
        for _ in 0..25 {
            let _ = app.update(Message::PadPress);
        }
        assert_eq!(app.cps.total, 25);
        let _ = app.update(Message::ResetCps);
        assert_eq!(app.cps.total, 0);
    }

    #[test]
    fn pad_hover_only_counts_as_testing_on_the_test_page() {
        let mut app = ClickerApp::from_settings(Settings::default());
        let _ = app.update(Message::PadEntered);
        let _ = app.update(Message::Tick);
        assert!(!app.engine.state.testing.load(Ordering::Relaxed));
        let _ = app.update(Message::Navigate(Page::Test));
        let _ = app.update(Message::PadEntered);
        let _ = app.update(Message::Tick);
        assert!(app.engine.state.testing.load(Ordering::Relaxed));
        let _ = app.update(Message::Navigate(Page::Sequence));
        let _ = app.update(Message::Tick);
        assert!(!app.engine.state.testing.load(Ordering::Relaxed));
    }

    #[test]
    fn cells_edit_in_place_and_ignore_invalid_delays() {
        let mut app = ClickerApp::from_settings(Settings::default());
        let _ = app.update(Message::EditField(1, Field::Delay));
        let _ = app.update(Message::EditDelayText("1.5".into()));
        assert_eq!(app.settings.steps[1], Step::Delay(1500));
        let _ = app.update(Message::EditDelayText("abc".into()));
        assert_eq!(app.settings.steps[1], Step::Delay(1500), "keeps the last valid delay");
        let _ = app.update(Message::EndEdit);
        assert!(app.editing.is_none());
        let _ = app.update(Message::ToggleKind(0));
        assert!(matches!(app.settings.steps[0], Step::Release(_)));
        let key = KeyChoice { code: 30, name: "A".into() };
        let _ = app.update(Message::EditField(2, Field::Key));
        let _ = app.update(Message::EditKey(key));
        assert_eq!(app.settings.steps[2], Step::Release(30));
        assert!(app.editing.is_none());
    }

    #[test]
    fn every_page_and_the_dialog_build_a_view() {
        let mut app = ClickerApp::from_settings(Settings::default());
        for page in [Page::Sequence, Page::Playback, Page::Test] {
            let _ = app.update(Message::Navigate(page));
            let _ = app.view();
        }
        let _ = app.update(Message::EditField(0, Field::Key));
        let _ = app.view();
    }
}
