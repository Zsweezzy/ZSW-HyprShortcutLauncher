# hypr-shortcut-launcher

A searchable overlay for Hyprland, bound to `Super+K`. Three tabs: the keymap you
have actually configured, the apps you have actually got running, and the
commands you can run.

- **Hyprland shortcuts** — active bindings read from `hyprctl binds -j`, with
  the `description` your config set on each bind.
- **Running apps** — one row per application class. Selecting a row focuses its
  most recently used window, which is usually the one you meant.
- **Installed apps** — every executable on `PATH`, in two sections: `TUI apps`
  and `Commands`. Each row shows the one-line summary from the manual (`whatis`
  reads the index, so no command is executed), and Enter runs it in a terminal
  window that stays open afterwards, so the output of a one-shot command like
  `ls` is readable.

The bindings tab is read-only. It shows what a key does; it does not run it.

Anything a desktop entry starts is treated as a graphical launcher and left out
of the installed list, since a launcher already exists for it. That signal comes
from the desktop entries under the XDG data directories: `Terminal=true` marks a
terminal program, everything else is a window. Programs with no desktop entry at
all, which is most TUI apps, are named in `TUI_APPS` in
[`src/installed.rs`](src/installed.rs).

## Why

`hyprctl binds` is a wall of dispatcher calls and no descriptions. Finding out
what `Super+Shift+V` does means reading the config. Finding out which of six
terminal windows is in front of the others means cycling. Both answers are one
keystroke away here, and the shortcut names come from the same strings that
already exist in the config.

## Build

Needs Rust (edition 2024), GTK 4 and gtk4-layer-shell development headers.

```sh
cargo build --release
```

## Install

```sh
install -Dm755 target/release/hypr-shortcut-launcher ~/.local/bin/hypr-shortcut-launcher
```

`cargo install --path .` also works if you want the binary managed by cargo.

## Bind it

Autostart a hidden instance so the overlay opens instantly, then bind the key.
See [`examples/hyprland.lua`](examples/hyprland.lua) for the annotated
Lua-config version.

```lua
hl.exec_cmd(os.getenv("HOME") .. "/.local/bin/hypr-shortcut-launcher --background")
hl.bind("SUPER, K", hl.dsp.exec_cmd(os.getenv("HOME") .. "/.local/bin/hypr-shortcut-launcher"),
        { description = "Open shortcuts and apps launcher" })
```

`--background` starts the instance without showing the overlay. It fills the
data cache up front, so the keystroke does not have to wait on `hyprctl`. Each
later invocation connects to the running instance over a per-user Unix socket
under `$XDG_RUNTIME_DIR` and toggles it, so repeated `Super+K` presses flip one
window rather than stacking overlays.

The `description` on a bind is not decoration. The shortcuts tab reads it out of
`hyprctl binds -j`, and Lua-backed binds are otherwise reported with an internal
`__lua` action id that means nothing to a person. A bind with no description is
shown as `Custom Hyprland action`.

Run `hyprctl reload` after changing the config.

## Controls

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | switch tab |
| `Alt+1` / `Alt+2` / `Alt+3` | pick a tab directly |
| `Up` / `Down`, `Ctrl+J` / `Ctrl+K` | move through results |
| `Enter` | show a shortcut, focus the selected app, or run the selected command |
| `Ctrl+R` | refresh Hyprland data |
| `Esc` | close |

The overlay also closes when it loses focus, so `Super+Arrow` or clicking
elsewhere dismisses it.

## Notes

The overlay is a full-monitor transparent layer surface with a centered,
rounded, lightly translucent panel. Outside the panel stays fully transparent,
so it reads like Rofi and does not dim the desktop behind it.

Hyprland data is fetched on a background thread and rendered in place, so the
window appears on the keystroke rather than after the query returns. The running
app list refreshes every two seconds while the overlay is open.

It sits on the largest physical monitor by default. Set
`HYPR_LAUNCHER_MAIN_MONITOR` to a connector name (`DP-1`, `HDMI-A-1`, …) to
choose explicitly.

Commands run in `$HYPR_LAUNCHER_TERMINAL`, default `kitty`. Point it at the same
variable your own terminal bind uses so both agree.

App focus tries the Lua dispatcher form first when the running Hyprland config
is Lua-based, and falls back to a native `focuswindow` dispatch.

## Licence

MIT, see [LICENSE](LICENSE). GTK 4 and gtk4-layer-shell are LGPL-2.1 and linked
dynamically against your system's copy; [NOTICE](NOTICE) has the details.
