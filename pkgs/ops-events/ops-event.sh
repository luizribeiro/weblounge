# shellcheck shell=bash
# Usage: ops-event started UNIT | failed UNIT | switch TOPLEVEL ACTION
#                  | boot | shutdown | down | drain | line INSTANCE
# The host's major events, as lines in #ops. The hooks (started, failed,
# switch, boot and down) run as root: they keep the little state there is
# and queue each event as <event>[:<unit or switch>]. drain, ops-post's
# one sandboxed run at a time, writes the queued events' lines and hands
# them to ops-notify oldest first, so #ops hears them in the order they
# happened. No unit starts while the host goes down, so shutdown posts
# its line itself.

root=${OPS_ROOT:-}
state=$root/run/ops-events
queue=$state/queue
switches=$state/switches
persistent=$root/var/lib/ops-events
switch_lock=$root/run/nixos/switch-to-configuration.lock
current_system=$root/run/current-system
# The trial's timer, as scripts/push names it.
rollback_timer=weblounge-rollback.timer

# Entries are numbered under a lock, so the queue's order is the events'.
notify() {
  local lock seq
  exec {lock}>>"$state/queue.lock"
  flock "$lock"
  seq=$(($(cat "$state/queued" 2>/dev/null || echo 0) + 1))
  echo "$seq" >"$state/queued"
  echo "$1" >"$state/entry"
  mv "$state/entry" "$queue/$(printf '%010d' "$seq")"
  exec {lock}>&-
}

# switch-to-configuration holds its lock, an flock, from start to end.
switching() {
  local major minor inode
  [ -e "$switch_lock" ] || return 1
  read -r major minor inode < <(stat -c '%Hd %Ld %i' "$switch_lock")
  grep -q " $(printf '%02x:%02x:%d' "$major" "$minor" "$inode") " "$root/proc/locks"
}

booting() {
  case $(systemctl is-system-running) in
    initializing | starting) return 0 ;;
    *) return 1 ;;
  esac
}

# Up to 5 minutes; a unit that fails to start has its own line.
wait_until_up() {
  local tries
  for ((tries = 0; tries < 300; tries++)); do
    case $(systemctl show -P ActiveState "$1") in
      active) return 0 ;;
      failed | inactive) return 1 ;;
    esac
    sleep 1
  done
  return 1
}

# The switch running now, which the units it restarts are listed in.
current_switch() {
  cat "$state/switch" 2>/dev/null || true
}

started() {
  local unit=$1 restarted
  if switching; then
    rm -f "$state/failed/$unit"
    restarted=$switches/$(current_switch)/restarted
    if [ -e "$restarted" ]; then echo "$unit" >>"$restarted"; fi
  elif booting; then
    rm -f "$state/failed/$unit"
  elif ! wait_until_up "$unit"; then
    return 0
  elif [ -e "$state/failed/$unit" ]; then
    rm -f "$state/failed/$unit"
    notify "back:$unit"
  elif (($(systemctl show -P NRestarts "$unit") > 0)); then
    notify "crashed:$unit"
  else
    notify "started:$unit"
  fi
}

# OnFailure= fires on every crash a restart follows too; only a unit
# left failed has given up.
failed() {
  local unit=$1
  [ "$(systemctl show -P ActiveState "$unit")" = failed ] || return 0
  touch "$state/failed/$unit"
  notify "failed:$unit"
}

# Whose lines have gone out, so nothing reads them again.
forget_posted_switches() {
  local dir
  for dir in "$switches"/*; do
    [ -d "$dir" ] || continue
    grep -qsx -- "[a-z-]*:${dir##*/}" "$queue"/* || rm -r "$dir"
  done
}

# A numbered record of the switch to toplevel, which its line describes
# however late it's written: the units it restarts go in it.
record_switch() {
  local toplevel=$1 id
  forget_posted_switches
  id=$(($(current_switch) + 1))
  mkdir -p "$switches/$id"
  echo "$toplevel" >"$switches/$id/system"
  : >"$switches/$id/restarted"
  echo "$id" >"$state/switch"
  echo "$id"
}

# Runs before switch-to-configuration does anything, as a pre-switch check.
switch() {
  local toplevel=$1 action=$2 current
  current=$(readlink -f "$current_system")
  case $action in
    switch | test)
      [ "$toplevel" != "$current" ] || return 0
      if [ "$(cat "$state/trial" 2>/dev/null)" = "$toplevel" ]; then
        rm -f "$state/trial"
        notify "rolled-back:$(record_switch "$toplevel")"
      elif systemctl -q is-active "$rollback_timer"; then
        echo "$current" >"$state/trial"
        notify "trial:$(record_switch "$toplevel")"
      else
        rm -f "$state/trial"
        notify "deployed:$(record_switch "$toplevel")"
      fi
      ;;
    boot)
      if [ "$toplevel" = "$current" ] && [ -e "$state/trial" ]; then
        rm -f "$state/trial"
        notify "kept:$(record_switch "$toplevel")"
      fi
      ;;
  esac
}

# A crash seconds after the marker changes would otherwise beat the
# filesystem's next commit to disk.
persist() {
  sync -f "$persistent"
}

# The marker lives from boot to a clean shutdown, so a boot that finds it
# follows a crash or a power loss.
boot() {
  if booting; then
    if [ -e "$persistent/up" ]; then notify crash-booted; else notify booted; fi
  fi
  touch "$persistent/up"
  persist
}

shutdown() {
  local jobs event
  jobs=$(systemctl list-jobs --no-legend)
  if grep -qE '(^| )(reboot|ctrl-alt-del|kexec|soft-reboot)\.target ' <<<"$jobs"; then
    event=rebooting
  elif grep -qE '(^| )(poweroff|halt)\.target ' <<<"$jobs"; then
    event=shutting-down
  fi
  [ -z "${event:-}" ] || post "$event"
}

down() {
  rm -f "$persistent/up"
  persist
}

revision_of() {
  local rev
  if ! rev=$("$1/sw/bin/nixos-version" --configuration-revision 2>/dev/null); then
    echo "an unknown revision"
  elif [[ $rev == *-dirty ]]; then
    echo "dirty tree on ${rev:0:7}"
  else
    echo "${rev:0:7}"
  fi
}

restarted() {
  local units
  [ -s "$switches/$1/restarted" ] || return 0
  units=$(awk '!seen[$0]++' "$switches/$1/restarted" | paste -sd, | sed 's/,/, /g')
  [ -z "$units" ] || echo " — restarted: $units"
}

# Until the switch is done: no switch holds the lock, or a later one does.
wait_for_switch() {
  local tries
  for ((tries = 0; tries < 600; tries++)); do
    if ! switching || [ "$(current_switch)" != "$1" ]; then return 0; fi
    sleep 1
  done
}

boot_seconds() {
  local finished
  timeout 600 systemctl is-system-running --wait >/dev/null || true
  finished=$(systemctl show -P FinishTimestampMonotonic)
  ((finished > 0)) || finished=$(awk '{ printf "%d", $1 * 1000000 }' "$root/proc/uptime")
  echo $(((finished + 500000) / 1000000))
}

line() {
  local event=${1%%:*} unit=${1#*:}
  case $event in
    started) echo "$unit started" ;;
    crashed) echo "$unit crashed and restarted (restart #$(systemctl show -P NRestarts "$unit")) — journalctl -u $unit" ;;
    back) echo "$unit is back" ;;
    failed)
      if [ "$(systemctl show -P Restart "$unit")" = no ]; then
        echo "$unit failed — journalctl -u $unit"
      else
        echo "$unit failed — gave up restarting"
      fi
      ;;
    deployed | trial | rolled-back | kept)
      local id=$unit system
      wait_for_switch "$id"
      system=$(revision_of "$(cat "$switches/$id/system")")
      case $event in
        deployed) echo "deployed $system$(restarted "$id")" ;;
        trial) echo "deployed $system (on trial)$(restarted "$id")" ;;
        rolled-back) echo "rolled back to $system$(restarted "$id")" ;;
        kept) echo "kept $system" ;;
      esac
      ;;
    booted) echo "$OPS_HOST booted after $(boot_seconds)s (kernel $(uname -r))" ;;
    crash-booted) echo "$OPS_HOST booted after a crash or power loss (kernel $(uname -r))" ;;
    rebooting) echo "$OPS_HOST rebooting" ;;
    shutting-down) echo "$OPS_HOST shutting down" ;;
    *)
      echo "ops-event: no such event: $event" >&2
      return 2
      ;;
  esac
}

post() {
  local line
  line=$(line "$1") || return
  case $1 in
    # The host is going down, and won't wait long.
    rebooting | shutting-down) OPS_NOTIFY_PATIENCE=15 ops-notify "$line" ;;
    # louro comes up last in a boot, behind Ergo, concierge and bender's account.
    booted | crash-booted) OPS_NOTIFY_PATIENCE=600 ops-notify "$line" ;;
    *) ops-notify "$line" ;;
  esac
}

# A line ops-notify gives up on is dropped, so those behind it go out.
drain() {
  local entries
  while entries=("$queue"/*) && [ -e "${entries[0]}" ]; do
    post "$(cat "${entries[0]}")" || true
    rm -f "${entries[0]}"
  done
}

umask 022
command=${1:-}
shift || true
case $command in
  started | failed | switch | boot | shutdown | down | drain | line) "$command" "$@" ;;
  *)
    echo "usage: ops-event started UNIT | failed UNIT | switch TOPLEVEL ACTION | boot | shutdown | down | drain | line INSTANCE" >&2
    exit 2
    ;;
esac
