use std::collections::HashMap;

use crate::model::WindowInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppGroup {
    pub class: String,
    pub display_name: String,
    pub windows: Vec<WindowInfo>,
    /// Global hotkey for an app with no window to focus. `None` for windowed
    /// apps, whose rows are focused with Enter instead.
    pub shortcut: Option<String>,
}

impl AppGroup {
    pub fn latest_window(&self) -> Option<&WindowInfo> {
        self.windows
            .iter()
            .min_by_key(|window| window.focus_history_id)
    }

    pub fn searchable_text(&self) -> String {
        let mut text = format!("{} {}", self.display_name, self.class);
        for window in &self.windows {
            text.push(' ');
            text.push_str(&window.title);
        }
        if let Some(shortcut) = &self.shortcut {
            text.push(' ');
            text.push_str(shortcut);
        }
        text.to_lowercase()
    }
}

/// An app that runs without a window. `hyprctl clients` cannot report these, so
/// a running one would otherwise be missing from the applications tab entirely.
struct WindowlessApp {
    class: &'static str,
    shortcut: &'static str,
}

const WINDOWLESS_APPS: &[WindowlessApp] = &[WindowlessApp {
    // Neru grabs the keyboard through the evdev proxy instead of binding a
    // Hyprland shortcut, so nothing about it is discoverable from the outside.
    class: "neru",
    shortcut: "Ctrl + Shift + Space",
}];

/// Appends the windowless apps that are currently running.
///
/// They go last, which is where `group_windows` already sorts an app with no
/// window to focus.
pub fn append_running_windowless_apps(apps: &mut Vec<AppGroup>) {
    let additions: Vec<AppGroup> = WINDOWLESS_APPS
        .iter()
        .filter(|app| !apps.iter().any(|group| group.class == app.class))
        .filter(|app| process_is_running(app.class))
        .map(windowless_app_group)
        .collect();
    apps.extend(additions);
}

fn windowless_app_group(app: &WindowlessApp) -> AppGroup {
    AppGroup {
        class: app.class.to_owned(),
        display_name: display_name_for_class(app.class),
        windows: Vec::new(),
        shortcut: Some(app.shortcut.to_owned()),
    }
}

/// Returns whether a process with this exact name is running, by reading
/// `/proc/<pid>/comm`. A full /proc scan is cheap enough for a couple of names
/// and avoids spawning `pgrep` per lookup.
fn process_is_running(name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        std::fs::read_to_string(entry.path().join("comm")).is_ok_and(|comm| comm.trim() == name)
    })
}

pub fn group_windows(windows: Vec<WindowInfo>) -> Vec<AppGroup> {
    let mut grouped: HashMap<String, Vec<WindowInfo>> = HashMap::new();

    for window in windows {
        if !window.mapped || !window.accepts_input || window.class.trim().is_empty() {
            continue;
        }
        grouped
            .entry(window.class.clone())
            .or_default()
            .push(window);
    }

    let mut apps: Vec<AppGroup> = grouped
        .into_iter()
        .map(|(class, mut windows)| {
            windows.sort_by_key(|window| window.focus_history_id);
            let display_name = display_name_for_class(&class);
            AppGroup {
                class,
                display_name,
                windows,
                shortcut: None,
            }
        })
        .collect();

    apps.sort_by(|left, right| {
        let left_focus = left
            .latest_window()
            .map(|window| window.focus_history_id)
            .unwrap_or(i32::MAX);
        let right_focus = right
            .latest_window()
            .map(|window| window.focus_history_id)
            .unwrap_or(i32::MAX);
        left_focus
            .cmp(&right_focus)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });

    apps
}

pub fn filter_apps(apps: &[AppGroup], query: &str) -> Vec<AppGroup> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return apps.to_vec();
    }

    apps.iter()
        .filter(|app| app.searchable_text().contains(&query))
        .cloned()
        .collect()
}

pub fn display_name_for_class(class: &str) -> String {
    let lower_class = class.to_ascii_lowercase();
    match lower_class.as_str() {
        "firefox" | "firefox-esr" | "firefoxdeveloperedition" => "Firefox".to_owned(),
        "opencode" => "OpenCode".to_owned(),
        "kitty" | "kitty.desktop" => "Kitty".to_owned(),
        "org.kde.kate" => "Kate".to_owned(),
        "org.kde.dolphin" => "Dolphin".to_owned(),
        "org.kde.konsole" => "Konsole".to_owned(),
        "code" | "code-url-handler" => "Visual Studio Code".to_owned(),
        "discord" => "Discord".to_owned(),
        "spotify" => "Spotify".to_owned(),
        _ => friendly_class_name(class),
    }
}

fn friendly_class_name(class: &str) -> String {
    let last_component = class.rsplit('.').next().unwrap_or(class).trim();
    if last_component.is_empty() {
        return class.to_owned();
    }

    let mut name = String::new();
    let mut previous_was_separator = false;
    for character in last_component.chars() {
        if character == '_' || character == '-' {
            if !previous_was_separator && !name.is_empty() {
                name.push(' ');
            }
            previous_was_separator = true;
        } else {
            previous_was_separator = false;
            name.push(character);
        }
    }

    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => class.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_process_is_not_reported_as_running() {
        assert!(!process_is_running("neru-not-a-real-process"));
    }

    #[test]
    fn windowless_row_carries_a_searchable_shortcut() {
        let app = windowless_app_group(&WINDOWLESS_APPS[0]);

        assert_eq!(app.display_name, "Neru");
        assert!(app.windows.is_empty());
        assert!(app.latest_window().is_none());
        assert!(app.searchable_text().contains("ctrl + shift + space"));
    }
}
