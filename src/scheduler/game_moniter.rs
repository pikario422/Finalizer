use std::{
    fs,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime},
};

use crate::{
    config::data,
    scheduler::{manager, state},
    utils,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct GameListStamp {
    modified: SystemTime,
    len: u64,
}

fn game_list_stamp(path: &str) -> io::Result<GameListStamp> {
    let metadata = fs::metadata(path)?;
    Ok(GameListStamp {
        modified: metadata.modified()?,
        len: metadata.len(),
    })
}

fn load_game_list(path: &str) -> io::Result<data::GameList> {
    let game_list = data::GameList::new(path)?;
    game_list
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(game_list)
}

pub struct GameMoniter {
    is_game: Arc<AtomicBool>,
    onf: Arc<AtomicBool>,
    mode: Arc<AtomicUsize>,
    game_profile: Arc<AtomicUsize>,
    game_list_path: String,
    game_list_stamp: Option<GameListStamp>,
    game_list: data::GameList,
    tx: mpsc::Sender<manager::Event>,
    logger_handle: Arc<Mutex<crate::logger::Logger>>,
}

impl GameMoniter {
    pub fn new(
        is_game: Arc<AtomicBool>,
        onf: Arc<AtomicBool>,
        mode: Arc<AtomicUsize>,
        game_profile: Arc<AtomicUsize>,
        game_list_path: String,
        game_list: data::GameList,
        logger_handle: Arc<Mutex<crate::logger::Logger>>,
        tx: mpsc::Sender<manager::Event>,
    ) -> Self {
        Self {
            is_game,
            onf,
            mode,
            game_profile,
            game_list_stamp: game_list_stamp(&game_list_path).ok(),
            game_list_path,
            game_list,
            tx,
            logger_handle,
        }
    }

    fn reload_game_list(&mut self) -> io::Result<bool> {
        let stamp = game_list_stamp(&self.game_list_path)?;
        if self.game_list_stamp == Some(stamp) {
            return Ok(false);
        }

        self.game_list = load_game_list(&self.game_list_path)?;
        self.game_list_stamp = Some(stamp);
        Ok(true)
    }

    pub fn start_loop(&mut self) {
        let mut last_window_error = None;
        let mut last_reload_error = None;
        loop {
            match self.reload_game_list() {
                Ok(true) => {
                    last_reload_error = None;
                    if let Ok(mut log) = self.logger_handle.lock() {
                        log.info(format!(
                            "游戏白名单已重载: {} 项",
                            self.game_list.listvalue.len()
                        ));
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    let message = error.to_string();
                    if last_reload_error.as_deref() != Some(message.as_str()) {
                        if let Ok(mut log) = self.logger_handle.lock() {
                            log.warn(format!(
                                "游戏白名单重载失败，继续使用上一份配置: {message}"
                            ));
                        }
                        last_reload_error = Some(message);
                    }
                }
            }

            let current_window = match utils::get_now_top_window_pkg_name() {
                Ok(window) => {
                    last_window_error = None;
                    window
                }
                Err(error) => {
                    let message = error.to_string();
                    if last_window_error.as_deref() != Some(message.as_str()) {
                        if let Ok(mut log) = self.logger_handle.lock() {
                            log.warn(format!("读取前台窗口失败，保留当前游戏状态: {message}"));
                        }
                        last_window_error = Some(message);
                    }
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
            };
            let matched_entry = state::is_whitelisted(&current_window, &self.game_list);
            let current = matched_entry.is_some();
            let current_profile = state::game_profile_index(matched_entry);
            let previous_profile = self.game_profile.swap(current_profile, Ordering::Relaxed);
            let profile_changed = current && previous_profile != current_profile;
            let previous = self.is_game.swap(current, Ordering::Relaxed);

            if previous != current || profile_changed {
                if let Ok(mut log) = self.logger_handle.lock() {
                    match matched_entry {
                        Some(entry) => log.info(format!(
                            "进入游戏: {} ({})，策略={}",
                            entry.name,
                            entry.pkg,
                            entry.mode.as_deref().unwrap_or("hardware")
                        )),
                        None => log.info("退出游戏，恢复 Finalizer 调度".to_string()),
                    }
                }

                let mode = data::RuntimeMode::from_index(self.mode.load(Ordering::Relaxed))
                    .unwrap_or(data::RuntimeMode::Power);
                if let Some(event) = state::game_transition(
                    previous,
                    current,
                    self.onf.load(Ordering::Relaxed),
                    mode,
                    current_profile,
                    profile_changed,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "finalizer-game-list-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn loads_and_validates_reloaded_game_list() {
        let path = temp_path();
        fs::write(
            &path,
            "[[listvalue]]\npkg = \"com.example.game\"\nname = \"Example\"\nmode = \"performance\"\n",
        )
        .unwrap();

        let loaded = load_game_list(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.listvalue.len(), 1);

        fs::write(
            &path,
            "[[listvalue]]\npkg = \"com.example.game\"\nname = \"Example\"\nmode = \"turbo\"\n",
        )
        .unwrap();
        assert_eq!(
            load_game_list(path.to_str().unwrap())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(path).unwrap();
    }
}
