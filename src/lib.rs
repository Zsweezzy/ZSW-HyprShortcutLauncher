pub mod apps;
pub mod hyprland;
pub mod installed;
pub mod model;

pub use apps::{AppGroup, filter_apps, group_windows};
pub use hyprland::{
    CommandExecutor, HyprlandError, HyprlandService, ProcessCommandExecutor, is_focus_change_event,
};
pub use installed::{InstalledCommand, InstalledRow};
pub use model::{
    Shortcut, filter_shortcuts, format_modmask, parse_bindings_json, parse_clients_json,
};
