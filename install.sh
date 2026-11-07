#!/usr/bin/env sh
# Install or update Quill.app from the latest release, and start it.
# Set QUILL_VERSION=vX.Y.Z to pin a release.
set -eu

repo="iyazerski/quill"
asset="Quill.zip"
id="dev.iyazerski.quill"

if [ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]; then
  echo "Quill supports macOS on Apple silicon only." >&2
  exit 1
fi

if [ -n "${QUILL_VERSION:-}" ]; then
  download_url="https://github.com/$repo/releases/download/$QUILL_VERSION/$asset"
else
  download_url="https://github.com/$repo/releases/latest/download/$asset"
fi
install_tmp="$(mktemp -d)"
trap 'rm -rf "$install_tmp"' EXIT HUP INT TERM

echo "Downloading $asset..."
curl -fsSL "$download_url" -o "$install_tmp/$asset"
curl -fsSL "$download_url.sha256" -o "$install_tmp/$asset.sha256"
(cd "$install_tmp" && shasum -a 256 -c "$asset.sha256")
ditto -x -k "$install_tmp/$asset" "$install_tmp"

# /Applications when it's writable without sudo, else ~/Applications.
apps="/Applications"
if [ ! -w "$apps" ]; then
  apps="$HOME/Applications"
  mkdir -p "$apps"
fi

updating=false
if [ -d "$apps/Quill.app" ]; then
  updating=true
fi
pkill -x quill 2>/dev/null && sleep 1 || true
rm -rf "$apps/Quill.app"
mv "$install_tmp/Quill.app" "$apps/Quill.app"
xattr -dr com.apple.quarantine "$apps/Quill.app" 2>/dev/null || true

if [ "$updating" = true ]; then
  # The Accessibility permission belongs to the old build's signature: clear
  # it, so Quill asks again instead of silently failing to copy.
  tccutil reset Accessibility "$id" >/dev/null 2>&1 || true
fi

echo "$("$apps/Quill.app/Contents/MacOS/quill" --version) is installed to $apps/Quill.app"
open "$apps/Quill.app"
echo "Quill is running. Allow it in Accessibility when asked, then select text and press ⌃⌥E."
