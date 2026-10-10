use std::{
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use crate::model::{Shortcut, WindowInfo, parse_bindings_json, parse_clients_json};

#[derive(Debug, thiserror::Error)]
pub enum HyprlandError {
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("invalid Hyprland JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("could not start Hyprland command: {0}")]
    Io(#[from] io::Error),
}

pub trait CommandExecutor {
    fn run(&self, args: &[&str]) -> Result<String, HyprlandError>;
}

#[derive(Debug, Clone)]
pub struct ProcessCommandExecutor {
    program: PathBuf,
}

impl ProcessCommandExecutor {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for ProcessCommandExecutor {
    fn default() -> Self {
        Self::new("hyprctl")
    }
}

impl CommandExecutor for ProcessCommandExecutor {
    fn run(&self, args: &[&str]) -> Result<String, HyprlandError> {
        let output = Command::new(&self.program).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() {
                format!("{} exited with {}", self.program.display(), output.status)
            } else {
                stderr
            };
            return Err(HyprlandError::CommandFailed(message));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

pub struct HyprlandService<E> {
    executor: E,
}

impl<E> HyprlandService<E>
where
    E: CommandExecutor,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub fn bindings(&self) -> Result<Vec<Shortcut>, HyprlandError> {
        let output = self.executor.run(&["binds", "-j"])?;
        Ok(parse_bindings_json(&output)?)
    }

    pub fn clients(&self) -> Result<Vec<WindowInfo>, HyprlandError> {
        let output = self.executor.run(&["clients", "-j"])?;
        Ok(parse_clients_json(&output)?)
    }

    pub fn focus_window(&self, address: &str) -> Result<(), HyprlandError> {
        validate_address(address)?;
        let lua_expression = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");

        match self.executor.run(&["dispatch", &lua_expression]) {
            Ok(_) => Ok(()),
            Err(lua_error) => {
                let native_argument = format!("address:{address}");
                self.executor
                    .run(&["dispatch", "focuswindow", &native_argument])
                    .map(|_| ())
                    .map_err(|native_error| {
                        HyprlandError::CommandFailed(format!(
                            "Lua focus failed: {lua_error}; native focus failed: {native_error}"
                        ))
                    })
            }
        }
    }
}

impl Default for HyprlandService<ProcessCommandExecutor> {
    fn default() -> Self {
        Self::new(ProcessCommandExecutor::default())
    }
}

pub fn is_focus_change_event(event: &str) -> bool {
    let event_name = event.split_once(">>").map_or(event, |(name, _)| name);
    matches!(
        event_name,
        "activewindow"
            | "activewindowv2"
            | "focusedmon"
            | "focusedmonv2"
            | "workspace"
            | "workspacev2"
    )
}

#[cfg(unix)]
pub fn spawn_focus_change_listener(change_requested: Arc<AtomicBool>) {
    thread::spawn(move || {
        loop {
            if let Some(path) = focus_event_socket_path()
                && listen_for_focus_changes(&path, &change_requested).is_err()
            {}
            thread::sleep(Duration::from_millis(250));
        }
    });
}

#[cfg(not(unix))]
pub fn spawn_focus_change_listener(_change_requested: Arc<AtomicBool>) {}

#[cfg(unix)]
fn focus_event_socket_path() -> Option<PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(
        PathBuf::from(runtime_dir)
            .join("hypr")
            .join(signature)
            .join(".socket2.sock"),
    )
}

#[cfg(unix)]
fn listen_for_focus_changes(path: &Path, change_requested: &AtomicBool) -> io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    for event in ["activewindow>>*", "focusedmon>>*", "workspace>>*"] {
        writeln!(stream, "{event}")?;
    }

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        if is_focus_change_event(&line?) {
            change_requested.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}

fn validate_address(address: &str) -> Result<(), HyprlandError> {
    let Some(hex) = address
        .strip_prefix("0x")
        .or_else(|| address.strip_prefix("0X"))
    else {
        return Err(HyprlandError::InvalidAddress(address.to_owned()));
    };
    if hex.is_empty() || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(HyprlandError::InvalidAddress(address.to_owned()));
    }
    Ok(())
}
