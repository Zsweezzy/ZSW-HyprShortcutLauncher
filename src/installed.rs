use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

const TUI_APPS: &[&str] = &[
    "btop",
    "btop++",
    "htop",
    "atop",
    "bottom",
    "glances",
    "procs",
    "bandwhich",
    "gping",
    "nvtop",
    "ctop",
    "lazygit",
    "gitui",
    "tig",
    "lazydocker",
    "lazysql",
    "k9s",
    "dive",
    "yazi",
    "xplr",
    "superfile",
    "ranger",
    "lf",
    "nnn",
    "vifm",
    "mc",
    "ncdu",
    "gdu",
    "dust",
    "duf",
    "fzf",
    "fzf-tmux",
    "sk",
    "tldr",
    "tmux",
    "zellij",
    "dvtm",
    "byobu",
    "screen",
    "lazy",
    "nvim",
    "vim",
    "view",
    "nano",
    "micro",
    "helix",
    "hx",
    "kak",
    "emacs",
    "gdb",
    "lldb",
    "rr",
    "python",
    "python3",
    "ipython",
    "bpython",
    "node",
    "deno",
    "bun",
    "irb",
    "psql",
    "mysql",
    "mariadb",
    "sqlite3",
    "redis-cli",
    "mongosh",
    "opencode",
    "aider",
    "claude",
    "codex",
    "crush",
    "goose",
    "ollama",
];

const EXEC_WRAPPERS: &[&str] = &[
    "env",
    "flatpak",
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "dbus-launch",
    "dbus-run-session",
    "setsid",
    "nohup",
    "sudo",
    "doas",
    "pkexec",
    "systemctl",
    "systemd-run",
    "gio",
    "gapplication",
    "false",
    "true",
    "Xwayland",
];

const TUI_LABEL: &str = "TUI apps";
const COMMAND_LABEL: &str = "Commands";

const WHATIS_BATCH: usize = 500;

const TERMINAL_ENV: &str = "HYPR_LAUNCHER_TERMINAL";
const DEFAULT_TERMINAL: &str = "kitty";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledCommand {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub tui: bool,
}

impl InstalledCommand {
    pub fn searchable_text(&self) -> String {
        format!("{} {}", self.name, self.path).to_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledRow {
    Section(&'static str),
    Command(InstalledCommand),
}

impl InstalledRow {
    pub fn command(&self) -> Option<&InstalledCommand> {
        match self {
            InstalledRow::Section(_) => None,
            InstalledRow::Command(command) => Some(command),
        }
    }
}

pub fn scan() -> Vec<InstalledCommand> {
    let desktop = desktop_programs();
    let mut seen: HashSet<String> = HashSet::new();
    let mut commands = Vec::new();

    for directory in path_directories() {
        if directory.ends_with("flatpak/exports/bin") {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !seen.insert(name.clone()) || desktop.graphical.contains(&name) {
                continue;
            }
            if !is_executable(&entry.path()) {
                continue;
            }
            let tui = desktop.terminal.contains(&name) || TUI_APPS.contains(&name.as_str());
            commands.push(InstalledCommand {
                path: entry.path().display().to_string(),
                name,
                description: None,
                tui,
            });
        }
    }

    let names: Vec<String> = commands
        .iter()
        .map(|command| command.name.clone())
        .collect();
    load_descriptions(&names);
    {
        let descriptions = descriptions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for command in &mut commands {
            command.description = descriptions.get(&command.name).cloned().flatten();
        }
    }

    commands.sort_by(|left, right| {
        right
            .tui
            .cmp(&left.tui)
            .then_with(|| left.name.cmp(&right.name))
    });
    commands
}

pub fn filter(commands: &[InstalledCommand], query: &str) -> Vec<InstalledCommand> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return commands.to_vec();
    }

    commands
        .iter()
        .filter(|command| command.searchable_text().contains(&query))
        .cloned()
        .collect()
}

pub fn rows(commands: &[InstalledCommand]) -> Vec<InstalledRow> {
    let mut rows = Vec::new();
    let mut section: Option<bool> = None;
    for command in commands {
        if section != Some(command.tui) {
            rows.push(InstalledRow::Section(if command.tui {
                TUI_LABEL
            } else {
                COMMAND_LABEL
            }));
            section = Some(command.tui);
        }
        rows.push(InstalledRow::Command(command.clone()));
    }
    rows
}

pub fn launch(name: &str) -> Result<(), String> {
    let terminal = env::var(TERMINAL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TERMINAL.to_owned());
    Command::new(&terminal)
        .arg("--hold")
        .arg("-e")
        .arg(name)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not run {name} in {terminal}: {error}"))
}

struct DesktopPrograms {
    graphical: HashSet<String>,
    terminal: HashSet<String>,
}

fn desktop_programs() -> &'static DesktopPrograms {
    static PROGRAMS: OnceLock<DesktopPrograms> = OnceLock::new();
    PROGRAMS.get_or_init(|| {
        let mut graphical = HashSet::new();
        let mut terminal = HashSet::new();

        for directory in desktop_directories() {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension() != Some(OsStr::new("desktop")) {
                    continue;
                }
                let Ok(contents) = fs::read_to_string(&path) else {
                    continue;
                };
                let Some((runs_in_terminal, program)) = main_entry(&contents) else {
                    continue;
                };
                if EXEC_WRAPPERS.contains(&program.as_str()) {
                    continue;
                }
                if runs_in_terminal {
                    terminal.insert(program);
                } else {
                    graphical.insert(program);
                }
            }
        }

        DesktopPrograms {
            graphical,
            terminal,
        }
    })
}

fn main_entry(contents: &str) -> Option<(bool, String)> {
    let mut in_main_group = false;
    let mut runs_in_terminal = false;
    let mut program = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_group {
            continue;
        }
        if let Some(value) = line.strip_prefix("Terminal=") {
            runs_in_terminal = value.trim().eq_ignore_ascii_case("true");
        } else if program.is_none()
            && let Some(value) = line.strip_prefix("Exec=")
        {
            program = exec_program(value);
        }
    }

    program.map(|program| (runs_in_terminal, program))
}

fn exec_program(value: &str) -> Option<String> {
    let token = value.split_whitespace().next()?.trim_matches('"');
    Path::new(token).file_name()?.to_str().map(str::to_owned)
}

fn desktop_directories() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let data_dirs: Vec<PathBuf> = env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();

    data_home
        .into_iter()
        .chain(data_dirs)
        .map(|directory| directory.join("applications"))
        .collect()
}

fn path_directories() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn descriptions() -> &'static Mutex<HashMap<String, Option<String>>> {
    static DESCRIPTIONS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    DESCRIPTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_descriptions(names: &[String]) {
    let missing: Vec<String> = {
        let cache = descriptions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        names
            .iter()
            .filter(|name| !cache.contains_key(*name))
            .cloned()
            .collect()
    };
    if missing.is_empty() {
        return;
    }

    let mut found = HashMap::new();
    for batch in missing.chunks(WHATIS_BATCH) {
        let Ok(output) = Command::new("whatis").arg("--").args(batch).output() else {
            break;
        };
        for (name, description) in parse_whatis(&String::from_utf8_lossy(&output.stdout)) {
            found.entry(name).or_insert(description);
        }
    }

    let mut cache = descriptions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for name in missing {
        cache.insert(name.clone(), found.remove(&name));
    }
}

fn parse_whatis(text: &str) -> HashMap<String, String> {
    let mut descriptions = HashMap::new();
    for line in text.lines() {
        let Some((entry, description)) = line.split_once(" - ") else {
            continue;
        };
        let name = entry
            .rsplit_once(" (")
            .map_or(entry, |(name, _)| name)
            .trim();
        if name.is_empty() {
            continue;
        }
        descriptions
            .entry(name.to_owned())
            .or_insert_with(|| description.trim().to_owned());
    }
    descriptions
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_splits_into_terminal_flag_and_program() {
        let (terminal, program) = main_entry(
            "[Desktop Entry]\nType=Application\nExec=/usr/bin/steam %U\nTerminal=false\n\
             [Desktop Action new-window]\nExec=steam -new\nTerminal=true\n",
        )
        .unwrap();

        assert!(!terminal);
        assert_eq!(program, "steam");
    }

    #[test]
    fn entry_without_exec_is_ignored() {
        assert!(main_entry("[Desktop Entry]\nType=Directory\nName=slot\n").is_none());
    }

    #[test]
    fn rows_head_each_section_and_drop_empty_ones() {
        let commands = vec![
            InstalledCommand {
                name: "btop".to_owned(),
                path: "/usr/bin/btop".to_owned(),
                description: None,
                tui: true,
            },
            InstalledCommand {
                name: "ls".to_owned(),
                path: "/usr/bin/ls".to_owned(),
                description: None,
                tui: false,
            },
            InstalledCommand {
                name: "mv".to_owned(),
                path: "/usr/bin/mv".to_owned(),
                description: None,
                tui: false,
            },
        ];

        let rows = rows(&commands);

        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0], InstalledRow::Section("TUI apps"));
        assert_eq!(rows[2], InstalledRow::Section("Commands"));
        assert_eq!(rows[1].command().unwrap().name, "btop");
        assert_eq!(rows[3].command().unwrap().name, "ls");
    }

    #[test]
    fn no_query_match_leaves_no_rows_at_all() {
        assert!(rows(&filter(&[], "firefox")).is_empty());
    }

    #[test]
    fn filter_matches_name_or_path() {
        let commands = vec![InstalledCommand {
            name: "python3.13".to_owned(),
            path: "/usr/bin/python3.13".to_owned(),
            description: None,
            tui: false,
        }];

        assert_eq!(filter(&commands, "PYTHON").len(), 1);
        assert_eq!(filter(&commands, "/usr/bin").len(), 1);
        assert!(filter(&commands, "firefox").is_empty());
    }

    #[test]
    fn whatis_output_splits_into_name_and_summary() {
        let parsed = parse_whatis(
            "ls (1)               - list directory contents\n\
             cp (1)               - copy files and directories\n\
             cp (1p)              - copy files\n\
             cat: nothing appropriate.\n",
        );

        assert_eq!(
            parsed.get("cp").map(String::as_str),
            Some("copy files and directories")
        );
        assert_eq!(
            parsed.get("ls").map(String::as_str),
            Some("list directory contents")
        );
        assert_eq!(parsed.len(), 2);
    }
}
