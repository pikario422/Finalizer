use std::{
    env,
    fs,
    process,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
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
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn request_shutdown(_signal: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, request_shutdown as libc::sighandler_t);
        libc::signal(libc::SIGTERM, request_shutdown as libc::sighandler_t);
    }
}

fn handle_cli() {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return;
    };

    if command != "--validate-config" {
        eprintln!("未知参数: {command}");
        process::exit(2);
    }
    let Some(path) = args.next() else {
        eprintln!("缺少配置文件路径");
        process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("参数过多");
        process::exit(2);
    }

    match data::Config::new(&path).and_then(|config| {
        config
            .validate()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }) {
        Ok(()) => {
            println!("配置有效");
            process::exit(0);
        }
        Err(error) => {
            eprintln!("配置无效: {error}");
            process::exit(1);
        }
    }
}

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
    handle_cli();
    install_signal_handlers();
    let mut log = logger::Logger::new(LOG_PATH);
    if let Err(error) = log.start_session() {
        eprintln!("初始化日志文件失败: {error}");
    }

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
    let game_list = match GameList::new(GAME_LIST_PATH) {
        Ok(list) => list,
        Err(error) => {
            log.error(format!("读取游戏列表失败: {error}"));
            return;
        }
    };
    if let Err(error) = game_list.validate() {
        log.error(format!("游戏列表校验失败: {error}"));
        return;
    }

    log.info(format!(
        "启动: 配置={} v{}, 作者={}, 日志={}",
        config.name.name,
        config.name.version,
        config.name.author,
        log_level.name()
    ));
    log.debug(format!("模块目录: {MODULE_PATH}"));

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
    let initial_screen_on = match utils::monitor_screen_status() {
        Ok(screen_on) => screen_on,
        Err(error) => {
            log.warn(format!("读取初始屏幕状态失败，暂按亮屏处理: {error}"));
            true
        }
    };
    let initial_window = match utils::get_now_top_window_pkg_name() {
        Ok(window) => window,
        Err(error) => {
            log.warn(format!("读取初始前台窗口失败: {error}"));
            String::new()
        }
    };
    let initial_game_entry = state::is_whitelisted(&initial_window, &game_list);
    let initial_is_game = initial_game_entry.is_some();
    let initial_game_profile = state::game_profile_index(initial_game_entry);
    log.debug(format!(
        "游戏检测初始窗口: {}",
        if initial_window.is_empty() {
            "<未获取>"
        } else {
            &initial_window
        }
    ));
    match initial_game_entry {
        Some(entry) => log.info(format!(
            "游戏检测已启动: 白名单={} 项，当前={} ({})，策略={}",
            game_list.listvalue.len(),
            entry.name,
            entry.pkg,
            entry.mode.as_deref().unwrap_or("hardware")
        )),
        None => log.info(format!(
            "游戏检测已启动: 白名单={} 项，当前前台未命中",
            game_list.listvalue.len()
        )),
    }

    let logger_handle = Arc::new(Mutex::new(log));
    let mode = Arc::new(AtomicUsize::new(initial_mode.index()));
    let onf = Arc::new(AtomicBool::new(initial_screen_on));
    let is_game = Arc::new(AtomicBool::new(initial_is_game));
    let game_profile = Arc::new(AtomicUsize::new(initial_game_profile));
    let (tx, rx) = mpsc::channel();

    let shutdown_tx = tx.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("signal_monitor".to_string())
        .spawn(move || {
            while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
            }
            let _ = shutdown_tx.send(manager::Event::Shutdown);
        })
    {
        log_thread_error(&logger_handle, "signal_monitor", &error);
        return;
    }

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
        initial_game_profile,
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
        game_profile.clone(),
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
        game_profile,
        GAME_LIST_PATH.to_string(),
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
