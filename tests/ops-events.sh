# shellcheck shell=bash
# Run by ops-events.nix, with ops-notify on PATH.
set -euo pipefail

failures=0

setup() {
  OPS_ROOT=$(mktemp -d)
  export OPS_ROOT
  mkdir -p "$OPS_ROOT/stub"
}

stub() {
  echo "$2" >"$OPS_ROOT/stub/$1"
}

expect() {
  local what=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$what" "$expected" "$actual" >&2
    failures=$((failures + 1))
  fi
}

setup
expect "ops-notify needs a line" "2" "$(ops-notify 2>/dev/null || echo $?)"
expect "ops-notify gives up while louro isn't running" "1" "$(OPS_NOTIFY_PATIENCE=0 ops-notify hello 2>/dev/null || echo $?)"
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#bots"]}'
expect "ops-notify gives up while louro isn't in #ops" "1" "$(OPS_NOTIFY_PATIENCE=0 ops-notify hello 2>/dev/null || echo $?)"
expect "and sends nothing meanwhile" "" "$(cat "$OPS_ROOT/stub/sent" 2>/dev/null || true)"
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#ops", "#bots"]}'
ops-notify credenza booted after 23s
expect "ops-notify sends once louro is in #ops" "credenza booted after 23s" "$(cat "$OPS_ROOT/stub/sent")"
stub send-status 2
expect "ops-notify doesn't retry a line louro refuses" "2" "$(OPS_NOTIFY_PATIENCE=60 ops-notify hello 2>/dev/null || echo $?)"

if ((failures > 0)); then
  echo "$failures failed" >&2
  exit 1
fi
