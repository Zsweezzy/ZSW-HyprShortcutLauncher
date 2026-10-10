use serde::Deserialize;

const MOD_SHIFT: u32 = 1;
const MOD_CAPS: u32 = 2;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8;
const MOD_ALT2: u32 = 16;
const MOD_ALT_GR: u32 = 32;
const MOD_SUPER: u32 = 64;
const MOD_MOD3: u32 = 128;
const MOD_MOD5: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub modmask: u32,
    pub key: String,
    pub description: String,
    pub dispatcher: String,
    pub argument: String,
    pub locked: bool,
    pub repeat: bool,
    pub release: bool,
    pub non_consuming: bool,
}

impl Shortcut {
    pub fn new(
        modmask: u32,
        key: impl Into<String>,
        description: impl Into<String>,
        dispatcher: impl Into<String>,
        argument: impl Into<String>,
    ) -> Self {
        Self {
            modmask,
            key: key.into(),
            description: description.into(),
            dispatcher: dispatcher.into(),
            argument: argument.into(),
            locked: false,
            repeat: false,
            release: false,
            non_consuming: false,
        }
    }

    pub fn display_key(&self) -> String {
        let modifiers = format_modmask(self.modmask);
        let key = format_key_name(&self.key);
        if modifiers.is_empty() {
            key
        } else {
            format!("{modifiers} + {key}")
        }
    }

    pub fn action_label(&self) -> String {
        if !self.description.trim().is_empty() {
            return self.description.trim().to_owned();
        }

        if self.dispatcher.trim() == "__lua" {
            return "Custom Hyprland action".to_owned();
        }

        if self.dispatcher.trim().is_empty() && self.argument.trim().is_empty() {
            "Unknown action".to_owned()
        } else if self.argument.trim().is_empty() {
            self.dispatcher.clone()
        } else {
            format!("{} {}", self.dispatcher, self.argument.trim())
        }
    }

    pub fn searchable_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.display_key(),
            self.description,
            self.dispatcher,
            self.argument,
            if self.locked { "locked" } else { "" }
        )
        .to_lowercase()
    }
}

pub fn format_modmask(modmask: u32) -> String {
    let mut modifiers = Vec::new();

    if modmask & MOD_SUPER != 0 {
        modifiers.push("Super");
    }
    if modmask & MOD_CTRL != 0 {
        modifiers.push("Ctrl");
    }
    if modmask & MOD_ALT != 0 {
        modifiers.push("Alt");
    }
    if modmask & MOD_SHIFT != 0 {
        modifiers.push("Shift");
    }
    if modmask & MOD_ALT_GR != 0 {
        modifiers.push("AltGr");
    }
    if modmask & MOD_ALT2 != 0 {
        modifiers.push("Alt2");
    }
    if modmask & MOD_MOD3 != 0 {
        modifiers.push("Mod3");
    }
    if modmask & MOD_MOD5 != 0 {
        modifiers.push("Mod5");
    }
    if modmask & MOD_CAPS != 0 {
        modifiers.push("Caps");
    }

    modifiers.join(" + ")
}

fn format_key_name(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return "Unknown".to_owned();
    }

    if let Some(mouse_button) = key.strip_prefix("mouse:") {
        return match mouse_button {
            "272" => "Mouse 1".to_owned(),
            "273" => "Mouse 2".to_owned(),
            "274" => "Mouse 3".to_owned(),
            "275" => "Mouse 4".to_owned(),
            "276" => "Mouse 5".to_owned(),
            other => format!("Mouse {other}"),
        };
    }

    match key.to_ascii_lowercase().as_str() {
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "up" => "Up".to_owned(),
        "down" => "Down".to_owned(),
        "mouse_down" => "Mouse Down".to_owned(),
        "mouse_up" => "Mouse Up".to_owned(),
        "print" => "Print".to_owned(),
        "return" | "enter" => "Enter".to_owned(),
        "escape" | "esc" => "Escape".to_owned(),
        "space" => "Space".to_owned(),
        "tab" => "Tab".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "delete" => "Delete".to_owned(),
        "xf86audioraisevolume" => "XF86AudioRaiseVolume".to_owned(),
        "xf86audiolowervolume" => "XF86AudioLowerVolume".to_owned(),
        "xf86audiomute" => "XF86AudioMute".to_owned(),
        "xf86audiomicmute" => "XF86AudioMicMute".to_owned(),
        "xf86audionext" => "XF86AudioNext".to_owned(),
        "xf86audiopause" => "XF86AudioPause".to_owned(),
        "xf86audioplay" => "XF86AudioPlay".to_owned(),
        "xf86audioprev" => "XF86AudioPrev".to_owned(),
        "xf86monbrightnessup" => "XF86MonBrightnessUp".to_owned(),
        "xf86monbrightnessdown" => "XF86MonBrightnessDown".to_owned(),
        other if other.chars().count() == 1 => other.to_ascii_uppercase(),
        other => {
            if other.starts_with("xf86") {
                other.to_owned()
            } else {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => "Unknown".to_owned(),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub address: String,
    pub class: String,
    pub title: String,
    pub workspace: String,
    pub monitor: i32,
    pub focus_history_id: i32,
    pub mapped: bool,
    pub hidden: bool,
    pub accepts_input: bool,
}

#[derive(Debug, Deserialize)]
struct RawBinding {
    #[serde(default)]
    modmask: u32,
    #[serde(default)]
    key: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    dispatcher: String,
    #[serde(default)]
    arg: Option<String>,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    repeat: bool,
    #[serde(default)]
    release: bool,
    #[serde(default)]
    non_consuming: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawClient {
    #[serde(default)]
    address: String,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    workspace: RawWorkspace,
    #[serde(default)]
    monitor: i32,
    #[serde(rename = "focusHistoryID", default)]
    focus_history_id: i32,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(rename = "acceptsInput", default)]
    accepts_input: bool,
}

pub fn parse_bindings_json(json: &str) -> Result<Vec<Shortcut>, serde_json::Error> {
    let raw_bindings: Vec<RawBinding> = serde_json::from_str(json)?;
    Ok(raw_bindings
        .into_iter()
        .map(|binding| Shortcut {
            modmask: binding.modmask,
            key: binding.key,
            description: binding.description.unwrap_or_default(),
            dispatcher: binding.dispatcher,
            argument: binding.arg.unwrap_or_default(),
            locked: binding.locked,
            repeat: binding.repeat,
            release: binding.release,
            non_consuming: binding.non_consuming,
        })
        .collect())
}

pub fn parse_clients_json(json: &str) -> Result<Vec<WindowInfo>, serde_json::Error> {
    let raw_clients: Vec<RawClient> = serde_json::from_str(json)?;
    Ok(raw_clients
        .into_iter()
        .map(|client| WindowInfo {
            address: client.address,
            class: client.class,
            title: client.title,
            workspace: client.workspace.name,
            monitor: client.monitor,
            focus_history_id: client.focus_history_id,
            mapped: client.mapped,
            hidden: client.hidden,
            accepts_input: client.accepts_input,
        })
        .collect())
}

pub fn filter_shortcuts(shortcuts: &[Shortcut], query: &str) -> Vec<Shortcut> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return shortcuts.to_vec();
    }

    shortcuts
        .iter()
        .filter(|shortcut| shortcut.searchable_text().contains(&query))
        .cloned()
        .collect()
}
