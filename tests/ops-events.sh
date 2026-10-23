# shellcheck shell=bash
# Run by ops-events.nix, with ops-event and ops-notify on PATH.
set -euo pipefail

failures=0

setup() {
  OPS_ROOT=$(mktemp -d)
  export OPS_ROOT
  mkdir -p "$OPS_ROOT"/{stub/active,run/ops-events/failed,run/nixos,var/lib/ops-events,proc,systems}
  echo running >"$OPS_ROOT/stub/system-state"
  : >"$OPS_ROOT/proc/locks"
  touch "$OPS_ROOT/run/nixos/switch-to-configuration.lock"
  system old ""
  ln -sfn "$OPS_ROOT/systems/old" "$OPS_ROOT/run/current-system"
}

# A system whose nixos-version knows the revision given, if any.
system() {
  mkdir -p "$OPS_ROOT/systems/$1/sw/bin"
  local version=$OPS_ROOT/systems/$1/sw/bin/nixos-version
  if [ -n "$2" ]; then
    printf '#!%s\necho %s\n' "$BASH" "$2" >"$version"
  else
    printf '#!%s\nexit 1\n' "$BASH" >"$version"
  fi
  chmod +x "$version"
}

switch_to() {
  ln -sfn "$OPS_ROOT/systems/$1" "$OPS_ROOT/run/current-system"
}

hold_switch_lock() {
  local major minor inode
  read -r major minor inode < <(stat -c '%Hd %Ld %i' "$OPS_ROOT/run/nixos/switch-to-configuration.lock")
  printf '1: FLOCK  ADVISORY  WRITE 4242 %02x:%02x:%d 0 EOF\n' "$major" "$minor" "$inode" >"$OPS_ROOT/proc/locks"
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
system new 0123456789abcdef0123456789abcdef01234567
switch_to new
expect "a deploy names the revision, short" "deployed 0123456" "$(ops-event line deployed)"
printf '%s\n' concierge ergochat concierge >"$OPS_ROOT/run/ops-events/restarted"
expect "a deploy lists what it restarted, once each" \
  "deployed 0123456 (on trial) — restarted: concierge, ergochat" "$(ops-event line trial)"
system dirty 0123456789abcdef0123456789abcdef01234567-dirty
switch_to dirty
expect "a dirty tree says so" \
  "rolled back to dirty tree on 0123456 — restarted: concierge, ergochat" "$(ops-event line rolled-back)"
switch_to old
expect "a system without a revision says so" "kept an unknown revision" "$(ops-event line kept)"

setup
old=$(readlink -f "$OPS_ROOT/run/current-system")
system new 0123456789abcdef0123456789abcdef01234567
new=$OPS_ROOT/systems/new
echo stale >"$OPS_ROOT/run/ops-events/restarted"
touch "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$new" test
expect "a switch while a push's rollback is armed is a trial" "start --no-block ops-notify@trial.service" "$(calls)"
expect "a trial remembers the system it may go back to" "$old" "$(cat "$OPS_ROOT/run/ops-events/trial")"
expect "a switch forgets the last one's restarts" "" "$(cat "$OPS_ROOT/run/ops-events/restarted")"
switch_to new
rm "$OPS_ROOT/stub/calls" "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$new" boot
expect "making the trial's system the boot default keeps it" "start --no-block ops-notify@kept.service" "$(calls)"
rm "$OPS_ROOT/stub/calls"
ops-event switch "$new" boot
ops-event switch "$new" test
expect "booting or activating the running system again says nothing" "" "$(calls)"
ops-event switch "$old" switch
expect "a switch with no trial is a plain deploy" "start --no-block ops-notify@deployed.service" "$(calls)"

setup
old=$(readlink -f "$OPS_ROOT/run/current-system")
system new ""
touch "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$OPS_ROOT/systems/new" test
switch_to new
rm "$OPS_ROOT/stub/calls"
ops-event switch "$old" test
expect "a switch back to the trial's old system is a rollback" "start --no-block ops-notify@rolled-back.service" "$(calls)"
expect "a rollback ends the trial" "no" "$([ -e "$OPS_ROOT/run/ops-events/trial" ] && echo yes || echo no)"

setup
stub concierge.NRestarts 0
hold_switch_lock
touch "$OPS_ROOT/run/ops-events/failed/concierge"
ops-event started concierge
ops-event started nginx
expect "starts during a deploy post nothing themselves" "" "$(calls)"
expect "starts during a deploy go in its line" "concierge nginx" "$(xargs <"$OPS_ROOT/run/ops-events/restarted")"
expect "a deploy that restarts a failed unit clears its failure" "" "$(ls "$OPS_ROOT/run/ops-events/failed")"

setup
stub system-state starting
ops-event boot
expect "a first boot is a clean one" "start --no-block ops-notify@booted.service" "$(calls)"
rm "$OPS_ROOT/stub/calls"
ops-event boot
expect "a boot that finds the last one never shut down followed a crash" \
  "start --no-block ops-notify@crash-booted.service" "$(calls)"
rm "$OPS_ROOT/stub/calls" "$OPS_ROOT/var/lib/ops-events/up"
stub system-state running
ops-event boot
expect "a start outside a boot posts nothing, but marks the host up" "yes" \
  "$(calls)$([ -e "$OPS_ROOT/var/lib/ops-events/up" ] && echo yes)"
stub .FinishTimestampMonotonic 23400000
expect "a boot's line gives its length and kernel" \
  "credenza booted after 23s (kernel $(uname -r))" "$(ops-event line booted)"

setup
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#ops"]}'
stub jobs "  123 ctrl-alt-del.target start   waiting"
ops-event shutdown
stub jobs "124 poweroff.target start waiting"
ops-event shutdown
stub jobs ""
ops-event shutdown
expect "a reboot and a power-off say so, a stop by hand nothing" \
  "credenza rebooting
credenza shutting down" "$(cat "$OPS_ROOT/stub/sent")"
touch "$OPS_ROOT/var/lib/ops-events/up"
ops-event down
expect "a clean shutdown clears the marker" "no" "$([ -e "$OPS_ROOT/var/lib/ops-events/up" ] && echo yes || echo no)"

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
