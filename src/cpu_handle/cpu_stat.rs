use std::{
    fs::OpenOptions,
    io::{self, Read},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize},
        mpsc::{self},
    },
    time::Duration,
};

use crate::{
    config::data,
    cpu_handle::cpu_freq::{self},
    scheduler::manager::Event,
};

fn calculate_target_limits(
    load_percent: u32,
    current_max: u32,
    policy: &data::MPolicy,
) -> Option<(u32, u32)> {
    let load = load_percent as f32 / 100.0;
    let target = (policy.max_freq as f32 * load * policy.margin)
        .clamp(policy.min_freq as f32, policy.max_freq as f32) as u32;

    if target.abs_diff(current_max) < policy.diff {
        None
    } else {
        Some((policy.min_freq, target))
    }
}

pub struct CpuStat<'a> {
    policy_id: usize,
    from: u32,
    to: u32,
    history: Vec<(u64, u64)>,
    file_path: &'a str,
    buffer: String,
    tx: mpsc::Sender<Event>,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
    policy_freq: cpu_freq::Policy,
    mode: Arc<AtomicUsize>,
    onf: Arc<AtomicBool>,
    is_game: Arc<AtomicBool>,
    config: data::Config,
}

impl<'a> CpuStat<'a> {
    pub fn new(
        policy_id: usize,
        from: u32,
        to: u32,
        tx: mpsc::Sender<Event>,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        config: data::Config,
        mode: Arc<AtomicUsize>,
        onf: Arc<AtomicBool>,
        is_game: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let num_cpus = if to >= from {
            (to - from + 1) as usize
        } else {
            0
        };

        let freq_handle = cpu_freq::Policy::new(from, logger_handle.clone())?;

        Ok(Self {
            policy_id,
            from,
            to,
            history: vec![(0, 0); num_cpus],
            file_path: "/proc/stat",
            // 复用buffer,预先分配2k内出
            buffer: String::with_capacity(2048),
            tx,
            logger_handle,
            policy_freq: freq_handle,
            mode,
            onf,
            is_game,
            config,
        })
    }

    fn get_cpu_load(&mut self) -> u32 {
        self.buffer.clear();

        let mut file = match OpenOptions::new().read(true).open("/proc/stat") {
            Ok(f) => f,
            Err(e) => {
                if let Ok(mut log) = self.logger_handle.lock() {
                    log.error(format!("无法打开文件:/proc/stat 错误:{}", e));
                }
                return 50; // 不 panic，让程序继续运行
            }
        };

        if let Err(e) = file.read_to_string(&mut self.buffer) {
            if let Ok(mut log) = self.logger_handle.lock() {
                log.error(format!("读取:{} 文件错误:{}", self.file_path, e));
            }
            return 50;
        }

        let mut total_load_sum = 0_u64;
        let mut counted_cpus = 0_u32;

        for line in self.buffer.lines() {
            if !line.starts_with("cpu") {
                continue;
            }

            let first_word = line.split_whitespace().next().unwrap_or("");
            if first_word == "cpu" {
                continue;
            }

            //获取cpu编号
            if let Ok(cpu_index) = first_word["cpu".len()..].parse::<u32>()
                && cpu_index >= self.from
                && cpu_index <= self.to
            {
                let history_idx = (cpu_index - self.from) as usize;

                let parts: Vec<u64> = line
                    .split_whitespace()
                    .skip(1)
                    //提升容错：用 filter_map 代替 expect，防止个别行数据破损导致 crash
                    .filter_map(|v| v.parse().ok())
                    .collect();

                if parts.len() < 4 {
                    continue;
                }

                let current_total: u64 = parts.iter().sum();
                let current_idle = parts[3] + parts.get(4).unwrap_or(&0);

                let (prev_total, prev_idle) = self.history[history_idx];

                // 只有当历史有记录，且时间戳推进时才计算（防止高频读取时部分核心未更新导致 total_diff 为 0）
                if prev_total != 0 && current_total > prev_total {
                    let total_diff = current_total - prev_total;
                    let idle_diff = current_idle.saturating_sub(prev_idle);

                    let cpu_load = ((total_diff - idle_diff) * 100) / total_diff;
                    total_load_sum += cpu_load;
                    counted_cpus += 1;
                }
                self.history[history_idx] = (current_total, current_idle);
            }
        }

        if counted_cpus > 0 {
            let avg_load = (total_load_sum / counted_cpus as u64) as u32;
            return avg_load;
        }

        50
    }

    pub fn start_send_event_loop(&mut self) {
        loop {
            let mode = data::RuntimeMode::from_index(
                self.mode.load(std::sync::atomic::Ordering::Relaxed),
            )
            .unwrap_or(data::RuntimeMode::Power);
            let policy = self.config.mode_policy(mode).policy[self.policy_id].clone();

            std::thread::sleep(Duration::from_millis(policy.delay));

            if !self.onf.load(std::sync::atomic::Ordering::Relaxed)
                || self.is_game.load(std::sync::atomic::Ordering::Relaxed)
            {
                continue;
            }

            let load = self.get_cpu_load();
            let current_max = match self.policy_freq.read_max() {
                Ok(freq) => freq,
                Err(error) => {
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.error(format!("读取 max_freq 失败: {error}"));
                    }
                    continue;
                }
            };

            if let Some(limits) = calculate_target_limits(load, current_max, &policy)
                && let Err(error) = self.tx.send(Event::SetFreq((self.from as u8, limits)))
            {
                if let Ok(mut log) = self.logger_handle.lock() {
                    log.error(format!("发送频率设置事件失败: {error}"));
                }
            }
        }
    }

    pub const fn policy_id(&self) -> usize {
        self.policy_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> data::MPolicy {
        data::MPolicy {
            delay: 400,
            max_freq: 3_000_000,
            min_freq: 384_000,
            can_boost_freq: 1_000_000,
            boost_freq: 2_400_000,
            margin: 2.0,
            diff: 70_000,
            governor: "walt".to_string(),
            sleep_freq: 960_000,
        }
    }

    #[test]
    fn target_uses_mode_max_instead_of_current_cap() {
        let policy = test_policy();
        assert_eq!(
            calculate_target_limits(50, 960_000, &policy),
            Some((384_000, 3_000_000))
        );
    }

    #[test]
    fn target_skips_small_changes() {
        let mut policy = test_policy();
        policy.margin = 1.0;
        policy.diff = 100_000;
        assert_eq!(calculate_target_limits(50, 1_550_000, &policy), None);
    }
}
