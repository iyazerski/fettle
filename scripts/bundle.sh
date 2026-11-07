#!/usr/bin/env bash
# Build Quill.app from a release binary: scripts/bundle.sh <binary> <version> <out-dir>
# The bundle is signed ad hoc: macOS needs a signature to run it on Apple
# silicon, and Accessibility remembers the app by it.
set -euo pipefail

binary=$1
version=$2
out=$3

app="$out/Quill.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/quill"
cp assets/Quill.icns "$app/Contents/Resources/Quill.icns"
sed "s/@VERSION@/$version/g" assets/Info.plist >"$app/Contents/Info.plist"
codesign --force --sign - --identifier dev.iyazerski.quill "$app"
echo "$app"
