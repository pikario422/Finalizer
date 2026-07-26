use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

pub fn set_file_permissions_numeric(file_path: &str, mode: u32) -> io::Result<()> {
    let metadata = fs::metadata(file_path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(mode);
    fs::set_permissions(file_path, permissions)
}

pub fn inotify_init(path: &str) -> io::Result<inotify::Inotify> {
    let inotify = inotify::Inotify::init()?;
    inotify
        .watches()
        .add(path, inotify::WatchMask::MODIFY)?;
    Ok(inotify)
}

pub fn inotify_blockage(inotify: &mut inotify::Inotify) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    loop {
        match inotify.read_events(&mut buffer) {
            Ok(events) => {
                #[allow(clippy::never_loop)]
                for _ in events {
                    break;
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

fn run_dumpsys(service: &str) -> io::Result<String> {
    let result = Command::new("dumpsys").arg(service).output()?;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "dumpsys {service} exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn parse_screen_status(output: &str) -> Option<bool> {
    let mut legacy_value = None;
    for line in output.lines() {
        for key in ["mHoldingDisplaySuspendBlocker=", "mScreenOn="] {
            if let Some((_, value)) = line.split_once(key) {
                let is_on = value.trim_start().starts_with("true");
                legacy_value = Some(legacy_value.unwrap_or(false) || is_on);
            }
        }
    }
    if legacy_value.is_some() {
        return legacy_value;
    }

    for line in output.lines() {
        if let Some((_, value)) = line.split_once("mWakefulness=") {
            return Some(value.trim_start().starts_with("Awake"));
        }
        if let Some((_, value)) = line.split_once("Display Power: state=") {
            return Some(value.trim_start().starts_with("ON"));
        }
    }
    None
}

pub fn monitor_screen_status() -> io::Result<bool> {
    let output = run_dumpsys("power")?;
    parse_screen_status(&output).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "dumpsys power did not contain a recognized screen state",
        )
    })
}

fn parse_current_focus(output: &str) -> String {
    output
        .lines()
        .find(|line| line.contains("mCurrentFocus=") && !line.contains("mCurrentFocus=null"))
        .or_else(|| output.lines().find(|line| line.contains("mFocusedApp=")))
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn get_now_top_window_pkg_name() -> io::Result<String> {
    let output = run_dumpsys("window")?;
    Ok(parse_current_focus(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_screen_state() {
        assert_eq!(
            parse_screen_status(
                "mHoldingDisplaySuspendBlocker=false\n  mScreenOn=true\n"
            ),
            Some(true)
        );
        assert_eq!(
            parse_screen_status(
                "mHoldingDisplaySuspendBlocker=false\n  mScreenOn=false\n"
            ),
            Some(false)
        );
    }

    #[test]
    fn parses_modern_screen_state() {
        assert_eq!(parse_screen_status("mWakefulness=Awake\n"), Some(true));
        assert_eq!(parse_screen_status("mWakefulness=Asleep\n"), Some(false));
    }

    #[test]
    fn extracts_current_focus_line() {
        let output = "other data\n  mCurrentFocus=Window{abc u0 com.example/.Main}\n";
        assert_eq!(
            parse_current_focus(output),
            "mCurrentFocus=Window{abc u0 com.example/.Main}"
        );
    }
}
