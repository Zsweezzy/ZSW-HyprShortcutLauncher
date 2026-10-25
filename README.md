# ZSW-HyprShortcutLauncher

A searchable `Super+K` overlay for Hyprland. Three tabs: the keymap you actually
configured, the apps you actually have running, and the commands you can run.

## TL;DR

- **What** — one search box over your active Hyprland binds, running windows, and
  every executable on `PATH`.
- **Install** — no Rust needed. Grab the binary from
  [Releases](../../releases), or run `install.sh`.
- **Bind** — autostart a hidden instance, then bind `SUPER, K`.
- **Use** — type to filter, `Enter` to focus the app or run the command, `Tab` to
  switch tabs, `Esc` to close.

## Install

### Without Rust

Download the binary for your architecture from the
[latest release](../../releases) and put it on `PATH`. Or:

```sh
curl -fsSL https://raw.githubusercontent.com/Zsweezzy/ZSW-HyprShortcutLauncher/main/install.sh | sh
```

The release binary needs GTK 4 and gtk4-layer-shell at runtime; both come from
your distro's package manager (on Arch: `gtk4` and `gtk4-layer-shell`). The
AppImage bundles them if you would rather not install anything.

### With Rust

Needs Rust (edition 2024), GTK 4 and gtk4-layer-shell development headers.

```sh
cargo build --release
install -Dm755 target/release/hypr-shortcut-launcher ~/.local/bin/hypr-shortcut-launcher
```

`cargo install --path .` also works if you want the binary managed by cargo.

## Bind it

Autostart a hidden instance so the overlay opens instantly, then bind the key.
See [`examples/hyprland.lua`](examples/hyprland.lua) for the annotated Lua version.

```lua
hl.exec_cmd(os.getenv("HOME") .. "/.local/bin/hypr-shortcut-launcher --background")
hl.bind("SUPER, K", hl.dsp.exec_cmd(os.getenv("HOME") .. "/.local/bin/hypr-shortcut-launcher"),
        { description = "Open shortcuts and apps launcher" })
```

In a plain `hyprland.conf`, the same two lines are
`exec-once = hypr-shortcut-launcher --background` and
`bind = SUPER, K, exec, hypr-shortcut-launcher`.

`--background` starts the instance hidden and warms the data cache, so the key
press does not wait on `hyprctl`. Later invocations reach the running instance
over a per-user Unix socket under `$XDG_RUNTIME_DIR`, so repeated `Super+K` presses
toggle one window instead of stacking overlays.

The `description` on a bind is not decoration: the shortcuts tab reads it back out
of `hyprctl binds -j`, and Lua-backed binds are otherwise shown as an internal
`__lua` id. A bind with no description shows as `Custom Hyprland action`.

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

- Full-monitor transparent layer surface, centered rounded panel; outside the
  panel stays fully transparent, like Rofi.
- Hyprland data is fetched on a background thread and rendered in place, so the
  window appears on the keystroke. The running-app list refreshes every two
  seconds while open.
- `Installed apps` is every executable on `PATH` minus graphical launchers, in
  `TUI apps` and `Commands` sections. Rows show the one-line `whatis` summary
  (`whatis` reads the manual index, so no command is executed); Enter runs it in a
  terminal that stays open. The list of sources is in
  [`src/installed.rs`](src/installed.rs).
- Sits on the largest physical monitor. Set `HYPR_LAUNCHER_MAIN_MONITOR` to a
  connector name (`DP-1`, `HDMI-A-1`, …) to choose.
- Commands run in `$HYPR_LAUNCHER_TERMINAL`, default `kitty`.
- App focus tries the Lua dispatcher first, then falls back to a native
  `focuswindow` dispatch.

## Development

```sh
cargo test                              # unit + integration tests
cargo clippy --all-targets -- -D warnings
```

Source files carry no comments; the rationale lives in [AGENTS.md](AGENTS.md).

## Licence

MIT, see [LICENSE](LICENSE). GTK 4 and gtk4-layer-shell are LGPL-2.1 and linked
dynamically against your system's copy; [NOTICE](NOTICE) has the details.
