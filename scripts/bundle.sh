#!/usr/bin/env bash
# Build Fettle.app from a release binary: scripts/bundle.sh <binary> <version> <out-dir>
# With FETTLE_DEV=1, build Fettle Dev.app from a debug binary instead: its name,
# bundle ID and executable match `DEV` in src/main.rs, so it runs beside Fettle.app.
# Signs with the identity in FETTLE_SIGN_IDENTITY (from FETTLE_SIGN_KEYCHAIN if
# set), or ad hoc without one. macOS keeps the Accessibility grant across
# updates only for a stable identity: an ad-hoc signature changes with every build.
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
