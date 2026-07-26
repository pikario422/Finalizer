use crate::{
    config::data::{GameList, ListValue, RuntimeMode},
    scheduler::manager::Event,
};

pub const HARDWARE_GAME_PROFILE: usize = 4;

pub fn game_profile_index(entry: Option<&ListValue>) -> usize {
    entry
        .and_then(|entry| entry.mode.as_deref())
        .and_then(|mode| RuntimeMode::parse(&mode.trim().to_ascii_lowercase()))
        .map(RuntimeMode::index)
        .unwrap_or(HARDWARE_GAME_PROFILE)
}

fn game_profile_event(profile: usize) -> Event {
    RuntimeMode::from_index(profile)
        .map(Event::ApplyMode)
        .unwrap_or(Event::RestoreHardware)
}

pub fn is_whitelisted<'a>(current_window: &str, list: &'a GameList) -> Option<&'a ListValue> {
    list.listvalue
        .iter()
        .find(|entry| {
            let package = entry.pkg.trim();
            !package.is_empty() && current_window.contains(package)
        })
}

pub fn event_for_state(
    screen_on: bool,
    is_game: bool,
    mode: RuntimeMode,
    game_profile: usize,
) -> Event {
    if !screen_on {
        Event::ApplySleep(mode)
    } else if is_game {
        game_profile_event(game_profile)
    } else {
        Event::ApplyMode(mode)
    }
}

pub fn game_transition(
    previous: bool,
    current: bool,
    screen_on: bool,
    mode: RuntimeMode,
    game_profile: usize,
    profile_changed: bool,
) -> Option<Event> {
    if (previous == current && !profile_changed) || !screen_on {
        None
    } else if current {
        Some(game_profile_event(game_profile))
    } else {
        Some(Event::ApplyMode(mode))
    }
}

pub fn screen_transition(
    previous: Option<bool>,
    current: bool,
    is_game: bool,
    mode: RuntimeMode,
    game_profile: usize,
) -> Option<Event> {
    if previous == Some(current) {
        None
    } else {
        Some(event_for_state(current, is_game, mode, game_profile))
    }
}

pub fn mode_transition(screen_on: bool, is_game: bool, mode: RuntimeMode) -> Option<Event> {
    (screen_on && !is_game).then_some(Event::ApplyMode(mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::data::{GameList, ListValue, RuntimeMode};
    use crate::scheduler::manager::Event;

    #[test]
    fn whitelist_matches_any_entry_regardless_of_order() {
        let list = GameList {
            listvalue: vec![
                ListValue {
                    pkg: "first.pkg".into(),
                    name: "First".into(),
                    mode: None,
                },
                ListValue {
                    pkg: "target.pkg".into(),
                    name: "Target".into(),
                    mode: None,
                },
                ListValue {
                    pkg: "last.pkg".into(),
                    name: "Last".into(),
                    mode: None,
                },
            ],
        };
        assert!(is_whitelisted("Window{ target.pkg/Main }", &list).is_some());
    }

    #[test]
    fn whitelist_ignores_empty_package_names() {
        let list = GameList {
            listvalue: vec![ListValue {
                pkg: "   ".into(),
                name: "Empty".into(),
                mode: None,
            }],
        };
        assert!(is_whitelisted("Window{ regular.pkg/Main }", &list).is_none());
    }

    #[test]
    fn game_entry_restores_hardware_only_when_screen_is_on() {
        assert_eq!(
            game_transition(
                false,
                true,
                true,
                RuntimeMode::Balance,
                HARDWARE_GAME_PROFILE,
                false,
            ),
            Some(Event::RestoreHardware)
        );
        assert_eq!(
            game_transition(
                false,
                true,
                false,
                RuntimeMode::Balance,
                HARDWARE_GAME_PROFILE,
                false,
            ),
            None
        );
    }

    #[test]
    fn game_exit_applies_latest_mode() {
        assert_eq!(
            game_transition(
                true,
                false,
                true,
                RuntimeMode::Fast,
                HARDWARE_GAME_PROFILE,
                false,
            ),
            Some(Event::ApplyMode(RuntimeMode::Fast))
        );
    }

    #[test]
    fn wake_restores_hardware_for_game() {
        assert_eq!(
            screen_transition(
                Some(false),
                true,
                true,
                RuntimeMode::Power,
                HARDWARE_GAME_PROFILE,
            ),
            Some(Event::RestoreHardware)
        );
    }

    #[test]
    fn first_off_sample_applies_sleep() {
        assert_eq!(
            screen_transition(
                None,
                false,
                false,
                RuntimeMode::Performance,
                HARDWARE_GAME_PROFILE,
            ),
            Some(Event::ApplySleep(RuntimeMode::Performance))
        );
    }

    #[test]
    fn mode_change_is_deferred_during_game() {
        assert_eq!(mode_transition(true, true, RuntimeMode::Fast), None);
        assert_eq!(
            mode_transition(true, false, RuntimeMode::Fast),
            Some(Event::ApplyMode(RuntimeMode::Fast))
        );
    }

    #[test]
    fn configured_game_profile_applies_selected_mode() {
        assert_eq!(
            game_transition(
                false,
                true,
                true,
                RuntimeMode::Power,
                RuntimeMode::Performance.index(),
                false,
            ),
            Some(Event::ApplyMode(RuntimeMode::Performance))
        );
    }

    #[test]
    fn game_profile_is_trimmed_and_case_insensitive() {
        let entry = ListValue {
            pkg: "example.game".into(),
            name: "Example".into(),
            mode: Some(" Performance ".into()),
        };
        assert_eq!(
            game_profile_index(Some(&entry)),
            RuntimeMode::Performance.index()
        );
    }
}
