use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize},
        mpsc,
    },
};

use finalizer::{
    config::data::{self, GameList, RuntimeMode},
    cpu_handle::cpu_stat::CpuStat,
    devices::touch,
    logger::{self, LogLevel},
    scheduler::{game_moniter, log_config, manager, mode_switch, screen_moniter, state},
    utils,
};

const MODULE_PATH: &str = "/data/adb/modules/SZE_FINALIZER";
const CONFIG_PATH: &str = "/data/adb/modules/SZE_FINALIZER/config/config.toml";
const GAME_LIST_PATH: &str = "/data/adb/modules/SZE_FINALIZER/config/game_list.toml";
const MODE_PATH: &str = "/data/adb/modules/SZE_FINALIZER/config/config.txt";
const LOG_PATH: &str = "/data/adb/modules/SZE_FINALIZER/log/log.log";

fn log_thread_error(
    logger_handle: &Arc<Mutex<logger::Logger>>,
    thread_name: &str,
    error: &std::io::Error,
) {
    if let Ok(mut log) = logger_handle.lock() {
        log.error(format!("线程 {thread_name} 启动失败: {error}"));
    }
}

fn log_touch_disabled(
    logger_handle: &Arc<Mutex<logger::Logger>>,
    error: &std::io::Error,
) {
    if let Ok(mut log) = logger_handle.lock() {
        log.warn(format!("触摸设备初始化失败，Touch Boost 已禁用: {error}"));
    }
}

fn main() {
    let mut log = logger::Logger::new(LOG_PATH);
    log.clear();

    let config = match data::Config::new(CONFIG_PATH) {
        Ok(config) => config,
        Err(error) => {
            log.error(format!("读取配置失败: {error}"));
            return;
        }
    };
    if let Err(error) = config.validate() {
        log.error(format!("配置校验失败: {error}"));
        return;
    }
    let Some(log_level) = LogLevel::parse(&config.log.level) else {
        log.error(format!("无效的日志级别: {}", config.log.level));
        return;
    };
    log.set_level(log_level);
    log.info("你好!感谢你使用SZE_FINALIZER".to_string());
    let game_list = match GameList::new(GAME_LIST_PATH) {
        Ok(list) => list,
        Err(error) => {
            log.error(format!("读取游戏列表失败: {error}"));
            return;
        }
    };

    log.info(format!("配置名:{}", config.name.name));
    log.info(format!("配置作者:{}", config.name.author));
    log.info(format!("配置版本:{}", config.name.version));
    log.info(format!("日志级别:{}", log_level.name()));
    log.info(format!("模块目录:{MODULE_PATH}"));

    let initial_mode = match fs::read_to_string(MODE_PATH)
        .ok()
        .and_then(|value| RuntimeMode::parse(&value))
    {
        Some(mode) => mode,
        None => {
            log.warn("无法读取有效初始模式，回退到 powersave".to_string());
            RuntimeMode::Power
        }
    };
    let initial_screen_on = utils::monitor_screen_status();
    let initial_window = utils::get_now_top_window_pkg_name();
    let initial_is_game = state::is_whitelisted(&initial_window, &game_list).is_some();

    let logger_handle = Arc::new(Mutex::new(log));
    let mode = Arc::new(AtomicUsize::new(initial_mode.index()));
    let onf = Arc::new(AtomicBool::new(initial_screen_on));
    let is_game = Arc::new(AtomicBool::new(initial_is_game));
    let (tx, rx) = mpsc::channel();

    let mut log_config_monitor =
        log_config::LogConfigMonitor::new(CONFIG_PATH.to_string(), logger_handle.clone());
    if let Err(error) = std::thread::Builder::new()
        .name("log_config".to_string())
        .spawn(move || log_config_monitor.start_loop())
    {
        log_thread_error(&logger_handle, "log_config", &error);
        return;
    }

    let mut manager = match manager::Manager::new(rx, logger_handle.clone(), config.clone()) {
        Ok(manager) => manager,
        Err(error) => {
            if let Ok(mut log) = logger_handle.lock() {
                log.error(format!("初始化 CPUFreq Manager 失败: {error}"));
            }
            return;
        }
    };

    let mut stats = Vec::with_capacity(config.policy.len());
    for (id, policy) in config.policy.iter().enumerate() {
        match CpuStat::new(
            id,
            policy.from,
            policy.to,
            tx.clone(),
            logger_handle.clone(),
            config.clone(),
            mode.clone(),
            onf.clone(),
            is_game.clone(),
        ) {
            Ok(stat) => stats.push(stat),
            Err(error) => {
                if let Ok(mut log) = logger_handle.lock() {
                    log.error(format!(
                        "初始化 CPU policy{} 监控失败: {error}",
                        policy.from
                    ));
                }
                return;
            }
        }
    }

    if let Err(error) = tx.send(state::event_for_state(
        initial_screen_on,
        initial_is_game,
        initial_mode,
    )) {
        if let Ok(mut log) = logger_handle.lock() {
            log.error(format!("发送初始调度状态失败: {error}"));
        }
        return;
    }

    for mut stat in stats {
        let thread_name = format!("cpu_policy_{}", stat.policy_id());
        if let Err(error) = std::thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || stat.start_send_event_loop())
        {
            log_thread_error(&logger_handle, &thread_name, &error);
            return;
        }
    }

    match touch::find_touchscreen_device() {
        Some(device) => match touch::Moniter::new(
            &device,
            tx.clone(),
            logger_handle.clone(),
            config.clone(),
            mode.clone(),
            onf.clone(),
            is_game.clone(),
        ) {
            Ok(mut monitor) => {
                if let Err(error) = std::thread::Builder::new()
                    .name("touch_listen".to_string())
                    .spawn(move || monitor.start_loop())
                {
                    if let Ok(mut log) = logger_handle.lock() {
                        log.warn(format!("Touch Boost 线程启动失败: {error}"));
                    }
                }
            }
            Err(error) => log_touch_disabled(&logger_handle, &error),
        },
        None => {
            if let Ok(mut log) = logger_handle.lock() {
                log.warn("未发现触摸设备，Touch Boost 已禁用".to_string());
            }
        }
    }

    let mut mode_monitor = mode_switch::ModeSwitch::new(
        MODE_PATH.to_string(),
        mode.clone(),
        tx.clone(),
        logger_handle.clone(),
        onf.clone(),
        is_game.clone(),
    );
    if let Err(error) = std::thread::Builder::new()
        .name("mode_switch".to_string())
        .spawn(move || mode_monitor.start_loop())
    {
        log_thread_error(&logger_handle, "mode_switch", &error);
        return;
    }

    let mut screen_monitor = screen_moniter::Moniter::new(
        onf.clone(),
        is_game.clone(),
        logger_handle.clone(),
        mode.clone(),
        tx.clone(),
    );
    if let Err(error) = std::thread::Builder::new()
        .name("screen_moniter".to_string())
        .spawn(move || screen_monitor.start_loop())
    {
        log_thread_error(&logger_handle, "screen_moniter", &error);
        return;
    }

    let mut game_monitor = game_moniter::GameMoniter::new(
        is_game,
        onf,
        mode,
        game_list,
        logger_handle.clone(),
        tx,
    );
    if let Err(error) = std::thread::Builder::new()
        .name("game_moniter".to_string())
        .spawn(move || game_monitor.start_loop())
    {
        log_thread_error(&logger_handle, "game_moniter", &error);
        return;
    }

    manager.start_loop();
}
