//! Read actual uinput events on a dedicated thread. While the pointer is over
//! the CPS pad, exclusively grab our own virtual devices, so an unbounded test
//! stream doesn't have to traverse the compositor/windowing/widget pipeline.
use std::{
    collections::VecDeque,
    io,
    os::fd::{AsFd, OwnedFd},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use evdev::{
    EventType, KeyCode, SynchronizationCode, raw_stream::RawDevice, uinput::VirtualDevice,
};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
};

use crate::engine::PlaybackState;

struct Interval {
    start: Duration,
    end: Option<Duration>,
}

pub struct Probe {
    reader: RawDevice,
    captured: bool,
    measuring: bool,
    intervals: VecDeque<Interval>,
}

impl Probe {
    pub fn open(device: &mut VirtualDevice) -> io::Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            for path in device.enumerate_dev_nodes_blocking()?.flatten() {
                if let Ok(reader) = RawDevice::open(path) {
                    let flags = fcntl_getfl(&reader)?;
                    fcntl_setfl(&reader, flags | OFlags::NONBLOCK)?;
                    return Ok(Self {
                        reader,
                        captured: false,
                        measuring: false,
                        intervals: VecDeque::new(),
                    });
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "cannot read the virtual device; grant /dev/input access (README.md)",
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Called only between complete, balanced sequences, never while a key is held.
    pub fn route(&mut self, capture: bool, measure: bool) -> io::Result<()> {
        let measure = capture && measure;
        if capture == self.captured && measure == self.measuring {
            return Ok(());
        }
        let now = timestamp();
        if capture != self.captured {
            if capture {
                self.reader.grab()?;
            } else {
                self.reader.ungrab()?;
            }
        }
        if self.measuring
            && let Some(interval) = self.intervals.back_mut()
        {
            interval.end = Some(now);
        }
        if measure {
            self.intervals.push_back(Interval {
                start: now,
                end: None,
            });
        }
        self.captured = capture;
        self.measuring = measure;
        Ok(())
    }

    fn drain(&mut self, state: &PlaybackState) -> io::Result<()> {
        let mut count = 0;
        let mut dropped = 0;
        let mut last = Duration::ZERO;
        // Bound work per ready device so one fast device cannot starve the others.
        // This bounds reader scheduling, not playback or the number of emitted events.
        for _ in 0..64 {
            match self.reader.fetch_events() {
                Ok(events) => {
                    for event in events {
                        let is_overrun = event.event_type() == EventType::SYNCHRONIZATION
                            && event.code() == SynchronizationCode::SYN_DROPPED.0;
                        let is_click = event.event_type() == EventType::KEY
                            && event.value() == 1
                            && (KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code())
                                .contains(&event.code());
                        // Most events are releases or SYN_REPORT. They do not
                        // need timestamp conversion or capture-interval lookup.
                        if !is_overrun && !is_click {
                            continue;
                        }
                        let at = event
                            .timestamp()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default();
                        last = last.max(at);
                        let testing = self.intervals.iter().any(|interval| {
                            at >= interval.start && interval.end.is_none_or(|end| at <= end)
                        });
                        if !testing {
                            continue;
                        }
                        if is_overrun {
                            dropped += 1;
                        } else {
                            count += 1;
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.raw_os_error() == Some(19) => break, // Removed device; retain the count already read.
                Err(error) => return Err(error),
            }
        }
        while self
            .intervals
            .front()
            .is_some_and(|interval| interval.end.is_some_and(|end| end < last))
        {
            self.intervals.pop_front();
        }
        state.test_clicks.fetch_add(count, Ordering::Relaxed);
        state.test_overruns.fetch_add(dropped, Ordering::Relaxed);
        Ok(())
    }
}

pub struct Tester {
    done: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Tester {
    pub fn start(probes: Vec<Arc<Mutex<Probe>>>, state: Arc<PlaybackState>) -> io::Result<Self> {
        // Poll duplicated descriptors for the SAME open evdev clients that own
        // the grabs. Only the Probe reads; duplicated descriptors are polling-only.
        let fds: Vec<OwnedFd> = probes
            .iter()
            .map(|probe| {
                probe
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reader
                    .as_fd()
                    .try_clone_to_owned()
            })
            .collect::<io::Result<_>>()?;
        let done = Arc::new(AtomicBool::new(false));
        let shared = done.clone();
        let worker = thread::Builder::new()
            .name("kernel-cps-reader".into())
            .spawn(move || {
                let mut poll_fds: Vec<_> = fds
                    .iter()
                    .map(|fd| PollFd::new(fd, PollFlags::IN))
                    .collect();
                let timeout = Timespec {
                    tv_sec: 0,
                    tv_nsec: 10_000_000,
                };
                while !shared.load(Ordering::Acquire) {
                    match poll(&mut poll_fds, Some(&timeout)) {
                        Ok(_) => {
                            for (index, fd) in poll_fds.iter().enumerate() {
                                if fd.revents().contains(PollFlags::IN) {
                                    let _ = probes[index]
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .drain(&state);
                                }
                            }
                        }
                        Err(rustix::io::Errno::INTR) => continue,
                        Err(_) => break,
                    }
                }
                for probe in &probes {
                    let _ = probe
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .drain(&state);
                }
            })?;
        Ok(Self {
            done,
            worker: Some(worker),
        })
    }
}

impl Drop for Tester {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn timestamp() -> Duration {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
}
