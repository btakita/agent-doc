#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <ed25519-private-key.pem> <plugin.zip> <signature-output>" >&2
  exit 2
fi

private_key=$1
package=$2
signature=$3

test -s "$private_key"
test -s "$package"
openssl pkeyutl -sign -rawin -inkey "$private_key" -in "$package" -out "$signature"
openssl pkeyutl -verify -rawin -pubin \
  -inkey <(openssl pkey -in "$private_key" -pubout 2>/dev/null) \
  -in "$package" -sigfile "$signature" >/dev/null
