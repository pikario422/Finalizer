use std::fs;

use crate::config::data::{self, GameList};

use super::data::Config;

fn parse_toml<T: serde::de::DeserializeOwned>(path: &str) -> std::io::Result<T> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

impl Config {
    pub fn new(path: &str) -> std::io::Result<Self> {
        parse_toml(path)
    }

    pub fn get_name(&self) -> data::Name {
        self.name.clone()
    }

    pub fn get_policy(&self) -> Vec<data::Policy> {
        self.policy.clone()
    }

    pub fn get_power(&self) -> data::Power {
        self.mode.power.clone()
    }

    pub fn get_blan(&self) -> data::Blan {
        self.mode.blan.clone()
    }

    pub fn get_perf(&self) -> data::Perf {
        self.mode.perf.clone()
    }

    pub fn get_fast(&self) -> data::Fast {
        self.mode.fast.clone()
    }

    pub fn get_mode(&self) -> data::Mode {
        self.mode.clone()
    }
}

impl GameList {
    pub fn new(path: &str) -> std::io::Result<Self> {
        parse_toml(path)
    }
}
