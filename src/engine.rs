use std::{
    collections::BTreeSet,
    io,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use evdev::{
    AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode, uinput::VirtualDevice,
};

use crate::model::{Settings, Step, validate};

pub const DEVICE_PREFIX: &str = "RustClicker virtual";

/// How long udev and the compositor need to discover a new virtual device
/// before its input is delivered.
const SETTLE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct Cancellation {
    stopped: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
}

impl Cancellation {
    pub fn stop(&self) {
        let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn reset(&self) {
        let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.store(false, Ordering::Release);
    }

    /// Returns true if interrupted. The condvar makes even hour-long delays stoppable.
    fn wait(&self, duration: Duration) -> bool {
        let gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped() {
            return true;
        }
        let (_gate, _) = self
            .wake
            .wait_timeout_while(gate, duration, |_| !self.is_stopped())
            .unwrap_or_else(|e| e.into_inner());
        self.is_stopped()
    }
}

/// A validated macro and how to play it.
#[derive(Clone, PartialEq)]
struct Job {
    steps: Arc<[Step]>,
    delay: Duration,
    devices: usize,
}

/// Requests from the app and the hotkey thread to the rig thread.
#[derive(Default)]
struct Control {
    /// The macro a trigger plays; None while the macro is invalid.
    job: Option<Job>,
    /// The job changed, so its devices may need to be rebuilt.
    changed: bool,
    start: bool,
    quit: bool,
}

#[derive(Default)]
pub struct PlaybackState {
    pub cancel: Cancellation,
    pub running: AtomicBool,
    pub ready: AtomicBool,
    pub loops: AtomicU64,
    pub testing: AtomicBool,
    pub gui_hovered: AtomicBool,
    pub test_clicks: AtomicU64,
    pub test_overruns: AtomicU64,
    error: Mutex<Option<String>>,
    control: Mutex<Control>,
    wake: Condvar,
}

impl PlaybackState {
    fn fail(&self, message: String) {
        let mut error = self.error.lock().unwrap_or_else(|e| e.into_inner());
        if error.is_none() {
            *error = Some(message);
        }
        self.cancel.stop();
    }

    /// Starts the prepared macro from any thread, without waiting for the
    /// interface. Returns false when nothing was started: no valid macro is
    /// prepared, or it is already running.
    pub fn trigger(&self) -> bool {
        let mut control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        if control.job.is_none() || control.quit || self.running.load(Ordering::Acquire) {
            return false;
        }
        self.cancel.reset();
        self.loops.store(0, Ordering::Relaxed);
        self.ready.store(false, Ordering::Release);
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.running.store(true, Ordering::Release);
        control.start = true;
        self.wake.notify_all();
        true
    }
}

/// Plays one macro. Its virtual devices are created ahead of time and kept
/// between runs, so a start does not wait for new devices to be discovered.
#[derive(Default)]
pub struct Engine {
    pub state: Arc<PlaybackState>,
    rig: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn running(&self) -> bool {
        self.state.running.load(Ordering::Acquire)
    }

    /// Prepares the macro that `start` and `PlaybackState::trigger` play, and
    /// creates its devices in the background if they do not fit it yet.
    pub fn prepare(&mut self, settings: &Settings, steps: &[Step]) -> Result<(), String> {
        let valid = validate(steps);
        let mut control = self.state.control.lock().unwrap_or_else(|e| e.into_inner());
        let delay = Duration::from_millis(settings.loop_delay_ms);
        let devices = settings.device_count();
        let unchanged = control.job.as_ref().is_some_and(|job| {
            job.delay == delay && job.devices == devices && *job.steps == *steps
        });
        if valid.is_err() {
            control.job = None;
        } else if !unchanged {
            control.job = Some(Job {
                steps: steps.into(),
                delay,
                devices,
            });
            control.changed = true;
            self.state.wake.notify_all();
        }
        drop(control);
        valid?;
        if self.rig.is_none() {
            let state = self.state.clone();
            let rig = thread::Builder::new()
                .name("playback".into())
                .spawn(move || rig(&state))
                .map_err(|error| format!("Cannot start playback thread: {error}"))?;
            self.rig = Some(rig);
        }
        Ok(())
    }

    pub fn start(&mut self, settings: &Settings, steps: &[Step]) -> Result<(), String> {
        self.prepare(settings, steps)?;
        self.state.trigger();
        Ok(())
    }

    pub fn stop(&self) {
        self.state.cancel.stop();
    }

    pub fn take_error(&self) -> Option<String> {
        self.state
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.state
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .quit = true;
        self.state.wake.notify_all();
        self.stop();
        if let Some(handle) = self.rig.take() {
            let _ = handle.join();
        }
    }
}

fn make_device(index: usize, steps: &[Step]) -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    // Standard keyboard + mouse capabilities allow libinput to classify the device.
    for code in 1..=248 {
        keys.insert(KeyCode::new(code));
    }
    for code in KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code() {
        keys.insert(KeyCode::new(code));
    }
    for step in steps {
        if let Step::Press(code) | Step::Release(code) = step {
            keys.insert(KeyCode::new(*code));
        }
    }
    let mut axes = AttributeSet::<RelativeAxisCode>::new();
    for axis in [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y] {
        axes.insert(axis);
    }
    VirtualDevice::builder()?
        .name(&format!("{DEVICE_PREFIX} {}", index + 1))
        .with_keys(&keys)?
        .with_relative_axes(&axes)?
        .build()
}

/// Input codes a device needs beyond the standard keyboard and mouse set.
fn extra_keys(steps: &[Step]) -> BTreeSet<u16> {
    steps
        .iter()
        .filter_map(|step| match step {
            Step::Press(code) | Step::Release(code) => Some(*code),
            Step::Delay(_) => None,
        })
        .filter(|code| {
            !(1..=248).contains(code)
                && !(KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code()).contains(code)
        })
        .collect()
}

fn is_mouse(steps: &[Step]) -> bool {
    steps.iter().any(|step| {
        matches!(step, Step::Press(code)
            if (KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code()).contains(code))
    })
}

/// Virtual devices that stay open between runs.
struct Devices {
    // Declared first so it is dropped, and drains the final events, first.
    _tester: crate::tester::Tester,
    devices: Vec<RoutedDevice>,
    keys: BTreeSet<u16>,
    created: Instant,
}

impl Devices {
    fn create(job: &Job, state: &Arc<PlaybackState>) -> Result<Self, String> {
        let mut devices = Vec::new();
        let mut probes = Vec::new();
        for index in 0..job.devices {
            let mut device = make_device(index, &job.steps).map_err(|error| {
                format!(
                    "Cannot create virtual input device: {error}. Check /dev/uinput access (see README.md)."
                )
            })?;
            let probe = crate::tester::Probe::open(&mut device)
                .map_err(|error| format!("Cannot open the device's input stream: {error}"))?;
            let probe = Arc::new(Mutex::new(probe));
            probes.push(probe.clone());
            devices.push(RoutedDevice {
                device,
                probe,
                captured: false,
                measuring: false,
                has_mouse: false,
                state: state.clone(),
            });
        }
        let tester = crate::tester::Tester::start(probes, state.clone())
            .map_err(|error| format!("Cannot start the CPS reader: {error}"))?;
        Ok(Self {
            _tester: tester,
            devices,
            keys: extra_keys(&job.steps),
            created: Instant::now(),
        })
    }

    fn fit(&self, job: &Job) -> bool {
        self.devices.len() == job.devices && extra_keys(&job.steps).is_subset(&self.keys)
    }

    /// Plays the job on every device until stopped. Returns false if a device failed.
    fn play(&mut self, job: &Job, state: &PlaybackState) -> bool {
        let settle = SETTLE.saturating_sub(self.created.elapsed());
        if !settle.is_zero() && state.cancel.wait(settle) {
            return true;
        }
        state.ready.store(true, Ordering::Release);
        let has_mouse = is_mouse(&job.steps);
        thread::scope(|scope| {
            let workers: Vec<_> = self
                .devices
                .iter_mut()
                .enumerate()
                .map(|(index, device)| {
                    device.has_mouse = has_mouse;
                    thread::Builder::new()
                        .name(format!("device-{}", index + 1))
                        .spawn_scoped(scope, move || {
                            play_loop(device, &job.steps, job.delay, state).map_err(|error| {
                                state.fail(format!("Device {}: {error}", index + 1));
                            })
                        })
                })
                .collect();
            let mut ok = true;
            for worker in workers {
                ok &= match worker {
                    Ok(worker) => worker.join().unwrap_or_else(|_| {
                        state.fail("A device worker unexpectedly exited.".into());
                        Err(())
                    }),
                    Err(error) => {
                        state.fail(format!("Cannot start device worker: {error}"));
                        Err(())
                    }
                }
                .is_ok();
            }
            ok
        })
    }
}

/// Keeps devices that fit the prepared job, and plays it when triggered.
fn rig(state: &Arc<PlaybackState>) {
    let mut devices: Option<Devices> = None;
    loop {
        let (job, start) = {
            let control = state.control.lock().unwrap_or_else(|e| e.into_inner());
            let mut control = state
                .wake
                .wait_while(control, |c| !c.quit && !c.start && !c.changed)
                .unwrap_or_else(|e| e.into_inner());
            if control.quit {
                return;
            }
            control.changed = false;
            (control.job.clone(), std::mem::take(&mut control.start))
        };
        if let Some(job) = &job
            && !devices.as_ref().is_some_and(|devices| devices.fit(job))
        {
            // Remove the old devices before creating their replacements.
            devices = None;
            match Devices::create(job, state) {
                Ok(created) => devices = Some(created),
                // Only report errors for a start, not while preparing ahead.
                Err(error) if start => state.fail(error),
                Err(_) => {}
            }
        }
        if start {
            if let (Some(job), Some(rig)) = (&job, &mut devices)
                && !rig.play(job, state)
            {
                // Recreate devices that failed before the next run.
                devices = None;
            }
            state.ready.store(false, Ordering::Release);
            state.running.store(false, Ordering::Release);
        }
    }
}

trait InputSink {
    fn begin_cycle(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn key(&mut self, code: u16, pressed: bool) -> io::Result<()>;
}

struct RoutedDevice {
    device: VirtualDevice,
    probe: Arc<Mutex<crate::tester::Probe>>,
    captured: bool,
    measuring: bool,
    has_mouse: bool,
    state: Arc<PlaybackState>,
}

impl InputSink for RoutedDevice {
    fn begin_cycle(&mut self) -> io::Result<()> {
        let measure = self.state.testing.load(Ordering::Relaxed);
        let capture = measure || (self.has_mouse && self.state.gui_hovered.load(Ordering::Relaxed));
        if capture != self.captured || measure != self.measuring {
            self.probe
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .route(capture, measure)?;
            self.captured = capture;
            self.measuring = measure;
        }
        Ok(())
    }

    fn key(&mut self, code: u16, pressed: bool) -> io::Result<()> {
        self.device.key(code, pressed)
    }
}

impl InputSink for VirtualDevice {
    fn key(&mut self, code: u16, pressed: bool) -> io::Result<()> {
        self.emit(&[InputEvent::new(EventType::KEY.0, code, i32::from(pressed))])
    }
}

struct HeldKeys<'a, S: InputSink> {
    sink: &'a mut S,
    held: BTreeSet<u16>,
}

impl<S: InputSink> HeldKeys<'_, S> {
    fn send(&mut self, code: u16, pressed: bool) -> io::Result<()> {
        // Track before sending: a partially successful write must also be cleaned up.
        if pressed {
            self.held.insert(code);
        }
        self.sink.key(code, pressed)?;
        if !pressed {
            self.held.remove(&code);
        }
        Ok(())
    }
}

impl<S: InputSink> Drop for HeldKeys<'_, S> {
    fn drop(&mut self) {
        for code in &self.held {
            let _ = self.sink.key(*code, false);
        }
    }
}

fn play_loop(
    sink: &mut impl InputSink,
    steps: &[Step],
    delay: Duration,
    state: &PlaybackState,
) -> io::Result<()> {
    let mut device = HeldKeys {
        sink,
        held: BTreeSet::new(),
    };
    loop {
        if state.cancel.is_stopped() {
            return Ok(());
        }
        device.sink.begin_cycle()?;
        for step in steps {
            if state.cancel.is_stopped() {
                return Ok(());
            }
            match *step {
                Step::Press(code) => device.send(code, true)?,
                Step::Release(code) => device.send(code, false)?,
                Step::Delay(us) => {
                    if us > 0 && state.cancel.wait(Duration::from_micros(us)) {
                        return Ok(());
                    }
                }
            }
        }
        state.loops.fetch_add(1, Ordering::Relaxed);
        if delay.is_zero() {
            // No artificial minimum delay, but let other workers and the UI run.
            thread::yield_now();
        } else if state.cancel.wait(delay) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;
    use std::sync::mpsc;
    use std::time::Instant;

    struct MockSink(mpsc::Sender<(u16, bool)>);
    impl InputSink for MockSink {
        fn key(&mut self, code: u16, pressed: bool) -> io::Result<()> {
            self.0.send((code, pressed)).unwrap();
            Ok(())
        }
    }

    #[test]
    fn stop_interrupts_long_delay_and_releases_held_buttons() {
        let state = Arc::new(PlaybackState::default());
        let shared = state.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            play_loop(
                &mut MockSink(tx),
                &[Step::Press(18), Step::Delay(60_000_000), Step::Release(18)],
                Duration::ZERO,
                &shared,
            )
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), (18, true));
        let start = Instant::now();
        state.cancel.stop();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            (18, false)
        );
        worker.join().unwrap().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(state.loops.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn preserves_chord_order_before_loop_delay() {
        let state = Arc::new(PlaybackState::default());
        let shared = state.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            play_loop(
                &mut MockSink(tx),
                &[
                    Step::Press(29),
                    Step::Press(18),
                    Step::Release(18),
                    Step::Release(29),
                ],
                Duration::from_secs(60),
                &shared,
            )
        });
        for expected in [(29, true), (18, true), (18, false), (29, false)] {
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), expected);
        }
        state.cancel.stop();
        worker.join().unwrap().unwrap();
        assert_eq!(state.loops.load(Ordering::Relaxed), 1);
    }

    #[test]
    #[ignore = "requires /dev/uinput and /dev/input access"]
    fn uinput_round_trip_on_parallel_devices() {
        // Exclusively grab only the test-created virtual devices before emitting;
        // these test presses must never be delivered to the desktop.
        let mut devices = Vec::new();
        let mut readers = Vec::new();
        for index in 0..5 {
            let mut device = make_device(index, &crate::model::click_sequence()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut reader = loop {
                let paths: Vec<_> = device
                    .enumerate_dev_nodes_blocking()
                    .unwrap()
                    .filter_map(Result::ok)
                    .collect();
                if let Some(reader) = paths.iter().find_map(|path| evdev::Device::open(path).ok()) {
                    break reader;
                }
                assert!(
                    Instant::now() < deadline,
                    "Virtual device node did not become accessible"
                );
                thread::sleep(Duration::from_millis(5));
            };
            reader.grab().unwrap();
            reader.set_nonblocking(true).unwrap();
            devices.push(device);
            readers.push(reader);
        }
        let expected = [
            (18, true),
            (18, false),
            (KeyCode::BTN_LEFT.code(), true),
            (KeyCode::BTN_LEFT.code(), false),
        ];
        for device in &mut devices {
            for (code, pressed) in expected {
                device.key(code, pressed).unwrap();
            }
        }
        for reader in &mut readers {
            let mut actual = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(2);
            while actual.len() < expected.len() {
                match reader.fetch_events() {
                    Ok(events) => actual.extend(
                        events
                            .filter(|event| event.event_type() == EventType::KEY)
                            .map(|event| (event.code(), event.value() == 1)),
                    ),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("Reading virtual device failed: {error}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "Virtual device did not deliver input"
                );
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(actual, expected);
        }
    }

    #[test]
    #[ignore = "requires /dev/uinput and /dev/input access"]
    fn zero_delay_kernel_tester_counts_events_and_isolates_desktop_delivery() {
        let steps = [
            Step::Press(KeyCode::BTN_LEFT.code()),
            Step::Release(KeyCode::BTN_LEFT.code()),
        ];
        let mut device = make_device(0, &steps).unwrap();
        let keepalive = device.as_fd().try_clone_to_owned().unwrap();
        let probe = Arc::new(Mutex::new(crate::tester::Probe::open(&mut device).unwrap()));
        let path = device
            .enumerate_dev_nodes_blocking()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let mut observer = evdev::Device::open(path).unwrap();
        observer.set_nonblocking(true).unwrap();
        let state = Arc::new(PlaybackState::default());
        state.testing.store(true, Ordering::Relaxed);
        let tester = crate::tester::Tester::start(vec![probe.clone()], state.clone()).unwrap();
        let mut sink = RoutedDevice {
            device,
            probe,
            captured: false,
            measuring: false,
            has_mouse: true,
            state: state.clone(),
        };
        let shared = state.clone();
        let worker = thread::spawn(move || play_loop(&mut sink, &steps, Duration::ZERO, &shared));
        thread::sleep(Duration::from_secs(2));
        state.cancel.stop();
        worker.join().unwrap().unwrap();
        drop(tester);
        let loops = state.loops.load(Ordering::Relaxed);
        let clicks = state.test_clicks.load(Ordering::Relaxed);
        let overruns = state.test_overruns.load(Ordering::Relaxed);
        eprintln!(
            "zero-delay: {loops} loops, {clicks} received clicks, {overruns} overruns in 2 seconds"
        );
        assert!(loops > 1000 && clicks > 1000);
        assert!(clicks <= loops + 1);
        if overruns == 0 {
            assert!(clicks >= loops);
        }
        match observer.fetch_events() {
            Ok(events) => assert!(
                events
                    .filter(|event| event.event_type() == EventType::KEY)
                    .count()
                    == 0
            ),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
        }
        drop(keepalive);
    }

    #[test]
    #[ignore = "requires /dev/uinput and /dev/input access"]
    fn prepared_devices_start_without_waiting() {
        let mut engine = Engine::default();
        // Capture into the tester so no click reaches the desktop.
        engine.state.testing.store(true, Ordering::Relaxed);
        let settings = Settings::default();
        let steps = crate::model::click_sequence();
        let first_click = |engine: &Engine| {
            let started = Instant::now();
            assert!(engine.state.trigger());
            while engine.state.test_clicks.load(Ordering::Relaxed) == 0 {
                assert!(started.elapsed() < Duration::from_secs(5), "{:?}", engine.take_error());
                thread::sleep(Duration::from_micros(200));
            }
            engine.stop();
            while engine.running() {
                thread::sleep(Duration::from_millis(1));
            }
            engine.state.test_clicks.store(0, Ordering::Relaxed);
            started.elapsed()
        };
        // The first start also creates the devices, as every start used to.
        engine.prepare(&settings, &steps).unwrap();
        let cold = first_click(&engine);
        let mut times = Vec::new();
        for _ in 0..5 {
            times.push(first_click(&engine));
        }
        eprintln!("trigger to first click: new devices {cold:?}, prepared devices {times:?}");
        assert!(times.iter().all(|time| *time < Duration::from_millis(50)));
    }

    #[test]
    #[ignore = "requires /dev/uinput and /dev/input access"]
    fn fifty_devices_run_uncapped_in_the_kernel_tester_and_stop_cleanly() {
        let mut engine = Engine::default();
        engine.state.testing.store(true, Ordering::Relaxed);
        let settings = Settings {
            loop_delay_ms: 0,
            parallel: true,
            devices: 50,
            ..Settings::default()
        };
        let steps = [
            Step::Press(KeyCode::BTN_LEFT.code()),
            Step::Release(KeyCode::BTN_LEFT.code()),
        ];
        engine.start(&settings, &steps).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !engine.state.ready.load(Ordering::Acquire) {
            assert!(engine.running(), "{:?}", engine.take_error());
            assert!(Instant::now() < deadline, "Devices did not become ready");
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_secs(2));
        engine.stop();
        let stopped_at = Instant::now();
        thread::sleep(Duration::from_millis(100));
        let stopped_loops = engine.state.loops.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            engine.state.loops.load(Ordering::Relaxed),
            stopped_loops,
            "Input continued after Stop"
        );
        // Removing 50 kernel devices can take multiple RCU/udev grace periods;
        // input must stop promptly even while device destruction finishes.
        let deadline = Instant::now() + Duration::from_secs(15);
        while engine.running() {
            assert!(Instant::now() < deadline, "Parallel workers failed to stop");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(engine.take_error().is_none());
        let loops = engine.state.loops.load(Ordering::Relaxed);
        let clicks = engine.state.test_clicks.load(Ordering::Relaxed);
        let overruns = engine.state.test_overruns.load(Ordering::Relaxed);
        eprintln!(
            "50 devices, zero delay: {loops} loops, {clicks} received clicks, {overruns} overruns in 2 seconds; cleanup {:?}",
            stopped_at.elapsed()
        );
        assert!(loops > 1000 && clicks > 1000);
        assert!(clicks <= loops + 50);
        if overruns == 0 {
            assert!(clicks >= loops);
        }
    }
}
