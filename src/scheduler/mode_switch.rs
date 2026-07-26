use std::{
    fs::read_to_string,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

use crate::{
    config::data,
    scheduler::{manager, state},
    utils,
};

pub struct ModeSwitch {
    mode_path: String,
    mode: Arc<AtomicUsize>,
    tx: mpsc::Sender<manager::Event>,
    onf: Arc<AtomicBool>,
    is_game: Arc<AtomicBool>,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
}

impl ModeSwitch {
    pub fn new(
        mode_path: String,
        mode: Arc<AtomicUsize>,
        tx: mpsc::Sender<manager::Event>,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        onf: Arc<AtomicBool>,
        is_game: Arc<AtomicBool>,
    ) -> Self {
        Self {
            mode_path,
            mode,
            tx,
            onf,
            is_game,
            logger_handle,
        }
    }

    fn process_mode_file(&mut self, current: &mut Option<data::RuntimeMode>) {
        let content = match read_to_string(&self.mode_path) {
            Ok(content) => content,
            Err(error) => {
                if let Ok(mut log) = self.logger_handle.lock() {
                    log.warn(format!("Failed to read mode file: {error}"));
                }
                return;
            }
        };

        let Some(next) = data::RuntimeMode::parse(&content) else {
            if let Ok(mut log) = self.logger_handle.lock() {
                log.warn(format!("Unknown mode: {}", content.trim()));
            }
            return;
        };

        if *current == Some(next) {
            return;
        }

        if let Ok(mut log) = self.logger_handle.lock() {
            let previous = match *current {
                Some(mode) => mode.name(),
                None => "uninitialized",
            };
            log.info(format!("模式切换: {previous} -> {}", next.name()));
        }

        *current = Some(next);
        self.mode.store(next.index(), Ordering::Relaxed);

        if let Some(event) = state::mode_transition(
            self.onf.load(Ordering::Relaxed),
            self.is_game.load(Ordering::Relaxed),
            next,
        ) && let Err(error) = self.tx.send(event)
            && let Ok(mut log) = self.logger_handle.lock()
        {
            log.warn(format!("Failed to send mode transition event: {error}"));
        }
    }

    pub fn start_loop(&mut self) {
        let mut inotify = utils::inotify_init(&self.mode_path);
        let mut current = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed));
        self.process_mode_file(&mut current);

        loop {
            utils::inotify_blockage(&mut inotify);
            self.process_mode_file(&mut current);
        }
    }
}
