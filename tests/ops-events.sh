# shellcheck shell=bash
# Run by ops-events.nix, with ops-event and ops-notify on PATH.
set -euo pipefail

failures=0

setup() {
  OPS_ROOT=$(mktemp -d)
  export OPS_ROOT
  mkdir -p "$OPS_ROOT"/{stub,stub/active,run/ops-events/failed}
  echo running >"$OPS_ROOT/stub/system-state"
}

stub() {
  echo "$2" >"$OPS_ROOT/stub/$1"
}

calls() {
  cat "$OPS_ROOT/stub/calls" 2>/dev/null || true
}

expect() {
  local what=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$what" "$expected" "$actual" >&2
    failures=$((failures + 1))
  fi
}

setup
stub concierge.NRestarts 2
expect "a restart's line counts restarts" \
  "concierge crashed and restarted (restart #2) — journalctl -u concierge" "$(ops-event line crashed:concierge)"
expect "a start's line names the unit" "nginx started" "$(ops-event line started:nginx)"
expect "a recovery's line" "ergochat is back" "$(ops-event line back:ergochat)"
stub concierge.Restart on-failure
expect "a unit that restarts gave up" \
  "concierge failed — gave up restarting" "$(ops-event line failed:concierge)"
stub acme-order-renew-x.Restart no
expect "a unit that doesn't restart points at its journal" \
  "acme-order-renew-x failed — journalctl -u acme-order-renew-x" "$(ops-event line failed:acme-order-renew-x)"
expect "an unknown event is refused" "2" "$(ops-event line exploded:x 2>/dev/null || echo $?)"

setup
stub system-state starting
touch "$OPS_ROOT/run/ops-events/failed/concierge"
ops-event started concierge
expect "starts during a boot post nothing" "" "$(calls)"
expect "a boot clears failures" "" "$(ls "$OPS_ROOT/run/ops-events/failed")"

setup
touch "$OPS_ROOT/run/ops-events/failed/concierge"
stub concierge.NRestarts 0
stub ergochat.NRestarts 3
stub nginx.NRestarts 0
stub nginx.ActiveState active
stub ergochat.ActiveState active
stub concierge.ActiveState active
ops-event started concierge
ops-event started ergochat
ops-event started nginx
expect "a start after a failure is a recovery, an automatic one a crash, others a start" \
  "start --no-block ops-notify@back:concierge.service
start --no-block ops-notify@crashed:ergochat.service
start --no-block ops-notify@started:nginx.service" "$(calls)"
expect "a recovery clears the failure" "" "$(ls "$OPS_ROOT/run/ops-events/failed")"
rm "$OPS_ROOT/stub/calls"
stub nginx.ActiveState failed
ops-event started nginx
expect "a start that fails is left to the failure's line" "" "$(calls)"

setup
stub concierge.ActiveState activating
expect "a failure a restart follows is left to the restart's line" "1" "$(ops-event record failed:concierge || echo $?)"
stub concierge.ActiveState failed
ops-event record failed:concierge
expect "a unit left failed is remembered" "concierge" "$(ls "$OPS_ROOT/run/ops-events/failed")"
ops-event record booted
expect "other events need no record" "concierge" "$(ls "$OPS_ROOT/run/ops-events/failed")"

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
