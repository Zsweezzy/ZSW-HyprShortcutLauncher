# AGENTS.md

Project guide for OpenCode (and other agents). The source files carry no
comments on purpose: the "why" behind each non-obvious line lives here, keyed by
file and symbol. Keep it that way — when you add code that needs an explanation,
add it to this file instead of a code comment.

## src/main.rs

- `APPLICATION_ID` — Reverse-DNS application id, used by GTK for the window's WM
  class and for the D-Bus name that makes a second invocation reach the running
  instance.
- `let _hold = app.hold();` — Keep the process alive after the overlay is hidden.
  The local instance socket forwards later invocations to this process, so
  Super+K toggles one launcher instead of creating a second copy.

## src/model.rs

- `MOD_*` constants — The modifier bit values used by Hyprland's `modmask` field.
- `Shortcut::action_label`, `dispatcher == "__lua"` branch — Hyprland's Lua bridge
  exposes an internal numeric action id rather than the action's meaning. Lua
  configurations should provide a description (which is returned earlier); keep
  older or custom configurations readable without leaking that implementation id.

## src/apps.rs

- `AppGroup::shortcut` — Global hotkey for an app with no window to focus. `None`
  for windowed apps, whose rows are focused with Enter instead.
- `WindowlessApp` — An app that runs without a window. `hyprctl clients` cannot
  report these, so a running one would otherwise be missing from the applications
  tab entirely.
- `WINDOWLESS_APPS[0]` (`neru`) — Neru grabs the keyboard through the evdev proxy
  instead of binding a Hyprland shortcut, so nothing about it is discoverable from
  the outside.
- `append_running_windowless_apps` — Appends the windowless apps that are
  currently running. They go last, which is where `group_windows` already sorts an
  app with no window to focus.
- `process_is_running` — Returns whether a process with this exact name is
  running, by reading `/proc/<pid>/comm`. A full `/proc` scan is cheap enough for
  a couple of names and avoids spawning `pgrep` per lookup.

## src/hyprland.rs

- `HyprlandService::focus_window` — This Hyprland instance uses the Lua
  configuration bridge. Its `hyprctl dispatch` endpoint expects a Lua dispatcher
  expression, rather than the native `focuswindow address:...` argument form. Keep
  the native form as a fallback for ordinary Hyprland setups.
- `is_focus_change_event` — Returns whether a Hyprland event socket message
  represents a focus or workspace change that should dismiss an open launcher
  overlay. Hyprland emits both the legacy event names and their `v2` counterparts,
  depending on the compositor version and the subscriptions used by the client.
- `spawn_focus_change_listener` — Starts a background listener for Hyprland's
  event socket. The listener is intentionally best-effort: if Hyprland is
  restarting or the event socket is not available yet, it reconnects periodically.
  The supplied flag is consumed by the GTK main loop, which performs the actual
  window mutation on the UI thread.
- `spawn_focus_change_listener`, empty `if let` block — Hyprland may be restarting
  or the socket may not have appeared yet. Keep trying without interrupting the
  launcher process.

## src/ipc.rs

- `acquire_instance_at`, lock acquired — A legacy primary may predate the lock
  file. Give it a chance to receive the toggle before replacing a stale path.
- `acquire_instance_at`, `WouldBlock` branch — The lock owner may be between
  opening the lock and binding its socket. Retry briefly instead of stealing its
  endpoint.
- `socket_path` — No `/tmp` fallback. A missing `XDG_RUNTIME_DIR` means no Wayland
  session either, so there is nothing to toggle, and a world-visible socket is the
  wrong trade for a hypothetical headless run.

## src/installed.rs

- Module role — Discovery of runnable programs for the "Installed apps" tab. Two
  sources decide what lands in the list. `PATH` says what can be run at all, and
  the desktop entries under the XDG data directories say which of those open a
  window of their own. Anything a desktop entry starts is a graphical launcher, so
  it stays out of the list the launcher offers to run.
- `TUI_APPS` — Programs whose whole purpose is an interactive full-screen terminal
  UI. Most of these ship no desktop entry at all, because a graphical launcher has
  no use for them. The desktop signal cannot see them, so they are named here.
  Entries are grouped by category: monitoring; git and cluster; files; pickers and
  docs; multiplexers; editors; debuggers; repls; terminal agents.
- `EXEC_WRAPPERS` — First tokens of an `Exec=` line that run something else rather
  than being the app. Naming one of these as "the graphical app" would hide a
  command the launcher is meant to offer (`flatpak` and `env` both appear this
  way).
- `WHATIS_BATCH` — How many names one `whatis` call may carry. The tool takes the
  whole batch at once, so the whole list is resolved in a handful of processes.
- `TERMINAL_ENV` — Terminal the launcher opens commands in. Hyprland passes its own
  `terminal` variable through the environment, so the two cannot drift apart.
- `InstalledCommand::path` — Resolved location, which is what tells `python3`
  apart from `python3.13`.
- `InstalledCommand::description` — One-line manual summary shown in place of the
  path. `None` when the manual has no entry for the command.
- `InstalledRow` — One rendered line: a section heading, or a command to run.
  Headings live in the same list as their commands so they scroll with them.
- `scan` — Every executable on `PATH`, minus the graphical launchers, ordered so
  the TUI section comes first and both sections read alphabetically.
- `scan`, flatpak skip — Flatpak's export directory holds one wrapper per graphical
  app, named after the app id rather than anything a person would type.
- `rows` — Interleaves the section headings. A heading is emitted where the
  section changes, so a section with no match under the current query disappears
  instead of heading an empty list.
- `launch` — Runs `name` in a new terminal window that stays open after the program
  exits, so the output of a one-shot command like `ls` can still be read.
- `desktop_programs` — Reads the installed desktop entries once. They are packaging
  output, so they do not change while the launcher is open and the `PATH` scan runs
  every couple of seconds.
- `main_entry` — Returns `Terminal=` and the program an entry starts, read from the
  main `[Desktop Entry]` group only: the action groups below it describe what the
  menu item does, not what the program is.
- `descriptions` — One-line manual summaries keyed by command name, kept for the
  life of the process. A `None` value records "no manual entry" so a command
  without one is not looked up again on the next refresh.
- `load_descriptions` — Fills the summary cache for any names not seen before.
  `whatis` only reads the prebuilt manual index, so no listed command is executed,
  and it accepts a whole batch per call.
- `parse_whatis` — Parses `whatis` output (`name (section) - summary`) into
  name/summary pairs. The first entry wins, which prefers the primary `(1)` section
  over variants.
- `parse_whatis` test — The primary section wins over the `(1p)` variant, and a
  line without a separator is ignored.

## src/ui.rs

- `ActiveTab::COUNT` — Number of tabs, and so the width of the Tab cycle.
- `State::installed_rows` — Section headings interleaved with `filtered_installed`,
  in the order the rows are rendered. A row index indexes this, not
  `filtered_installed`.
- `RefreshResult::installed` — A `PATH` scan that swallows unreadable directories,
  so it cannot fail.
- window setup, `set_resizable(true)` — Keep the layer surface resizable so
  gtk4-layer-shell can honor all four anchors and expand its transparent hit area
  across the selected output.
- key controller — Capture keys before the focused search entry can consume Escape
  or navigation shortcuts. Returning `Proceed` still allows normal text input to
  reach the entry.
- `Activate`/hidden startup — Prime the layer surface while the daemon starts
  hidden, then keep the window ready for the first Super+K press without a visible
  flash.
- background timeout — Warm the data cache while the daemon is hidden in the
  background.
- row scroll scheduling — Rows are rebuilt while the window may still be laying
  out. Retry once on the next idle turn so the first keyboard interaction is not
  lost.
- `start_refresh` — Present the shell before querying Hyprland. Both `hyprctl`
  calls are subprocesses and must not delay the first frame of the launcher.
- installed selection merge — Row indices count headings, so resolve the selection
  against the rendered rows and hand back a row index.
- installed selection merge, `command_row_at_or_after` — The installed list opens
  with a section heading, which cannot be run. Land the selection on the first
  command instead so Enter works right after typing a query, the same as in the
  other tabs.
- key handling, `Super+K` — The Hyprland binding and IPC own the actual Super+K
  toggle. If the compositor also forwards the key to GTK, consume it without
  toggling a second time (or inserting a stray "k" into the search entry).
- `move_selection` — Section headings are rows like any other, so step past the
  ones that cannot be selected rather than letting the cursor rest on a label.
- `select_and_scroll` — Selects a row and brings it into view, if the list has that
  many rows.
- `installed_row` — One line of the installed-apps tab: a section heading, or a
  runnable command.
- windowless app row — A windowless app has nothing for Enter to focus, so its row
  advertises the global hotkey that drives it instead of a window title.
- `preserved_installed_index` — Same idea as `preserved_app_index`, but over rows
  that carry headings, so a selected command stays selected when a heading appears
  above it.
- `command_row_at_or_after` — First runnable row at or after `start`, skipping
  section headings.
- `command_row_at_or_after` test — Row 0 is the "TUI apps" heading; the first
  runnable row is `btop`.

## scripts and packaging

- `install.sh` — Downloads the latest release tarball for the host architecture
  into `~/.local/bin` (or the directory in `$1`). Tarballs are named
  `hypr-shortcut-launcher-<arch>-linux.tar.gz` and hold the binary at the top
  level. An unknown architecture tells the user to build from source.
- `packaging/build-appimage.sh` — Builds an AppImage for `$ARCH` (default
  `x86_64`) into `$OUT_DIR` (default `dist/`). Downloads linuxdeploy,
  appimagetool, and the linuxdeploy GTK plugin into
  `$XDG_CACHE_HOME/hypr-shortcut-launcher-appimage`; the tools are cached across
  runs. The AppImage is built locally, not by CI.
- `packaging/build-appimage.sh`, `sed` guard on the GTK plugin — The plugin's
  `copy_lib_tree "$gtk4_libdir"` aborts the whole build with
  `cp: cannot stat 'gtk-4.0'` on hosts without `/usr/lib/gtk-4.0`. Making that
  copy conditional leaves the rest of the GTK bundling untouched.
- `packaging/build-appimage.sh`, `--appdir` then `appimagetool` — linuxdeploy is
  run without `--output appimage` so the AppDir can be patched before packaging.
  The app is a `gtk4-layer-shell` overlay, which is Wayland-only, but the GTK
  plugin's AppRun hook (sourced by the generated `AppRun`) forces
  `GDK_BACKEND=x11`. Appending `unset GDK_BACKEND` to that hook restores GTK's
  native backend choice (Wayland under Hyprland), then appimagetool packages the
  AppDir.
- `.github/workflows/release.yml` — On a `v*` tag (or a manual dispatch) builds
  the tarball on an x86_64 and an aarch64 runner and attaches
  `hypr-shortcut-launcher-<arch>-linux.tar.gz` to the release. Each tarball holds
  the binary plus `README.md`, `LICENSE`, and `NOTICE`.
- `.github/workflows/release.yml`, gtk4-layer-shell step — Ubuntu 24.04 (the
  runner) has no `libgtk4-layer-shell-dev`: the package only reached Ubuntu in
  25.10. Build the pinned `v1.3.0` tag from source with meson instead.
  `-Dintrospection=false -Dvapi=false -Dsmoke-tests=false` drops the optional
  gir/vapi data and the headless-hostile smoke tests; `--prefix=/usr` lands the
  `.pc` file where `pkg-config` looks.
- `packaging/hypr-shortcut-launcher.desktop`,
  `packaging/hypr-shortcut-launcher.svg` — AppImage desktop entry and icon.
