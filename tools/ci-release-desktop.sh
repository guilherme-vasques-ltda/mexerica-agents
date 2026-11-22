#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in mac|linux|win) ;; *) exit 1 ;; esac
test -n "${MEXERICA_DESKTOP_MAIN_PLIST:-}" || {
  echo '::error::A configuracao publica da conta oficial esta ausente.'
  exit 1
}
backup=$(mktemp)
cp config/main.plist "$backup"
trap 'mv -f "$backup" config/main.plist' EXIT
printf '%s\n' "$MEXERICA_DESKTOP_MAIN_PLIST" > config/main.plist
unset MEXERICA_DESKTOP_MAIN_PLIST
npm run "dist:$1:official"
