use std::{collections::BTreeSet, time::Duration};

use evdev::KeyCode;
use serde::{Deserialize, Serialize};

pub const MAX_KEY: u16 = 0x2ff;
pub const MAX_DELAY_US: u64 = 3_600_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Press(u16),
    Release(u16),
    Delay(u64), // Microseconds preserve sub-millisecond recorded timing.
}

pub fn format_ms(us: u64) -> String {
    if us.is_multiple_of(1000) {
        (us / 1000).to_string()
    } else {
        format!("{:.3}", us as f64 / 1000.0)
            .trim_end_matches('0')
            .to_owned()
    }
}

pub fn key_name(code: u16) -> String {
    match KeyCode::new(code) {
        KeyCode::BTN_LEFT => "Mouse Left".into(),
        KeyCode::BTN_RIGHT => "Mouse Right".into(),
        KeyCode::BTN_MIDDLE => "Mouse Middle".into(),
        KeyCode::BTN_SIDE => "Mouse Side".into(),
        KeyCode::BTN_EXTRA => "Mouse Extra".into(),
        KeyCode::BTN_FORWARD => "Mouse Forward".into(),
        KeyCode::BTN_BACK => "Mouse Back".into(),
        KeyCode::BTN_TASK => "Mouse Task".into(),
        key => {
            let name = format!("{key:?}");
            name.strip_prefix("KEY_").unwrap_or(&name).replace('_', " ")
        }
    }
}

pub fn is_keyboard(code: u16) -> bool {
    format!("{:?}", KeyCode::new(code)).starts_with("KEY_") && code != 0
}

pub fn click_sequence() -> Vec<Step> {
    vec![
        Step::Press(KeyCode::BTN_LEFT.code()),
        Step::Delay(20_000),
        Step::Release(KeyCode::BTN_LEFT.code()),
    ]
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub loop_delay_ms: u64,
    pub parallel: bool,
    pub devices: usize,
    pub hotkey: u16,
    pub steps: Vec<Step>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            loop_delay_ms: 20,
            parallel: false,
            devices: 5,
            hotkey: KeyCode::KEY_F6.code(),
            steps: click_sequence(),
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.loop_delay_ms = self.loop_delay_ms.min(2000);
        self.devices = self.devices.clamp(1, 50);
        if !is_keyboard(self.hotkey) {
            self.hotkey = KeyCode::KEY_F6.code();
        }
    }

    /// `$XDG_CONFIG_HOME/rust-clicker/settings.json`.
    fn path() -> Option<std::path::PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
            })?;
        Some(config.join("rust-clicker").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes atomically so a crash never leaves a truncated settings file.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec(self)?)?;
        std::fs::rename(temporary, path)
    }

    pub fn device_count(&self) -> usize {
        if self.parallel { self.devices } else { 1 }
    }
}

pub fn validate(steps: &[Step]) -> Result<(), String> {
    let mut held = BTreeSet::new();
    let mut has_input = false;
    for (index, step) in steps.iter().enumerate() {
        let fail = |message: String| Err(format!("Step {}: {message}", index + 1));
        match *step {
            Step::Press(code) | Step::Release(code) if code == 0 || code > MAX_KEY => {
                return fail("invalid Linux input code".into());
            }
            Step::Press(code) => {
                has_input = true;
                if !held.insert(code) {
                    return fail(format!("{} is already pressed", key_name(code)));
                }
            }
            Step::Release(code) => {
                if !held.remove(&code) {
                    return fail(format!(
                        "{} needs a Press before its Release",
                        key_name(code)
                    ));
                }
            }
            Step::Delay(us) if us > MAX_DELAY_US => {
                return fail("delay must be at most 3,600,000 ms".into());
            }
            Step::Delay(_) => {}
        }
    }
    if !has_input {
        return Err("Record a sequence or load the left-click preset first.".into());
    }
    if !held.is_empty() {
        return Err(format!(
            "Add a Release for: {}",
            held.into_iter()
                .map(key_name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

#[derive(Default)]
pub struct Recorder {
    last: Option<Duration>,
    held: BTreeSet<u16>,
}

impl Recorder {
    pub fn record(&mut self, steps: &mut Vec<Step>, code: u16, pressed: bool, at: Duration) {
        // Discard auto-repeat and releases for presses outside the recording area.
        let changed = if pressed {
            self.held.insert(code)
        } else {
            self.held.remove(&code)
        };
        if !changed {
            return;
        }
        self.delay(steps, at);
        steps.push(if pressed {
            Step::Press(code)
        } else {
            Step::Release(code)
        });
    }

    fn delay(&mut self, steps: &mut Vec<Step>, at: Duration) {
        if let Some(last) = self.last {
            let us = at
                .saturating_sub(last)
                .as_micros()
                .min(MAX_DELAY_US as u128) as u64;
            // Keep zero delays too: each pair of raw transitions has an explicit delay.
            steps.push(Step::Delay(us));
        }
        self.last = Some(at);
    }

    pub fn holds(&self, code: u16) -> bool {
        self.held.contains(&code)
    }

    pub fn finish(&mut self, steps: &mut Vec<Step>, at: Duration) {
        if !self.held.is_empty() {
            self.delay(steps, at);
            steps.extend(self.held.iter().copied().map(Step::Release));
        }
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_chords_exact_timing_and_ignores_repeat_and_orphan_release() {
        let mut recorder = Recorder::default();
        let mut steps = Vec::new();
        for (code, pressed, us) in [
            (18, false, 100),
            (18, true, 1000),
            (18, true, 1500),
            (34, true, 21_000),
            (18, false, 21_450),
            (34, false, 61_450),
        ] {
            recorder.record(&mut steps, code, pressed, Duration::from_micros(us));
        }
        assert_eq!(
            steps,
            vec![
                Step::Press(18),
                Step::Delay(20_000),
                Step::Press(34),
                Step::Delay(450),
                Step::Release(18),
                Step::Delay(40_000),
                Step::Release(34),
            ]
        );
        assert!(validate(&steps).is_ok());
    }

    #[test]
    fn ending_a_recording_balances_held_keys() {
        let mut recorder = Recorder::default();
        let mut steps = Vec::new();
        recorder.record(&mut steps, 18, true, Duration::from_millis(10));
        recorder.finish(&mut steps, Duration::from_millis(35));
        assert_eq!(
            steps,
            vec![Step::Press(18), Step::Delay(25_000), Step::Release(18)]
        );
        assert!(validate(&steps).is_ok());
    }

    #[test]
    fn rejects_stuck_keys_and_invalid_edits() {
        assert!(validate(&[Step::Press(18)]).is_err());
        assert!(validate(&[Step::Release(18)]).is_err());
        assert!(validate(&[Step::Delay(20)]).is_err());
        assert!(validate(&[Step::Press(18), Step::Press(18), Step::Release(18)]).is_err());
        assert!(validate(&click_sequence()).is_ok());
    }
}
