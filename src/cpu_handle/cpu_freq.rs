use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    sync::{Arc, Mutex},
};

use crate::{
    config::data::{self},
    utils,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitWrite {
    Min(u32),
    Max(u32),
}

fn plan_limit_writes(current: (u32, u32), target: (u32, u32)) -> io::Result<Vec<LimitWrite>> {
    if target.0 > target.1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("minimum {} exceeds maximum {}", target.0, target.1),
        ));
    }

    let mut writes = Vec::with_capacity(2);
    if target.1 > current.1 {
        if target.1 != current.1 {
            writes.push(LimitWrite::Max(target.1));
        }
        if target.0 != current.0 {
            writes.push(LimitWrite::Min(target.0));
        }
    } else {
        if target.0 != current.0 {
            writes.push(LimitWrite::Min(target.0));
        }
        if target.1 != current.1 {
            writes.push(LimitWrite::Max(target.1));
        }
    }
    Ok(writes)
}

fn write_bytes(file: &mut File, value: &[u8]) -> io::Result<()> {
    file.seek(io::SeekFrom::Start(0))?;
    file.write_all(value)?;
    file.flush()
}

fn write_value(file: &mut File, value: u32) -> io::Result<()> {
    write_bytes(file, value.to_string().as_bytes())
}

fn read_value(file: &mut File) -> io::Result<u32> {
    let mut buffer = String::new();
    file.seek(io::SeekFrom::Start(0))?;
    file.read_to_string(&mut buffer)?;
    buffer.trim().parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid frequency: {error}"),
        )
    })
}

fn with_path(action: &str, path: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("failed to {action} {path}: {error}"))
}

fn read_frequency_path(path: &str) -> io::Result<u32> {
    let mut file = File::open(path).map_err(|error| with_path("open", path, error))?;
    read_value(&mut file).map_err(|error| with_path("read", path, error))
}

fn read_text_path(path: &str) -> io::Result<String> {
    std::fs::read_to_string(path)
        .map(|value| value.trim().to_string())
        .map_err(|error| with_path("read", path, error))
}

pub struct CpuFreq {
    pub policys: HashMap<u8, Policy>,
    idle_governor: Option<String>,
    original_idle_governor: Option<String>,
}

impl CpuFreq {
    pub fn new(
        config: data::Config,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
    ) -> io::Result<Self> {
        let result = utils::set_file_permissions_numeric(
            "/sys/devices/system/cpu/cpuidle/current_governor",
            0o666,
        );

        let have_idle = match result {
            Ok(_) => true,
            Err(e) => {
                if let Ok(mut log) = logger_handle.lock() {
                    log.error(format!(
                        "无法修改文件权限:/sys/devices/system/cpu/cpuidle/current_governor 错误:{}",
                        e
                    ));
                }
                false
            }
        };

        let idle_governor = if have_idle {
            Some("/sys/devices/system/cpu/cpuidle/current_governor".to_string())
        } else {
            None
        };
        let original_idle_governor = idle_governor
            .as_deref()
            .and_then(|path| read_text_path(path).ok());

        let mut lines = Vec::new();

        for i in config.policy {
            lines.push(i.from);
        }

        let mut hash_map_policy = HashMap::new();

        for line in lines {
            let policy = Policy::new(line, logger_handle.clone())?;
            hash_map_policy.insert(line as u8, policy);
        }

        Ok(Self {
            policys: hash_map_policy,
            idle_governor,
            original_idle_governor,
        })
    }

    fn get_policy(&mut self, index: u8) -> io::Result<&mut Policy> {
        self.policys.get_mut(&index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("CPUFreq policy{index} is not initialized"),
            )
        })
    }

    pub fn write_index_freq(&mut self, index: u8, values: (u32, u32)) -> io::Result<()> {
        self.get_policy(index)?.write_limits(values)
    }

    pub fn write_index_governor(&mut self, index: u8, value: &str) -> io::Result<()> {
        self.get_policy(index)?.write_governor(value)
    }

    pub fn write_idle_governor(&mut self, value: &str) -> io::Result<()> {
        if let Some(s) = self.idle_governor.clone() {
            let mut file = OpenOptions::new().write(true).open(s)?;
            write_bytes(&mut file, value.as_bytes())?;
        }
        Ok(())
    }

    pub fn restore_hardware_limits(&mut self) -> io::Result<()> {
        for policy in self.policys.values_mut() {
            let limits = policy.hardware_limits();
            policy.write_limits(limits)?;
        }
        Ok(())
    }

    pub fn restore_hardware_state(&mut self) -> io::Result<()> {
        for policy in self.policys.values_mut() {
            policy.restore_hardware_state()?;
        }

        if let (Some(path), Some(governor)) = (
            self.idle_governor.as_deref(),
            self.original_idle_governor.as_deref(),
        ) {
            let mut file = OpenOptions::new().write(true).open(path)?;
            write_bytes(&mut file, governor.as_bytes())?;
        }
        Ok(())
    }
}

pub struct Policy {
    max_freq: File,
    min_freq: File,
    governor: File,
    hardware_min: u32,
    hardware_max: u32,
    original_governor: String,
    available_frequencies: Vec<u32>,
}

impl Policy {
    pub fn new(
        index: u32,
        _logger_handle: Arc<Mutex<crate::logger::Logger>>,
    ) -> io::Result<Self> {
        let max_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/scaling_max_freq",
            index
        );
        let min_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/scaling_min_freq",
            index
        );

        let governor_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/scaling_governor",
            index
        );
        let hardware_min_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/cpuinfo_min_freq",
            index
        );
        let hardware_max_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/cpuinfo_max_freq",
            index
        );

        for path in [&max_path, &min_path, &governor_path] {
            utils::set_file_permissions_numeric(path, 0o666)
                .map_err(|error| with_path("set permissions on", path, error))?;
        }

        let max_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&max_path)
            .map_err(|error| with_path("open", &max_path, error))?;
        let min_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&min_path)
            .map_err(|error| with_path("open", &min_path, error))?;
        let governor_file = OpenOptions::new()
            .write(true)
            .open(&governor_path)
            .map_err(|error| with_path("open", &governor_path, error))?;

        let hardware_min = read_frequency_path(&hardware_min_path)?;
        let hardware_max = read_frequency_path(&hardware_max_path)?;
        let original_governor = read_text_path(&governor_path)?;
        let available_path = format!(
            "/sys/devices/system/cpu/cpufreq/policy{}/scaling_available_frequencies",
            index
        );
        let mut available_frequencies = read_text_path(&available_path)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|value| value.parse::<u32>().ok())
            .filter(|value| (hardware_min..=hardware_max).contains(value))
            .collect::<Vec<_>>();
        available_frequencies.sort_unstable();
        available_frequencies.dedup();

        Ok(Self {
            max_freq: max_file,
            min_freq: min_file,
            governor: governor_file,
            hardware_min,
            hardware_max,
            original_governor,
            available_frequencies,
        })
    }

    fn write_limits(&mut self, target: (u32, u32)) -> io::Result<()> {
        let current = (read_value(&mut self.min_freq)?, read_value(&mut self.max_freq)?);
        for write in plan_limit_writes(current, target)? {
            match write {
                LimitWrite::Min(value) => write_value(&mut self.min_freq, value)?,
                LimitWrite::Max(value) => write_value(&mut self.max_freq, value)?,
            }
        }
        Ok(())
    }

    fn write_governor(&mut self, value: &str) -> io::Result<()> {
        write_bytes(&mut self.governor, value.as_bytes())
    }

    pub fn read_max(&mut self) -> io::Result<u32> {
        read_value(&mut self.max_freq)
    }

    pub fn read_min(&mut self) -> io::Result<u32> {
        read_value(&mut self.min_freq)
    }

    pub const fn hardware_limits(&self) -> (u32, u32) {
        (self.hardware_min, self.hardware_max)
    }

    pub fn round_up_frequency(&self, target: u32) -> u32 {
        self.available_frequencies
            .iter()
            .copied()
            .find(|frequency| *frequency >= target)
            .or_else(|| self.available_frequencies.last().copied())
            .unwrap_or(target)
    }

    fn restore_hardware_state(&mut self) -> io::Result<()> {
        let limits = self.hardware_limits();
        self.write_limits(limits)?;
        let governor = self.original_governor.clone();
        self.write_governor(&governor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "finalizer-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn raises_max_before_min_when_target_exceeds_current_max() {
        assert_eq!(
            plan_limit_writes((384_000, 960_000), (1_996_800, 1_996_800)).unwrap(),
            vec![
                LimitWrite::Max(1_996_800),
                LimitWrite::Min(1_996_800),
            ]
        );
    }

    #[test]
    fn lowers_min_before_max_when_reducing_range() {
        assert_eq!(
            plan_limit_writes((1_996_800, 1_996_800), (384_000, 960_000)).unwrap(),
            vec![LimitWrite::Min(384_000), LimitWrite::Max(960_000)]
        );
    }

    #[test]
    fn rejects_inverted_target_limits() {
        assert!(plan_limit_writes((384_000, 960_000), (960_000, 384_000)).is_err());
    }

    #[test]
    fn repeated_writes_replace_previous_value() {
        let path = temp_path("freq");
        std::fs::write(&path, "1000").unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        write_value(&mut file, 2000).unwrap();
        write_value(&mut file, 3000).unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "3000");
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_reads_reset_each_file_offset() {
        let min_path = temp_path("min");
        let max_path = temp_path("max");
        std::fs::write(&min_path, "384000").unwrap();
        std::fs::write(&max_path, "3532800").unwrap();
        let mut min_file = File::open(&min_path).unwrap();
        let mut max_file = File::open(&max_path).unwrap();

        assert_eq!(read_value(&mut max_file).unwrap(), 3_532_800);
        assert_eq!(read_value(&mut min_file).unwrap(), 384_000);
        assert_eq!(read_value(&mut max_file).unwrap(), 3_532_800);
        assert_eq!(read_value(&mut min_file).unwrap(), 384_000);

        drop(min_file);
        drop(max_file);
        std::fs::remove_file(min_path).unwrap();
        std::fs::remove_file(max_path).unwrap();
    }
}
