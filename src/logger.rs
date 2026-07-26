use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
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
    previous_path: PathBuf,
    level: LogLevel,
    max_size: u64,
    size: u64,
    file: Option<File>,
}

impl Logger {
    pub fn new(path: &str) -> Self {
        let path = PathBuf::from(path);
        let previous_path = path.with_extension("previous.log");
        let size = fs::metadata(&path).map(|metadata| metadata.len()).unwrap_or(0);
        Self {
            path,
            previous_path,
            level: LogLevel::Info,
            max_size: 512 * 1024,
            size,
            file: None,
        }
    }

    pub fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    pub const fn level(&self) -> LogLevel {
        self.level
    }

    pub fn start_session(&mut self) -> io::Result<()> {
        self.ensure_parent()?;
        if self.size > 0 {
            self.rotate()
        } else {
            self.open_file()
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

    fn write(&mut self, level: LogLevel, message: &str) {
        if level > self.level {
            return;
        }

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("[{timestamp}] [{}] {message}\n", level.label());
        let line_size = line.len() as u64;

        if self.file.is_none() && self.open_file().is_err() {
            return;
        }
        if let Some(file) = self.file.as_ref()
            && let Ok(metadata) = file.metadata()
        {
            self.size = metadata.len();
        }
        if self.size > 0
            && self.size.saturating_add(line_size) > self.max_size
            && self.rotate().is_err()
        {
            return;
        }

        let Some(file) = self.file.as_mut() else {
            return;
        };
        if file.write_all(line.as_bytes()).is_ok() {
            self.size = self.size.saturating_add(line_size);
        }
    }

    fn ensure_parent(&self) -> std::io::Result<()> {
        match self.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
            _ => Ok(()),
        }
    }

    fn open_file(&mut self) -> io::Result<()> {
        self.ensure_parent()?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        match fs::remove_file(&self.previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match fs::rename(&self.path, &self.previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.size = 0;
        self.open_file()
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

    fn clean(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(path.with_extension("previous.log"));
    }

    #[test]
    fn start_session_preserves_previous_log() {
        let path = test_path("session");
        clean(&path);
        fs::write(&path, "old log\n").unwrap();

        let mut logger = logger_for(&path);
        logger.start_session().unwrap();
        logger.info("new log".to_string());

        assert!(fs::read_to_string(&path).unwrap().contains("new log"));
        assert_eq!(
            fs::read_to_string(path.with_extension("previous.log")).unwrap(),
            "old log\n"
        );
        clean(&path);
    }

    #[test]
    fn default_level_writes_info_and_above() {
        let path = test_path("levels");
        clean(&path);
        let mut logger = logger_for(&path);
        logger.start_session().unwrap();

        logger.info("started".to_string());
        logger.warn("limited".to_string());
        logger.error("failed".to_string());
        logger.debug("details".to_string());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[INFO] started"));
        assert!(content.contains("[WARN] limited"));
        assert!(content.contains("[ERROR] failed"));
        assert!(!content.contains("[DEBUG] details"));
        clean(&path);
    }

    #[test]
    fn debug_level_writes_debug_messages() {
        let path = test_path("debug");
        clean(&path);
        let mut logger = logger_for(&path);
        logger.start_session().unwrap();
        logger.set_level(LogLevel::Debug);

        logger.debug("details".to_string());

        assert!(fs::read_to_string(&path).unwrap().contains("[DEBUG] details"));
        clean(&path);
    }

    #[test]
    fn configured_level_filters_less_important_messages() {
        let path = test_path("filter");
        clean(&path);
        let mut logger = logger_for(&path);
        logger.start_session().unwrap();
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
        clean(&path);
    }

    #[test]
    fn rotates_when_size_limit_is_reached() {
        let path = test_path("rotate");
        clean(&path);
        let mut logger = logger_for(&path);
        logger.max_size = 80;
        logger.start_session().unwrap();

        logger.info("first message fills the current log".to_string());
        logger.info("second message triggers rotation".to_string());

        assert!(path.with_extension("previous.log").exists());
        assert!(fs::read_to_string(&path).unwrap().contains("second message"));
        clean(&path);
    }

    #[test]
    fn follows_external_log_truncation() {
        let path = test_path("truncate");
        clean(&path);
        let mut logger = logger_for(&path);
        logger.start_session().unwrap();
        logger.info("before clear".to_string());

        fs::write(&path, "").unwrap();
        logger.info("after clear".to_string());

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("before clear"));
        assert!(content.contains("after clear"));
        clean(&path);
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
