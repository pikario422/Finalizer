use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use crate::{
    config::data,
    scheduler::{manager, state},
    utils,
};

pub struct Moniter {
    onf: Arc<AtomicBool>,
    is_game: Arc<AtomicBool>,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
    mode: Arc<AtomicUsize>,
    game_profile: Arc<AtomicUsize>,
    tx: mpsc::Sender<manager::Event>,
}

impl Moniter {
    pub fn new(
        onf: Arc<AtomicBool>,
        is_game: Arc<AtomicBool>,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        mode: Arc<AtomicUsize>,
        game_profile: Arc<AtomicUsize>,
        tx: mpsc::Sender<manager::Event>,
    ) -> Self {
        Self {
            onf,
            is_game,
            logger_handle,
            mode,
            game_profile,
            tx,
        }
    }

    pub fn start_loop(&mut self) {
        let mut previous_status = Some(self.onf.load(Ordering::Relaxed));

        loop {
            let screen_status = utils::monitor_screen_status();
            let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                .unwrap_or(data::RuntimeMode::Power);

            if let Some(event) = state::screen_transition(
                previous_status,
                screen_status,
                self.is_game.load(Ordering::Relaxed),
                mode,
                self.game_profile.load(Ordering::Relaxed),
            ) {
                if let Ok(mut log) = self.logger_handle.lock() {
                    let previous = if previous_status == Some(true) {
                        "亮屏"
                    } else {
                        "熄屏"
                    };
                    let current = if screen_status { "亮屏" } else { "熄屏" };
                    log.info(format!("屏幕状态: {previous} -> {current}"));
                }

                self.onf.store(screen_status, Ordering::Relaxed);
                if let Err(error) = self.tx.send(event)
                    && let Ok(mut log) = self.logger_handle.lock()
                {
                    log.warn(format!("Failed to send screen transition event: {error}"));
                }
                previous_status = Some(screen_status);
            }

            std::thread::sleep(Duration::from_secs(5));
        }
    }
}
