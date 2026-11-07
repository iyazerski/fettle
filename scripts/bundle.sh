#!/usr/bin/env bash
# Build Fettle.app from a release binary: scripts/bundle.sh <binary> <version> <out-dir>
# Signs with the identity in FETTLE_SIGN_IDENTITY, or ad hoc without one. macOS
# keeps the Accessibility grant across updates only for a stable identity: an
# ad-hoc signature changes with every build.
set -euo pipefail

binary=$1
version=$2
out=$3

app="$out/Fettle.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/fettle"
cp assets/Fettle.icns "$app/Contents/Resources/Fettle.icns"
cp LICENSE THIRD_PARTY_LICENSES.md "$app/Contents/Resources/"
sed "s/@VERSION@/$version/g" assets/Info.plist >"$app/Contents/Info.plist"
codesign --force --sign "${FETTLE_SIGN_IDENTITY:--}" --identifier dev.iyazerski.fettle "$app"
echo "$app"
