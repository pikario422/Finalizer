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

pub struct GameMoniter {
    is_game: Arc<AtomicBool>,
    onf: Arc<AtomicBool>,
    mode: Arc<AtomicUsize>,
    game_list: data::GameList,
    tx: mpsc::Sender<manager::Event>,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
}

impl GameMoniter {
    pub fn new(
        is_game: Arc<AtomicBool>,
        onf: Arc<AtomicBool>,
        mode: Arc<AtomicUsize>,
        game_list: data::GameList,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        tx: mpsc::Sender<manager::Event>,
    ) -> Self {
        Self {
            is_game,
            onf,
            mode,
            game_list,
            tx,
            logger_handle,
        }
    }

    pub fn start_loop(&mut self) {
        loop {
            let current_window = utils::get_now_top_window_pkg_name();
            let matched_entry = state::is_whitelisted(&current_window, &self.game_list);
            let current = matched_entry.is_some();
            let previous = self.is_game.swap(current, Ordering::Relaxed);

            if previous != current {
                if let Ok(mut log) = self.logger_handle.lock() {
                    match matched_entry {
                        Some(entry) => log.info(format!(
                            "Entered whitelisted app {}:{}; Finalizer scheduling suspended",
                            entry.name, entry.pkg
                        )),
                        None => log.info(
                            "Exited whitelisted app; Finalizer scheduling resumed".to_string(),
                        ),
                    }
                }

                let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                    .unwrap_or(data::RuntimeMode::Power);
                if let Some(event) = state::game_transition(
                    previous,
                    current,
                    self.onf.load(Ordering::Relaxed),
                    mode,
                ) && let Err(error) = self.tx.send(event)
                    && let Ok(mut log) = self.logger_handle.lock()
                {
                    log.warn(format!("Failed to send game transition event: {error}"));
                }
            }

            std::thread::sleep(Duration::from_secs(5));
        }
    }
}
