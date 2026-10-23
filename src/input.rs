use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use evdev::{Device, EventType};

use crate::engine::{DEVICE_PREFIX, PlaybackState};

pub const IDLE: u8 = 0;
pub const RECORDING: u8 = 1;
pub const SELECT_HOTKEY: u8 = 2;
pub const SELECT_STEP: u8 = 3;

pub enum Message {
    Key {
        code: u16,
        pressed: bool,
        at: Duration,
        mode: u8,
        in_area: bool,
    },
    /// A binding's hotkey was pressed.
    Toggle {
        binding: BindingId,
        stop_only: bool,
    },
    /// The hotkey of a hold-to-play binding was pressed or released.
    Hold {
        binding: BindingId,
        pressed: bool,
    },
}

#[derive(Clone, Default)]
pub struct DeviceStatus {
    pub connected: usize,
    pub denied: usize,
}

/// A binding's profile index and its index within that profile.
pub type BindingId = (usize, usize);

/// A binding that is listened for: its hotkey, whether it plays only while
/// held, and the playback it controls.
pub struct Listened {
    pub id: BindingId,
    pub hotkey: u16,
    pub hold: bool,
    pub playback: Arc<PlaybackState>,
}

pub struct InputState {
    pub bindings: Mutex<Vec<Listened>>,
    pub mode: AtomicU8,
    pub in_area: AtomicBool,
    pub overflow: AtomicBool,
    pub status: Mutex<DeviceStatus>,
    shutdown: AtomicBool,
}

pub struct InputMonitor {
    pub state: Arc<InputState>,
    pub messages: Receiver<Message>,
    thread: Option<JoinHandle<()>>,
}

impl InputMonitor {
    pub fn new() -> Self {
        let state = Arc::new(InputState {
            bindings: Mutex::new(Vec::new()),
            mode: AtomicU8::new(IDLE),
            in_area: AtomicBool::new(false),
            overflow: AtomicBool::new(false),
            status: Mutex::new(DeviceStatus::default()),
            shutdown: AtomicBool::new(false),
        });
        let (tx, messages) = mpsc::sync_channel(16_384);
        let shared = state.clone();
        let thread = thread::Builder::new()
            .name("physical-input".into())
            .spawn(move || monitor(&shared, tx))
            .expect("Cannot create input monitor thread");
        Self {
            state,
            messages,
            thread: Some(thread),
        }
    }

    pub fn status(&self) -> DeviceStatus {
        self.state
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

struct Reader {
    path: PathBuf,
    device: Device,
}

fn discover(readers: &mut Vec<Reader>, state: &InputState) {
    let mut denied = 0;
    if let Ok(entries) = fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("event") {
                continue;
            }
            let path = entry.path();
            if readers.iter().any(|reader| reader.path == path) {
                continue;
            }
            match Device::open(&path) {
                Ok(device) => {
                    if device
                        .name()
                        .is_some_and(|name| name.starts_with(DEVICE_PREFIX))
                        || device.supported_keys().is_none()
                    {
                        continue;
                    }
                    if device.set_nonblocking(true).is_ok() {
                        readers.push(Reader { path, device });
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
                Err(_) => {}
            }
        }
    }
    *state.status.lock().unwrap_or_else(|e| e.into_inner()) = DeviceStatus {
        connected: readers.len(),
        denied,
    };
}

fn monitor(state: &InputState, tx: SyncSender<Message>) {
    let mut readers = Vec::new();
    discover(&mut readers, state);
    let mut scan = Instant::now();
    let mut events = Vec::new();
    while !state.shutdown.load(Ordering::Acquire) {
        if scan.elapsed() >= Duration::from_secs(2) {
            discover(&mut readers, state);
            scan = Instant::now();
        }
        events.clear();
        let mut disconnected = HashSet::new();
        for reader in &mut readers {
            match reader.device.fetch_events() {
                Ok(batch) => {
                    for event in batch {
                        if event.event_type() == EventType::KEY && matches!(event.value(), 0 | 1) {
                            let at = event
                                .timestamp()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default();
                            events.push((at, event.code(), event.value() == 1));
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    disconnected.insert(reader.path.clone());
                }
            }
        }
        readers.retain(|reader| !disconnected.contains(&reader.path));
        // Devices have separate queues: merge their kernel timestamps, not read order.
        events.sort_by_key(|event| event.0);
        for &(at, code, pressed) in &events {
            let mode = state.mode.load(Ordering::Acquire);
            if mode == IDLE {
                let bindings = state.bindings.lock().unwrap_or_else(|e| e.into_inner());
                for listened in bindings.iter() {
                    let Listened { id: binding, hotkey, hold, ref playback } = *listened;
                    if hotkey != code || (!pressed && !hold) {
                        continue;
                    }
                    let message = if hold {
                        // The release is still sent: the start may not have happened yet.
                        Message::Hold { binding, pressed }
                    } else {
                        Message::Toggle {
                            binding,
                            stop_only: playback.running.load(Ordering::Acquire),
                        }
                    };
                    if matches!(
                        message,
                        Message::Toggle { stop_only: true, .. } | Message::Hold { pressed: false, .. }
                    ) {
                        // Stop directly from this thread, even if rendering is overloaded.
                        playback.cancel.stop();
                    }
                    if tx.try_send(message).is_err() {
                        state.overflow.store(true, Ordering::Release);
                    }
                }
            } else {
                let message = Message::Key {
                    code,
                    pressed,
                    at,
                    mode,
                    in_area: state.in_area.load(Ordering::Acquire),
                };
                if tx.try_send(message).is_err() {
                    state.overflow.store(true, Ordering::Release);
                }
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
}
