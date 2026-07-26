use evdev::enumerate;
use nix::fcntl::{OFlag, open};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::stat::Mode;
use nix::unistd::read;
use std::io;
use std::mem::size_of;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::config::data;
use crate::cpu_handle::cpu_freq::CpuFreq;
use crate::scheduler::manager::Event;

#[repr(C)]
#[derive(Debug)]
pub struct TouchEvent {
    pub tv_sec: i64,
    pub tv_usec: i64,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

pub fn open_devices(devices: &str) -> io::Result<OwnedFd> {
    open(devices, OFlag::O_RDONLY | OFlag::O_NONBLOCK, Mode::empty())
        .map_err(|error| io::Error::from_raw_os_error(error as i32))
}

pub struct Moniter {
    devices: OwnedFd,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
    config: data::Config,
    mode: Arc<AtomicUsize>,
    cpu_freq_handle: CpuFreq,
    onf: Arc<AtomicBool>,
    is_game: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
}

impl Moniter {
    pub fn new(
        devices: &str,
        tx: mpsc::Sender<Event>,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        config: data::Config,
        mode: Arc<AtomicUsize>,
        onf: Arc<AtomicBool>,
        is_game: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let devices = open_devices(devices)?;
        let cpu_freq_handle = CpuFreq::new(config.clone(), logger_handle.clone())?;
        Ok(Self {
            devices,
            tx,
            config,
            mode,
            cpu_freq_handle,
            logger_handle,
            onf,
            is_game,
        })
    }

    fn touch_monitor(&self) -> bool {
        let event_size = size_of::<TouchEvent>();
        let mut buffer = vec![0u8; event_size];
        let borrowed_fd = self.devices.as_fd();
        let mut poll_fd = [PollFd::new(borrowed_fd, PollFlags::POLLIN)];
        let mut touched = false;

        match poll(&mut poll_fd, PollTimeout::NONE) {
            Ok(n) if n > 0 => {
                if let Some(flags) = poll_fd[0].revents()
                    && flags.contains(PollFlags::POLLIN)
                {
                    loop {
                        match read(self.devices.as_fd(), &mut buffer) {
                            Ok(bytes_read) if bytes_read == event_size => touched = true,
                            Ok(_) | Err(nix::Error::EAGAIN) => break,
                            Err(error) => {
                                if let Ok(mut log) = self.logger_handle.lock() {
                                    log.error(format!("Failed to read touch input event: {error}"));
                                }
                                break;
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                if let Ok(mut log) = self.logger_handle.lock() {
                    log.error(format!("Failed to poll touch input: {error}"));
                }
            }
        }

        touched
    }

    pub fn start_loop(&mut self) {
        loop {
            if !self.touch_monitor()
                || !self.onf.load(Ordering::Relaxed)
                || self.is_game.load(Ordering::Relaxed)
            {
                continue;
            }

            let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                .unwrap_or(data::RuntimeMode::Power);
            let policies = self.config.mode_policy(mode).policy.to_vec();
            let cpu_policies = self.config.policy.clone();
            let mut boosted = false;

            for (cpu, policy) in cpu_policies.iter().zip(policies.iter()) {
                let Some(current_policy) = self.cpu_freq_handle.policys.get_mut(&(cpu.from as u8))
                else {
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.warn(format!("Touch Boost policy {} is unavailable", cpu.from));
                    }
                    continue;
                };

                match current_policy.read_max() {
                    Ok(freq) if freq < policy.can_boost_freq => {
                        match self.tx.send(Event::Boost((
                            cpu.from as u8,
                            (policy.boost_freq, policy.boost_freq),
                        ))) {
                            Ok(()) => boosted = true,
                            Err(error) => {
                                if let Ok(mut log) = self.logger_handle.lock() {
                                    log.warn(format!(
                                        "Failed to send Touch Boost event for policy {}: {error}",
                                        cpu.from
                                    ));
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if let Ok(mut log) = self.logger_handle.lock() {
                            log.warn(format!(
                                "Failed to read max frequency for policy {}: {error}",
                                cpu.from
                            ));
                        }
                    }
                }
            }

            if boosted {
                std::thread::sleep(Duration::from_millis(300));
                if self.onf.load(Ordering::Relaxed) && !self.is_game.load(Ordering::Relaxed) {
                    let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                        .unwrap_or(data::RuntimeMode::Power);
                    if let Err(error) = self.tx.send(Event::EndBoost(mode))
                        && let Ok(mut log) = self.logger_handle.lock()
                    {
                        log.warn(format!("Failed to restore mode after Touch Boost: {error}"));
                    }
                }
            }
        }
    }
}

pub fn find_touchscreen_device() -> Option<String> {
    for (path, device) in enumerate() {
        if let Some(abs_bits) = device.supported_absolute_axes()
            && abs_bits.contains(evdev::AbsoluteAxisCode::ABS_MT_POSITION_X)
        {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}
