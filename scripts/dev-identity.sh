#!/usr/bin/env bash
# Make the self-signed "Fettle Dev" signing identity on first use, unlock its
# keychain, and print the keychain's path. `make dev` signs every Fettle Dev
# build with it: one stable identity lets macOS keep the Accessibility grant
# across rebuilds, which an ad-hoc signature can't.
#
# The identity lives in its own keychain, out of the search list, so nothing
# else sees it. That keychain's random password is in the login keychain.
set -euo pipefail

identity="Fettle Dev"
keychain="$HOME/Library/Keychains/fettle-dev.keychain-db"
service="Fettle Dev signing keychain"

if [ ! -f "$keychain" ]; then
  password=$(/usr/bin/openssl rand -hex 24)
  security add-generic-password -U -a "$USER" -s "$service" -w "$password"
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  printf '%s\n' '[req]' 'distinguished_name=dn' 'prompt=no' 'x509_extensions=ext' \
    '[dn]' "CN=$identity" \
    '[ext]' 'basicConstraints=critical,CA:false' 'keyUsage=critical,digitalSignature' \
    'extendedKeyUsage=critical,codeSigning' >"$work/openssl.cnf"
  # LibreSSL, which macOS ships, writes a PKCS#12 file that `security import` reads.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/openssl.cnf" \
    -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
  /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
    -out "$work/identity.p12" -passout pass:fettle
  security create-keychain -p "$password" "$keychain"
  # Never lock on a timer or on sleep, so signing needs no prompt.
  security set-keychain-settings "$keychain"
  security import "$work/identity.p12" -k "$keychain" -P fettle -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
fi

security unlock-keychain -p "$(security find-generic-password -a "$USER" -s "$service" -w)" "$keychain"
echo "$keychain"
