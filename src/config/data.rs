use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Power,
    Balance,
    Performance,
    Fast,
}

impl RuntimeMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "powersave" => Some(Self::Power),
            "balance" => Some(Self::Balance),
            "performance" => Some(Self::Performance),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Power => 0,
            Self::Balance => 1,
            Self::Performance => 2,
            Self::Fast => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Power => "powersave",
            Self::Balance => "balance",
            Self::Performance => "performance",
            Self::Fast => "fast",
        }
    }

    pub const fn from_index(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::Power),
            1 => Some(Self::Balance),
            2 => Some(Self::Performance),
            3 => Some(Self::Fast),
            _ => None,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub name: Name,
    #[serde(default)]
    pub log: Log,
    pub policy: Vec<Policy>,
    pub mode: Mode,
}

pub struct ModePolicyRef<'a> {
    pub idle_governor: &'a str,
    pub policy: &'a [MPolicy],
}

impl Config {
    pub fn mode_policy(&self, mode: RuntimeMode) -> ModePolicyRef<'_> {
        match mode {
            RuntimeMode::Power => ModePolicyRef {
                idle_governor: &self.mode.power.idle_governor,
                policy: &self.mode.power.policy,
            },
            RuntimeMode::Balance => ModePolicyRef {
                idle_governor: &self.mode.blan.idle_governor,
                policy: &self.mode.blan.policy,
            },
            RuntimeMode::Performance => ModePolicyRef {
                idle_governor: &self.mode.perf.idle_governor,
                policy: &self.mode.perf.policy,
            },
            RuntimeMode::Fast => ModePolicyRef {
                idle_governor: &self.mode.fast.idle_governor,
                policy: &self.mode.fast.policy,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if crate::logger::LogLevel::parse(&self.log.level).is_none() {
            return Err(format!(
                "invalid log level '{}'; expected error, warn, info, or debug",
                self.log.level
            ));
        }

        if self.policy.is_empty() {
            return Err("at least one CPU policy is required".to_string());
        }

        for (index, policy) in self.policy.iter().enumerate() {
            if policy.from > policy.to {
                return Err(format!("CPU policy {index} has from > to"));
            }

            for (other_index, other) in self.policy.iter().enumerate().skip(index + 1) {
                if policy.from <= other.to && other.from <= policy.to {
                    return Err(format!(
                        "CPU policies {index} and {other_index} overlap"
                    ));
                }
            }
        }

        for mode in [
            RuntimeMode::Power,
            RuntimeMode::Balance,
            RuntimeMode::Performance,
            RuntimeMode::Fast,
        ] {
            let set = self.mode_policy(mode);
            if set.policy.len() != self.policy.len() {
                return Err(format!(
                    "{} policy count {} does not match CPU policy count {}",
                    mode.name(),
                    set.policy.len(),
                    self.policy.len()
                ));
            }

            for (index, policy) in set.policy.iter().enumerate() {
                let invalid = policy.min_freq > policy.max_freq
                    || !(policy.min_freq..=policy.max_freq).contains(&policy.boost_freq)
                    || !(policy.min_freq..=policy.max_freq).contains(&policy.sleep_freq)
                    || policy.can_boost_freq > policy.boost_freq
                    || policy.delay == 0
                    || !policy.margin.is_finite()
                    || policy.margin <= 0.0;
                if invalid {
                    return Err(format!(
                        "{} policy {index} has invalid limits",
                        mode.name()
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Log {
    pub level: String,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Name {
    pub name: String,
    pub version: String,
    pub author: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Policy {
    pub from: u32,
    pub to: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Mode {
    pub power: Power,
    pub blan: Blan,
    pub perf: Perf,
    pub fast: Fast,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Power {
    pub idle_governor: String,
    pub policy: Vec<MPolicy>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Blan {
    pub idle_governor: String,
    pub policy: Vec<MPolicy>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Perf {
    pub idle_governor: String,
    pub policy: Vec<MPolicy>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Fast {
    pub idle_governor: String,
    pub policy: Vec<MPolicy>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct MPolicy {
    pub delay: u64,
    pub max_freq: u32,
    pub min_freq: u32,
    pub can_boost_freq: u32,
    pub boost_freq: u32,
    pub margin: f32,
    pub diff: u32,
    pub governor: String,
    pub sleep_freq: u32,
}

#[test]
fn test() {
    let content = std::fs::read_to_string("./debug/config.toml").unwrap();
    let config: Config = toml::from_str(&content).unwrap();

    println!("{:?}", config);
}

#[derive(Deserialize, Debug, Clone)]
pub struct GameList {
    pub listvalue: Vec<ListValue>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ListValue {
    pub pkg: String,
    pub name: String,
}

#[test]
fn testgame() {
    let content = std::fs::read_to_string("./debug/game_list.toml").unwrap();
    let config: GameList = toml::from_str(&content).unwrap();

    println!("{:?}", config);
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    fn sd8e_config() -> Config {
        Config::new("./mode/config/config.toml").expect("SD8e config must parse")
    }

    #[test]
    fn parses_runtime_modes() {
        assert_eq!(RuntimeMode::parse("powersave"), Some(RuntimeMode::Power));
        assert_eq!(RuntimeMode::parse("balance\n"), Some(RuntimeMode::Balance));
        assert_eq!(
            RuntimeMode::parse("performance"),
            Some(RuntimeMode::Performance)
        );
        assert_eq!(RuntimeMode::parse("fast"), Some(RuntimeMode::Fast));
        assert_eq!(RuntimeMode::parse("unknown"), None);
    }

    #[test]
    fn validates_sd8e_config() {
        sd8e_config()
            .validate()
            .expect("SD8e config must validate");
    }

    #[test]
    fn defaults_to_info_when_log_section_is_missing() {
        let content = std::fs::read_to_string("./mode/config/config.toml").unwrap();
        let content = content.replacen("[log]", "[ignored_log]", 1);
        let config: Config = toml::from_str(&content).unwrap();

        assert_eq!(config.log.level, "info");
    }

    #[test]
    fn rejects_unknown_log_level() {
        let mut config = sd8e_config();
        config.log.level = "trace".to_string();

        assert!(config.validate().unwrap_err().contains("invalid log level"));
    }

    #[test]
    fn rejects_mismatched_mode_policy_count() {
        let mut config = sd8e_config();
        config.mode.fast.policy.pop();
        let error = config.validate().unwrap_err();
        assert!(error.contains("fast policy count"));
    }

    #[test]
    fn rejects_overlapping_cpu_ranges() {
        let mut config = sd8e_config();
        config.policy[1].from = 5;
        let error = config.validate().unwrap_err();
        assert!(error.contains("overlap"));
    }

    #[test]
    fn rejects_invalid_frequency_bounds() {
        let mut config = sd8e_config();
        config.mode.power.policy[0].min_freq = 3_000_000;
        let error = config.validate().unwrap_err();
        assert!(error.contains("powersave policy 0"));
    }
}
