use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

use chrono::Local;

pub struct Logger {
    path: PathBuf,
}

impl Logger {
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
        }
    }

    pub fn clear(&mut self) {
        if self.ensure_parent().is_ok() {
            let _ = File::create(&self.path);
        }
    }

    pub fn info(&mut self, message: String) {
        self.write("INFO", &message);
    }

    pub fn warn(&mut self, message: String) {
        self.write("WARN", &message);
    }

    pub fn error(&mut self, message: String) {
        self.write("ERROR", &message);
    }

    fn write(&self, level: &str, message: &str) {
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
        let _ = writeln!(file, "[{timestamp}] [{level}] {message}");
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
    fn writes_all_supported_levels() {
        let path = test_path("levels");
        let mut logger = logger_for(&path);
        logger.clear();

        logger.info("started".to_string());
        logger.warn("limited".to_string());
        logger.error("failed".to_string());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[INFO] started"));
        assert!(content.contains("[WARN] limited"));
        assert!(content.contains("[ERROR] failed"));
        let _ = fs::remove_file(path);
    }
}
