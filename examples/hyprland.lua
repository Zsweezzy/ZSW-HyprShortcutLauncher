-- Super+K integration for hypr-shortcut-launcher.
--
-- Two lines. One warms a hidden instance at login, one binds the key. Paste
-- them into your own ~/.config/hypr/hyprland.lua; this file is a reference,
-- not something the launcher loads.
--
-- Written for the Lua-backed Hyprland configuration API (hl.bind /
-- hl.exec_cmd). In a plain hyprland.conf, translate them to
-- `bind = SUPER, K, exec, hypr-shortcut-launcher` and the matching
-- `exec-once = hypr-shortcut-launcher --background` line.

local LAUNCHER = (os.getenv("HOME") or "/home/user")
    .. "/.local/bin/hypr-shortcut-launcher"

-- Autostart: start the persistent instance hidden. It fills the Hyprland data
-- cache (binds, clients) without showing anything, so the first Super+K press
-- only has to reveal an existing window instead of paying the `hyprctl` query
-- cost on the keystroke.
--
-- Run this inside your startup callback. Hyprland's autostart block runs with
-- a minimal environment, so use an absolute path rather than relying on PATH.
hl.exec_cmd(LAUNCHER .. " --background")

-- Super+K = searchable Hyprland shortcuts and running-app switcher.
--
-- The `description` field is not decoration: the shortcuts tab reads it back
-- out of `hyprctl binds -j` to say what the key does. Lua-backed binds are
-- otherwise reported with an internal `__lua` action id, which would be all a
-- user ever sees. Binds with no description fall back to "Custom Hyprland
-- action" rather than exposing that id.
--
-- `exec` (not `execsh`) passes the path through without a shell, so no
-- quoting is needed.
hl.bind("SUPER, K", hl.dsp.exec_cmd(LAUNCHER), {
    description = "Open shortcuts and apps launcher",
})
