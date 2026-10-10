#!/bin/sh
set -eu

repo="Zsweezzy/hypr-shortcut-launcher"
dest="${1:-$HOME/.local/bin}"

case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *)
        echo "unsupported architecture: $(uname -m)" >&2
        echo "build from source instead: cargo install --path ." >&2
        exit 1
        ;;
esac

url="https://github.com/$repo/releases/latest/download/hypr-shortcut-launcher-$arch-linux.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "downloading $url"
curl -fsSL "$url" -o "$tmp/app.tar.gz"
tar -xzf "$tmp/app.tar.gz" -C "$tmp"
install -Dm755 "$tmp/hypr-shortcut-launcher" "$dest/hypr-shortcut-launcher"

echo "installed $dest/hypr-shortcut-launcher"
case ":$PATH:" in
    *":$dest:"*) ;;
    *) echo "note: add $dest to PATH" ;;
esac
