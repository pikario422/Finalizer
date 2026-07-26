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
    logger_handle: Arc<Mutex<logger::Logger>>,
    mode: Arc<AtomicUsize>,
    tx: mpsc::Sender<manager::Event>,
}

impl Moniter {
    pub fn new(
        onf: Arc<AtomicBool>,
        is_game: Arc<AtomicBool>,
        logger_handle: Arc<Mutex<logger::Logger>>,
        mode: Arc<AtomicUsize>,
        tx: mpsc::Sender<manager::Event>,
    ) -> Self {
        Self {
            onf,
            is_game,
            logger_handle,
            mode,
            tx,
        }
    }

    pub fn start_loop(&mut self) {
        let mut previous_status = None;

        loop {
            let screen_status = utils::monitor_screen_status();
            let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                .unwrap_or(data::RuntimeMode::Power);

            if let Some(event) = state::screen_transition(
                previous_status,
                screen_status,
                self.is_game.load(Ordering::Relaxed),
                mode,
            ) {
                if let Ok(mut log) = self.logger_handle.lock() {
                    log.info(format!(
                        "Screen state: {:?} -> {screen_status}",
                        previous_status
                    ));
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
