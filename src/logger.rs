use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

use chrono::Local;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

pub struct Logger {
    path: PathBuf,
    level: LogLevel,
}

impl Logger {
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
            level: LogLevel::Info,
        }
    }

    pub fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    pub const fn level(&self) -> LogLevel {
        self.level
    }

    pub fn clear(&mut self) {
        if self.ensure_parent().is_ok() {
            let _ = File::create(&self.path);
        }
    }

    pub fn info(&mut self, message: String) {
        self.write(LogLevel::Info, &message);
    }

    pub fn warn(&mut self, message: String) {
        self.write(LogLevel::Warn, &message);
    }

    pub fn error(&mut self, message: String) {
        self.write(LogLevel::Error, &message);
    }

    pub fn debug(&mut self, message: String) {
        self.write(LogLevel::Debug, &message);
    }

    fn write(&self, level: LogLevel, message: &str) {
        if level > self.level {
            return;
        }

        if self.ensure_parent().is_err() {
            return;
        }

        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{timestamp}] [{}] {message}", level.label());
    }

    fn ensure_parent(&self) -> std::io::Result<()> {
        match self.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("finalizer-logger-{name}-{}.log", std::process::id()))
    }

    fn logger_for(path: &Path) -> Logger {
        Logger::new(path.to_str().expect("temporary path must be valid UTF-8"))
    }

    #[test]
    fn clear_truncates_existing_log() {
        let path = test_path("clear");
        fs::write(&path, "old log\n").unwrap();

        logger_for(&path).clear();

        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn default_level_writes_info_and_above() {
        let path = test_path("levels");
        let mut logger = logger_for(&path);
        logger.clear();

        logger.info("started".to_string());
        logger.warn("limited".to_string());
        logger.error("failed".to_string());
        logger.debug("details".to_string());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[INFO] started"));
        assert!(content.contains("[WARN] limited"));
        assert!(content.contains("[ERROR] failed"));
        assert!(!content.contains("[DEBUG] details"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn debug_level_writes_debug_messages() {
        let path = test_path("debug");
        let mut logger = logger_for(&path);
        logger.clear();
        logger.set_level(LogLevel::Debug);

        logger.debug("details".to_string());

        assert!(fs::read_to_string(&path).unwrap().contains("[DEBUG] details"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn configured_level_filters_less_important_messages() {
        let path = test_path("filter");
        let mut logger = logger_for(&path);
        logger.clear();
        logger.set_level(LogLevel::Warn);

        logger.debug("debug".to_string());
        logger.info("info".to_string());
        logger.warn("warn".to_string());
        logger.error("error".to_string());

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("[DEBUG]"));
        assert!(!content.contains("[INFO]"));
        assert!(content.contains("[WARN] warn"));
        assert!(content.contains("[ERROR] error"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_supported_levels() {
        assert_eq!(LogLevel::parse("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::parse("warn"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse("debug\n"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("trace"), None);
    }
}
