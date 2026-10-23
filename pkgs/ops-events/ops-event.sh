# shellcheck shell=bash
# Usage: ops-event started UNIT | record INSTANCE | post INSTANCE
# The host's major events, as lines in #ops. The hooks (started, and
# record, which ops-notify@ runs first) run as root: they keep the little
# state there is and start ops-notify@<event>[:unit], whose sandboxed post
# writes the line and hands it to ops-notify.

root=${OPS_ROOT:-}
state=$root/run/ops-events

notify() {
  systemctl start --no-block "ops-notify@$1.service"
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
  if booting; then
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
  started | record | post | line) "$command" "$@" ;;
  *)
    echo "usage: ops-event started UNIT | record INSTANCE | post INSTANCE" >&2
    exit 2
    ;;
esac
