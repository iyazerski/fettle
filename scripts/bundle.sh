#!/usr/bin/env bash
# Build Quill.app from a release binary: scripts/bundle.sh <binary> <version> <out-dir>
# Signs with the identity in QUILL_SIGN_IDENTITY, or ad hoc without one. macOS
# keeps the Accessibility grant across updates only for a stable identity: an
# ad-hoc signature changes with every build.
set -euo pipefail

binary=$1
version=$2
out=$3

app="$out/Quill.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/quill"
cp assets/Quill.icns "$app/Contents/Resources/Quill.icns"
cp LICENSE THIRD_PARTY_LICENSES.md "$app/Contents/Resources/"
sed "s/@VERSION@/$version/g" assets/Info.plist >"$app/Contents/Info.plist"
codesign --force --sign "${QUILL_SIGN_IDENTITY:--}" --identifier dev.iyazerski.quill "$app"
echo "$app"
