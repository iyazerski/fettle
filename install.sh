#!/usr/bin/env sh
# Set FETTLE_VERSION=vX.Y.Z to pin a release.
set -eu

repo="iyazerski/fettle"
asset="Fettle.zip"
id="dev.iyazerski.fettle"
# Every release is signed with this certificate; the checksum only catches damage.
signer="=identifier \"$id\" and certificate leaf = H\"d92f0c1f8b4c244056244514e6e27e95cc431234\""

if [ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]; then
  echo "Fettle supports macOS on Apple silicon only." >&2
  exit 1
fi

if [ -n "${FETTLE_VERSION:-}" ]; then
  download_url="https://github.com/$repo/releases/download/$FETTLE_VERSION/$asset"
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
if ! codesign --verify --strict -R "$signer" "$install_tmp/Fettle.app"; then
  echo "The download isn't signed by Fettle's release certificate. Not installing it." >&2
  exit 1
fi

apps="/Applications"
if [ ! -w "$apps" ]; then
  apps="$HOME/Applications"
  mkdir -p "$apps"
fi

pkill -x fettle 2>/dev/null && sleep 1 || true
rm -rf "$apps/Fettle.app"
mv "$install_tmp/Fettle.app" "$apps/Fettle.app"
xattr -dr com.apple.quarantine "$apps/Fettle.app" 2>/dev/null || true

echo "$("$apps/Fettle.app/Contents/MacOS/fettle" --version) is installed to $apps/Fettle.app"
open "$apps/Fettle.app"
echo "Fettle is running. Allow it in Accessibility when asked, then select text and press ⌃⌥E."
