# shellcheck shell=bash
# Run by ops-events.nix, with ops-event and ops-notify on PATH.
set -euo pipefail

failures=0

setup() {
  OPS_ROOT=$(mktemp -d)
  export OPS_ROOT
  mkdir -p "$OPS_ROOT"/{stub/active,run/ops-events/{failed,queue,switches},run/nixos,var/lib/ops-events,proc,systems}
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

release_switch_lock() {
  : >"$OPS_ROOT/proc/locks"
}

stub() {
  echo "$2" >"$OPS_ROOT/stub/$1"
}

# The events waiting for ops-post, oldest first.
queued() {
  local entry
  for entry in "$OPS_ROOT/run/ops-events/queue"/*; do
    if [ -e "$entry" ]; then cat "$entry"; fi
  done
}

clear_queue() {
  rm -f "$OPS_ROOT/run/ops-events/queue"/*
}

sent() {
  cat "$OPS_ROOT/stub/sent" 2>/dev/null || true
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
expect "starts during a boot post nothing" "" "$(queued)"
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
  "back:concierge
crashed:ergochat
started:nginx" "$(queued)"
expect "a recovery clears the failure" "" "$(ls "$OPS_ROOT/run/ops-events/failed")"
clear_queue
stub nginx.ActiveState failed
ops-event started nginx
expect "a start that fails is left to the failure's line" "" "$(queued)"

setup
stub concierge.ActiveState activating
ops-event failed concierge
expect "a failure a restart follows is left to the restart's line" "" "$(queued)$(ls "$OPS_ROOT/run/ops-events/failed")"
stub concierge.ActiveState failed
ops-event failed concierge
expect "a unit left failed is remembered" "concierge" "$(ls "$OPS_ROOT/run/ops-events/failed")"
expect "and its failure queued" "failed:concierge" "$(queued)"

setup
system new 0123456789abcdef0123456789abcdef01234567
ops-event switch "$OPS_ROOT/systems/new" test
expect "a deploy names the revision, short" "deployed 0123456" "$(ops-event line deployed:1)"
printf '%s\n' concierge ergochat concierge >"$OPS_ROOT/run/ops-events/switches/1/restarted"
expect "a deploy lists what it restarted, once each" \
  "deployed 0123456 (on trial) — restarted: concierge, ergochat" "$(ops-event line trial:1)"
switch_to new
system dirty 0123456789abcdef0123456789abcdef01234567-dirty
ops-event switch "$OPS_ROOT/systems/dirty" test
expect "a dirty tree says so, and a switch lists only its own restarts" \
  "rolled back to dirty tree on 0123456" "$(ops-event line rolled-back:2)"
switch_to dirty
ops-event switch "$OPS_ROOT/systems/old" test
expect "a system without a revision says so" "kept an unknown revision" "$(ops-event line kept:3)"

setup
old=$(readlink -f "$OPS_ROOT/run/current-system")
system new 0123456789abcdef0123456789abcdef01234567
new=$OPS_ROOT/systems/new
touch "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$new" test
expect "a switch while a push's rollback is armed is a trial" "trial:1" "$(queued)"
expect "a trial remembers the system it may go back to" "$old" "$(cat "$OPS_ROOT/run/ops-events/trial")"
switch_to new
clear_queue
rm "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$new" boot
expect "making the trial's system the boot default keeps it" "kept:2" "$(queued)"
clear_queue
ops-event switch "$new" boot
ops-event switch "$new" test
expect "booting or activating the running system again says nothing" "" "$(queued)"
ops-event switch "$old" switch
expect "a switch with no trial is a plain deploy" "deployed:3" "$(queued)"
expect "a switch forgets the records of switches whose lines went out" "3" "$(ls "$OPS_ROOT/run/ops-events/switches")"

setup
old=$(readlink -f "$OPS_ROOT/run/current-system")
system new ""
touch "$OPS_ROOT/stub/active/weblounge-rollback.timer"
ops-event switch "$OPS_ROOT/systems/new" test
switch_to new
clear_queue
ops-event switch "$old" test
expect "a switch back to the trial's old system is a rollback" "rolled-back:2" "$(queued)"
expect "a rollback ends the trial" "no" "$([ -e "$OPS_ROOT/run/ops-events/trial" ] && echo yes || echo no)"

setup
system new ""
ops-event switch "$OPS_ROOT/systems/new" test
stub concierge.NRestarts 0
hold_switch_lock
touch "$OPS_ROOT/run/ops-events/failed/concierge"
ops-event started concierge
ops-event started nginx
expect "starts during a deploy post nothing themselves" "deployed:1" "$(queued)"
expect "starts during a deploy go in its line" "concierge nginx" "$(xargs <"$OPS_ROOT/run/ops-events/switches/1/restarted")"
expect "a deploy that restarts a failed unit clears its failure" "" "$(ls "$OPS_ROOT/run/ops-events/failed")"

setup
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#ops"]}'
old=$(readlink -f "$OPS_ROOT/run/current-system")
system new 0123456789abcdef0123456789abcdef01234567
touch "$OPS_ROOT/stub/active/weblounge-rollback.timer"
hold_switch_lock
ops-event switch "$OPS_ROOT/systems/new" test
switch_to new
ops-event started concierge
ops-event switch "$old" test
switch_to old
ops-event started nginx
expect "a switch's line is done once a later switch holds the lock, and names its own system and restarts" \
  "deployed 0123456 (on trial) — restarted: concierge" "$(ops-event line trial:1)"
release_switch_lock
ops-event drain
expect "the queue goes out oldest first, each switch with what it restarted" \
  "deployed 0123456 (on trial) — restarted: concierge
rolled back to an unknown revision — restarted: nginx" "$(sent)"
expect "and goes out once" "" "$(queued)"

setup
rm -f "$OPS_ROOT/stub/louro-status"
stub nginx.ActiveState active
stub nginx.NRestarts 0
ops-event started nginx
OPS_NOTIFY_PATIENCE=0 ops-event drain 2>/dev/null
expect "a line louro can't take is dropped" "" "$(queued)$(sent)"
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#ops"]}'
echo "exploded:x" >"$OPS_ROOT/run/ops-events/queue/0000000001"
echo "started:nginx" >"$OPS_ROOT/run/ops-events/queue/0000000002"
ops-event drain 2>/dev/null
expect "and the ones behind a line that fails still go out" "nginx started" "$(sent)"

setup
system new 0123456789abcdef0123456789abcdef01234567
switch_to new
ops-event deploying "$OPS_ROOT/systems/next" fedcba9876543210fedcba9876543210fedcba98-dirty
ops-event deploying "$OPS_ROOT/systems/later" fedcba9
expect "a push queues what it deploys and the revision it deploys over" \
  "deploying:fedcba9876543210fedcba9876543210fedcba98-dirty 0123456789abcdef0123456789abcdef01234567
deploying:fedcba9 0123456789abcdef0123456789abcdef01234567" "$(queued)"
expect "a deploy's start names both revisions, short" \
  "deploying dirty tree on fedcba9 (from 0123456)" \
  "$(ops-event line "deploying:fedcba9876543210fedcba9876543210fedcba98-dirty 0123456789abcdef0123456789abcdef01234567")"
clear_queue
ops-event deploying "$OPS_ROOT/systems/new" 0123456789abcdef0123456789abcdef01234567
expect "a push of the running system says nothing" "" "$(queued)"
system odd "release 1.0"
switch_to odd
ops-event deploying "$OPS_ROOT/systems/next" unknown
expect "a revision of neither side's is unknown" \
  "deploying an unknown revision (from an unknown revision)" "$(ops-event line "$(queued)")"
clear_queue
for revision in "" abc 0123456789abcdef0123456789abcdef012345678 FEDCBA9 "fedcba9 hi" "fedcba9
hi" fedcba9-clean "unknown!"; do
  expect "a push can't queue \"$revision\" as a revision" "2" \
    "$(ops-event deploying "$OPS_ROOT/systems/next" "$revision" 2>/dev/null || echo $?)"
done
expect "and queues nothing for it" "" "$(queued)"

setup
system new 0123456789abcdef0123456789abcdef01234567
switch_to new
ops-event gave-up "$OPS_ROOT/systems/next" fedcba9876543210fedcba9876543210fedcba98
ops-event gave-up "$OPS_ROOT/systems/new" 0123456789abcdef0123456789abcdef01234567
expect "a push that stops before switching says so, unless it pushed the running system" \
  "gave-up:fedcba9876543210fedcba9876543210fedcba98" "$(queued)"
expect "a push that stops before switching names what it didn't deploy" \
  "gave up deploying fedcba9" "$(ops-event line "$(queued)")"
expect "a push can't queue anything but a revision as one it gave up on" "2" \
  "$(ops-event gave-up "$OPS_ROOT/systems/next" "it broke" 2>/dev/null || echo $?)"

setup
stub system-state starting
ops-event boot
expect "a first boot is a clean one" "booted" "$(queued)"
clear_queue
ops-event boot
expect "a boot that finds the last one never shut down followed a crash" "crash-booted" "$(queued)"
clear_queue
rm "$OPS_ROOT/var/lib/ops-events/up"
stub system-state running
ops-event boot
expect "a start outside a boot posts nothing, but marks the host up" "yes" \
  "$(queued)$([ -e "$OPS_ROOT/var/lib/ops-events/up" ] && echo yes)"
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
credenza shutting down" "$(sent)"
touch "$OPS_ROOT/var/lib/ops-events/up"
ops-event down
expect "a clean shutdown clears the marker" "no" "$([ -e "$OPS_ROOT/var/lib/ops-events/up" ] && echo yes || echo no)"

setup
expect "ops-notify needs a line" "2" "$(ops-notify 2>/dev/null || echo $?)"
expect "ops-notify gives up while louro isn't running" "1" "$(OPS_NOTIFY_PATIENCE=0 ops-notify hello 2>/dev/null || echo $?)"
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#bots"]}'
expect "ops-notify gives up while louro isn't in #ops" "1" "$(OPS_NOTIFY_PATIENCE=0 ops-notify hello 2>/dev/null || echo $?)"
expect "and sends nothing meanwhile" "" "$(sent)"
stub louro-status '{"connected": true, "nick": "bender", "channels": ["#ops", "#bots"]}'
ops-notify credenza booted after 23s
expect "ops-notify sends once louro is in #ops" "credenza booted after 23s" "$(sent)"
stub send-status 2
expect "ops-notify doesn't retry a line louro refuses" "2" "$(OPS_NOTIFY_PATIENCE=60 ops-notify hello 2>/dev/null || echo $?)"

if ((failures > 0)); then
  echo "$failures failed" >&2
  exit 1
fi
