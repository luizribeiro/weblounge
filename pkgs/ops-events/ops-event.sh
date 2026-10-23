# shellcheck shell=bash
# Usage: ops-event started UNIT | record INSTANCE | switch TOPLEVEL ACTION
#                  | post INSTANCE
# The host's major events, as lines in #ops. The hooks (started, switch,
# and record, which ops-notify@ runs first) run as root: they keep the
# little state there is and start ops-notify@<event>[:unit], whose
# sandboxed post writes the line and hands it to ops-notify.

root=${OPS_ROOT:-}
state=$root/run/ops-events
switch_lock=$root/run/nixos/switch-to-configuration.lock
current_system=$root/run/current-system
# The trial's timer, as scripts/push names it.
rollback_timer=weblounge-rollback.timer

notify() {
  systemctl start --no-block "ops-notify@$1.service"
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

started() {
  local unit=$1
  if switching; then
    rm -f "$state/failed/$unit"
    echo "$unit" >>"$state/restarted"
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
record() {
  case $1 in
    failed:*)
      local unit=${1#failed:}
      [ "$(systemctl show -P ActiveState "$unit")" = failed ] || exit 1
      touch "$state/failed/$unit"
      ;;
  esac
}

# Runs before switch-to-configuration does anything, as a pre-switch check.
switch() {
  local toplevel=$1 action=$2 current
  current=$(readlink -f "$current_system")
  case $action in
    switch | test)
      [ "$toplevel" != "$current" ] || return 0
      : >"$state/restarted"
      if [ "$(cat "$state/trial" 2>/dev/null)" = "$toplevel" ]; then
        rm -f "$state/trial"
        notify rolled-back
      elif systemctl -q is-active "$rollback_timer"; then
        echo "$current" >"$state/trial"
        notify trial
      else
        rm -f "$state/trial"
        notify deployed
      fi
      ;;
    boot)
      if [ "$toplevel" = "$current" ] && [ -e "$state/trial" ]; then
        rm -f "$state/trial"
        notify kept
      fi
      ;;
  esac
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
  [ -s "$state/restarted" ] || return 0
  units=$(awk '!seen[$0]++' "$state/restarted" | paste -sd, | sed 's/,/, /g')
  [ -z "$units" ] || echo " — restarted: $units"
}

wait_for_switch() {
  local tries
  for ((tries = 0; tries < 600; tries++)); do
    switching || return 0
    sleep 1
  done
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
    deployed | trial | rolled-back)
      wait_for_switch
      local system
      system=$(revision_of "$current_system")
      case $event in
        deployed) echo "deployed $system$(restarted)" ;;
        trial) echo "deployed $system (on trial)$(restarted)" ;;
        rolled-back) echo "rolled back to $system$(restarted)" ;;
      esac
      ;;
    kept) echo "kept $(revision_of "$current_system")" ;;
    *)
      echo "ops-event: no such event: $event" >&2
      return 2
      ;;
  esac
}

post() {
  local line
  line=$(line "$1")
  ops-notify "$line"
}

umask 022
command=${1:-}
shift || true
case $command in
  started | record | switch | post | line) "$command" "$@" ;;
  *)
    echo "usage: ops-event started UNIT | record INSTANCE | switch TOPLEVEL ACTION | post INSTANCE" >&2
    exit 2
    ;;
esac
