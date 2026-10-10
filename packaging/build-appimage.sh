#!/bin/sh
set -eu

arch="${ARCH:-x86_64}"
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/hypr-shortcut-launcher-appimage"
out="${OUT_DIR:-$root/dist}"
mkdir -p "$cache" "$out"

fetch() {
    [ -f "$2" ] || curl -fsSL -o "$2" "$1"
    chmod +x "$2"
}

fetch "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage" "$cache/linuxdeploy.AppImage"
fetch "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage" "$cache/appimagetool.AppImage"
fetch "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh" "$cache/linuxdeploy-plugin-gtk.sh"

sed -i 's|^\( *\)copy_lib_tree "$gtk4_libdir" "$APPDIR/"|\1[ -d "$gtk4_libdir" ] \&\& copy_lib_tree "$gtk4_libdir" "$APPDIR/"|' "$cache/linuxdeploy-plugin-gtk.sh"

export PATH="$cache:$PATH"
export ARCH="$arch"

cargo build --release --locked --manifest-path "$root/Cargo.toml"

work="$cache/appdir"
rm -rf "$work"
mkdir -p "$work"
cd "$work"

linuxdeploy.AppImage --appimage-extract-and-run \
    --appdir . \
    --plugin gtk \
    --desktop-file "$root/packaging/hypr-shortcut-launcher.desktop" \
    --icon-file "$root/packaging/hypr-shortcut-launcher.svg" \
    --executable "$root/target/release/hypr-shortcut-launcher"

printf '\nunset GDK_BACKEND\n' >> apprun-hooks/linuxdeploy-plugin-gtk.sh

appimagetool.AppImage --appimage-extract-and-run . "$out/hypr-shortcut-launcher-$arch.AppImage"
echo "wrote $out/hypr-shortcut-launcher-$arch.AppImage"
