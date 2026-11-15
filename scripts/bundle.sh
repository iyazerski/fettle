#!/usr/bin/env bash
# Usage: scripts/bundle.sh <binary> <version> <out-dir>
# FETTLE_DEV must match the binary's build flavor in src/main.rs.
# Set FETTLE_SIGN_IDENTITY for a stable Accessibility grant across rebuilds.
set -euo pipefail

binary=$1
version=$2
out=$3

if [ "${FETTLE_DEV:-}" = 1 ]; then
  name="Fettle Dev"
  id=dev.iyazerski.fettle.dev
  executable=fettle-dev
else
  name=Fettle
  id=dev.iyazerski.fettle
  executable=fettle
fi

app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/$executable"
cp assets/Fettle.icns "$app/Contents/Resources/Fettle.icns"
cp LICENSE THIRD_PARTY_LICENSES.md "$app/Contents/Resources/"
sed -e "s/@VERSION@/$version/g" -e "s/@NAME@/$name/g" -e "s/@BUNDLE_ID@/$id/g" \
  -e "s/@EXECUTABLE@/$executable/g" assets/Info.plist >"$app/Contents/Info.plist"
codesign --force --sign "${FETTLE_SIGN_IDENTITY:--}" ${FETTLE_SIGN_KEYCHAIN:+--keychain "$FETTLE_SIGN_KEYCHAIN"} \
  --identifier "$id" "$app"
echo "$app"
