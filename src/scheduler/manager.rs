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

    fn log_success(&self, event: &Event) {
        let Ok(mut log) = self.logger_handle.lock() else {
            return;
        };

        match event {
            Event::Boost((index, (min_freq, max_freq))) => log.debug(format!(
                "Touch Boost 已应用: policy{index}, min_freq={min_freq} kHz, max_freq={max_freq} kHz"
            )),
            Event::SetFreq((index, (min_freq, max_freq))) => log.debug(format!(
                "动态调频已应用: policy{index}, min_freq={min_freq} kHz, max_freq={max_freq} kHz"
            )),
            Event::ApplyMode(mode) => {
                let set = self.config.mode_policy(*mode);
                log.info(format!(
                    "模式策略已应用: mode={}, idle_governor={}",
                    mode.name(),
                    set.idle_governor
                ));
                for (cpu, policy) in self.config.policy.iter().zip(set.policy.iter()) {
                    log.info(format!(
                        "  policy{}(cpu{}-{}): governor={}, min_freq={} kHz, max_freq={} kHz, sleep_freq={} kHz, delay={} ms, margin={}, diff={} kHz, can_boost_freq={} kHz, boost_freq={} kHz",
                        cpu.from,
                        cpu.from,
                        cpu.to,
                        policy.governor,
                        policy.min_freq,
                        policy.max_freq,
                        policy.sleep_freq,
                        policy.delay,
                        policy.margin,
                        policy.diff,
                        policy.can_boost_freq,
                        policy.boost_freq
                    ));
                }
            }
            Event::RestoreHardware => {
                log.info("游戏场景已恢复硬件频率范围".to_string());
                for cpu in &self.config.policy {
                    if let Some(policy) = self.cpu_freq_handle.policys.get(&(cpu.from as u8)) {
                        let (min_freq, max_freq) = policy.hardware_limits();
                        log.info(format!(
                            "  policy{}(cpu{}-{}): hardware_min={} kHz, hardware_max={} kHz",
                            cpu.from, cpu.from, cpu.to, min_freq, max_freq
                        ));
                    }
                }
            }
            Event::ApplySleep(mode) => {
                let set = self.config.mode_policy(*mode);
                log.info(format!("熄屏策略已应用: mode={}", mode.name()));
                for (cpu, policy) in self.config.policy.iter().zip(set.policy.iter()) {
                    log.info(format!(
                        "  policy{}(cpu{}-{}): min_freq={} kHz, sleep_max={} kHz",
                        cpu.from, cpu.from, cpu.to, policy.min_freq, policy.sleep_freq
                    ));
                }
            }
        }
    }

    pub fn start_loop(&mut self) {
        while let Ok(event) = self.rx.recv() {
            let result = self.handle_event(&event);
            match result {
                Ok(()) => self.log_success(&event),
                Err(error) => {
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.warn(format!("调度事件执行失败: {event:?}, 错误: {error}"))
                    }
                }
            }
        }
    }
}
