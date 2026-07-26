use std::{
    io,
    sync::{Arc, Mutex, mpsc},
};

use crate::{config::data, cpu_handle::cpu_freq::CpuFreq};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Boost((u8, (u32, u32))),
    SetFreq((u8, (u32, u32))),
    ApplyMode(data::RuntimeMode),
    RestoreHardware,
    ApplySleep(data::RuntimeMode),
}

pub struct Manager {
    rx: mpsc::Receiver<Event>,
    cpu_freq_handle: CpuFreq,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
    config: data::Config,
}

impl Manager {
    pub fn new(
        rx: mpsc::Receiver<Event>,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        config: data::Config,
    ) -> io::Result<Self> {
        let cpu_freq_handle = CpuFreq::new(config.clone(), logger_handle.clone())?;
        Ok(Self {
            rx,
            cpu_freq_handle,
            logger_handle,
            config,
        })
    }

    fn apply_mode(&mut self, mode: data::RuntimeMode) -> io::Result<()> {
        let set = self.config.mode_policy(mode);
        let idle_governor = set.idle_governor.to_string();
        let policies = set.policy.to_vec();

        for (cpu, policy) in self.config.policy.iter().zip(policies.iter()) {
            self.cpu_freq_handle
                .write_index_governor(cpu.from as u8, &policy.governor)?;
            self.cpu_freq_handle.write_index_freq(
                cpu.from as u8,
                (policy.min_freq, policy.max_freq),
            )?;
        }

        self.cpu_freq_handle.write_idle_governor(&idle_governor)
    }

    fn apply_sleep(&mut self, mode: data::RuntimeMode) -> io::Result<()> {
        let policies = self.config.mode_policy(mode).policy.to_vec();

        for (cpu, policy) in self.config.policy.iter().zip(policies.iter()) {
            self.cpu_freq_handle.write_index_freq(
                cpu.from as u8,
                (policy.min_freq, policy.sleep_freq),
            )?;
        }

        Ok(())
    }

    fn handle_event(&mut self, event: &Event) -> io::Result<()> {
        match event {
            Event::Boost((index, limits)) | Event::SetFreq((index, limits)) => {
                self.cpu_freq_handle.write_index_freq(*index, *limits)
            }
            Event::ApplyMode(mode) => self.apply_mode(*mode),
            Event::RestoreHardware => self.cpu_freq_handle.restore_hardware_limits(),
            Event::ApplySleep(mode) => self.apply_sleep(*mode),
        }
    }

    pub fn start_loop(&mut self) {
        while let Ok(event) = self.rx.recv() {
            let result = self.handle_event(&event);
            match result {
                Ok(())
                    if matches!(
                        &event,
                        Event::ApplyMode(_) | Event::RestoreHardware | Event::ApplySleep(_)
                    ) =>
                {
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.info(format!("调度事件执行成功: {event:?}"));
                    }
                }
                Ok(()) => {}
                Err(error) => {
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.warn(format!("调度事件执行失败: {event:?}, 错误: {error}"))
                    }
                }
            }
        }
    }
}
