use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    config::data::Config,
    logger::{LogLevel, Logger},
};

pub struct LogConfigMonitor {
    config_path: String,
    logger_handle: Arc<Mutex<Logger>>,
}

impl LogConfigMonitor {
    pub fn new(config_path: String, logger_handle: Arc<Mutex<Logger>>) -> Self {
        Self {
            config_path,
            logger_handle,
        }
    }

    pub fn start_loop(&mut self) {
        let mut current_level = self
            .logger_handle
            .lock()
            .map(|logger| logger.level())
            .unwrap_or(LogLevel::Info);
        let mut last_error = None;

        loop {
            std::thread::sleep(Duration::from_secs(1));

            let result = Config::new(&self.config_path).and_then(|config| {
                LogLevel::parse(&config.log.level).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid log level: {}", config.log.level),
                    )
                })
            });

            match result {
                Ok(next_level) => {
                    last_error = None;
                    if next_level == current_level {
                        continue;
                    }

                    if let Ok(mut logger) = self.logger_handle.lock() {
                        logger.set_level(next_level);
                        logger.info(format!(
                            "日志级别已更新: {} -> {}",
                            current_level.name(),
                            next_level.name()
                        ));
                    }
                    current_level = next_level;
                }
                Err(error) => {
                    let message = error.to_string();
                    if last_error.as_deref() != Some(message.as_str()) {
                        if let Ok(mut logger) = self.logger_handle.lock() {
                            logger.warn(format!("重新读取日志配置失败: {message}"));
                        }
                        last_error = Some(message);
                    }
                }
            }
        }
    }
}
